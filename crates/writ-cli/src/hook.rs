//! `writ audit --hook HOST`. Spec section 9.2.
//!
//! Every host wants the same verdict in a different protocol. One tested
//! code path emits all three, rather than a shell wrapper per host that
//! drifts from the others. That is P8 applied to the piece most likely to
//! break it.
//!
//! Hook entry gates on **any** selected learning, blocking or advisory.
//! `blocking` decides whether an unfixed violation stops the work at
//! ingest, not whether the agent is sent back to review. Section 9.2.
//!
//! The gate is otherwise quiet. A Stop hook fires after every turn,
//! including the turns that wrote nothing, so a selection of nothing
//! leaves stdout empty and exits `0`.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use writ_core::{
    Config, CounterMetric, Error, GateResultMetric, HookHostMetric, Result, TelemetryBatch,
};

use crate::audit::{self, Args, Selection};

/// How many times Cursor may resubmit the turn before the gate gives up.
///
/// Cursor does not block a stop, it submits a follow-up user turn, so
/// nothing else bounds the loop. Section 9.2 names 5.
pub const DEFAULT_LOOP_LIMIT: u64 = 5;

/// The hosts that have a gate. Spec section 9.2.
///
/// OpenCode is absent on purpose. Every one of its plugin hooks returns
/// `Promise<void>` and `session.idle` reaches only the fire-and-forget
/// `event` hook, so nothing there can block a turn or inject a prompt.
/// Accepting `--hook opencode` and exiting `0` would promise a gate the
/// host cannot enforce, which is the crit #873 failure with worse
/// consequences than a wrong flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Host {
    /// `Stop` hook. Exit 2, findings on stderr.
    ClaudeCode,
    /// `Stop` hook. `{"decision":"block"}` on stdout, exit 0.
    Codex,
    /// `stop` hook. `{"followup_message":...}` on stdout, exit 0.
    Cursor,
}

/// What the host said about the turn it is ending.
///
/// Both fields default to "this is the first attempt", so a host that
/// sends nothing, sends something writ cannot parse, or sends a payload
/// with neither key still gets one block. Treating an unreadable payload
/// as "already retried" would silently disarm the gate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Claude Code and Codex: this turn was already continued once.
    pub stop_hook_active: bool,
    /// Cursor: how many follow-up turns the gate has already submitted.
    pub loop_count: u64,
}

impl Retry {
    /// Read the signal out of a host payload. An unreadable payload reads
    /// as a first attempt.
    pub fn parse(payload: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
            return Self::default();
        };
        Self {
            stop_hook_active: value
                .get("stop_hook_active")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            loop_count: value
                .get("loop_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        }
    }

    /// Whether the cap is reached, so this turn must be allowed to stop.
    ///
    /// An agent that reports `ignored` produces the same finding on the
    /// next audit, forever. writ cannot see that, because it sees each
    /// audit fresh, so the host's own retry signal is the only bound.
    /// Spec sections 9.2 and 9.3.
    pub fn spent(self, host: Host, loop_limit: u64) -> bool {
        match host {
            Host::ClaudeCode | Host::Codex => self.stop_hook_active,
            Host::Cursor => self.loop_count >= loop_limit,
        }
    }
}

/// Run the audit and emit the verdict in the host's protocol.
pub fn run(
    host: Host,
    args: &Args,
    db: &Path,
    config: &Config,
) -> Result<(ExitCode, TelemetryBatch)> {
    let retry = Retry::parse(&read_payload());

    let mut run = match audit::select(args, db, config) {
        Ok(run) => run,
        // A turn that touched no code, or a session outside a repository,
        // has nothing to gate. Exiting 6 or 7 would put a red error in
        // front of the user after every such turn. The cause still goes
        // to stderr, so the pass-through is not silent.
        Err(error @ (Error::EmptyDiff { .. } | Error::NotAGitRepository { .. })) => {
            eprintln!("writ: {error}. Nothing to audit, so the gate passes.");
            return Ok((
                ExitCode::SUCCESS,
                gate_telemetry(host, GateResultMetric::Pass),
            ));
        }
        Err(error) => return Err(error),
    };

    for notice in &run.notices {
        eprintln!("{notice}");
    }

    if !run.gates() {
        run.telemetry
            .counters
            .extend(gate_counters(host, GateResultMetric::Pass));
        return Ok((ExitCode::SUCCESS, run.telemetry));
    }
    if retry.spent(host, args.loop_limit) {
        eprintln!(
            "writ: {} learning(s) still apply, and the retry cap is reached. \
             Letting the turn stop.",
            run.selected.len()
        );
        run.telemetry
            .counters
            .extend(gate_counters(host, GateResultMetric::RetryCapped));
        return Ok((ExitCode::SUCCESS, run.telemetry));
    }

    let code = emit(host, &run);
    run.telemetry
        .counters
        .extend(gate_counters(host, GateResultMetric::Block));
    Ok((code, run.telemetry))
}

fn gate_counters(host: Host, result: GateResultMetric) -> [CounterMetric; 2] {
    let host = match host {
        Host::ClaudeCode => HookHostMetric::ClaudeCode,
        Host::Codex => HookHostMetric::Codex,
        Host::Cursor => HookHostMetric::Cursor,
    };
    [
        CounterMetric::HookHost(host),
        CounterMetric::GateResult(result),
    ]
}

fn gate_telemetry(host: Host, result: GateResultMetric) -> TelemetryBatch {
    TelemetryBatch {
        counters: gate_counters(host, result).into(),
        buckets: Vec::new(),
    }
}

/// Write the block in the host's protocol and return its exit code.
fn emit(host: Host, run: &Selection) -> ExitCode {
    let prompt = run.prompt();
    match host {
        // Claude Code feeds a Stop hook's stderr back to the model when
        // the hook exits 2. stdout stays empty, because on exit 2 it is
        // not read at all.
        Host::ClaudeCode => {
            eprint!("{prompt}");
            ExitCode::from(2)
        }
        Host::Codex => {
            write_json(&serde_json::json!({
                "decision": "block",
                "reason": prompt,
            }));
            ExitCode::SUCCESS
        }
        // Cursor differs in kind, not only in shape: it does not stop the
        // stop, it submits a follow-up user turn.
        Host::Cursor => {
            write_json(&serde_json::json!({ "followup_message": prompt }));
            ExitCode::SUCCESS
        }
    }
}

fn write_json(body: &serde_json::Value) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{body}");
}

/// Read the host payload, and never wait on a person. crit #693.
///
/// A terminal on stdin means a human ran the command by hand, so there is
/// no payload to read and no reason to block on one. Anything else is
/// read to end of file: the hosts close the pipe after writing.
fn read_payload() -> String {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return String::new();
    }
    let mut text = String::new();
    let _ = stdin.read_to_string(&mut text);
    text
}
