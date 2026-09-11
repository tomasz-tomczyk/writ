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
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
///
/// What it writes is the **pointer**, not the prompt. Spec section 9.2,
/// **The gate points, it does not paste**. Every host renders its block
/// message into the transcript verbatim, so pasting a prompt that carries
/// the whole diff puts 100 KB of a long branch in front of the developer
/// after every turn. The pointer is three lines; the document behind it
/// reaches the agent through `writ_audit`, which the host collapses.
fn emit(host: Host, run: &Selection) -> ExitCode {
    let pointer = run.pointer();
    match host {
        // Claude Code feeds a Stop hook's stderr back to the model when
        // the hook exits 2. stdout stays empty, because on exit 2 it is
        // not read at all.
        Host::ClaudeCode => {
            eprint!("{pointer}");
            ExitCode::from(2)
        }
        Host::Codex => {
            write_json(&serde_json::json!({
                "decision": "block",
                "reason": pointer,
            }));
            ExitCode::SUCCESS
        }
        // Cursor differs in kind, not only in shape: it does not stop the
        // stop, it submits a follow-up user turn.
        Host::Cursor => {
            write_json(&serde_json::json!({ "followup_message": pointer }));
            ExitCode::SUCCESS
        }
    }
}

fn write_json(body: &serde_json::Value) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{body}");
}

/// Read the host payload, and never wait. P7 and crit #693.
///
/// A terminal on stdin means a human ran the command by hand, so there is
/// no payload and no reason to block on one.
///
/// Everything else is read under a deadline rather than to end of file. A
/// Stop hook inherits whatever stdin the host had. When that is a pipe
/// nobody writes to and nobody closes, end of file never arrives and
/// `read_to_string` parks the gate for the life of the session. Absent
/// stdin is not an error here — the spec says a hook legitimately runs
/// with no payload on some hosts — so the deadline yields "no retry
/// signal" and the gate carries on.
fn read_payload() -> String {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return String::new();
    }
    read_within(stdin, PAYLOAD_WAIT)
}

/// How long the gate waits for a host payload before deciding there is none.
///
/// Generous rather than tight, because the two failures are not
/// symmetric. A pause of this length once per turn costs nothing. Losing a
/// retry signal that arrived late makes the gate block a second time, and
/// on Cursor the cap that bounds the follow-up loop is the payload. The
/// wait is also not paid in practice: a complete JSON document ends it as
/// soon as it parses.
const PAYLOAD_WAIT: Duration = Duration::from_secs(2);

/// Read `source` until it ends, until it holds one complete JSON document,
/// or until `limit` runs out, whichever comes first.
///
/// The read runs on its own thread because there is no portable way to
/// abandon a blocking read on the thread that issued it. The thread is
/// detached on purpose: it may still be parked inside `read` when the
/// deadline passes, and the process exits moments later and takes it.
///
/// Stopping on a parseable document, rather than on end of file, is what
/// makes the common path free. A host that writes its payload and leaves
/// the pipe open is served at once instead of at the deadline.
fn read_within<R: Read + Send + 'static>(mut source: R, limit: Duration) -> String {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match source.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if sender.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let deadline = Instant::now() + limit;
    let mut bytes = Vec::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        // An error is either end of file or the deadline. Both mean the
        // payload is whatever has arrived so far, which may be nothing.
        let Ok(chunk) = receiver.recv_timeout(remaining) else {
            break;
        };
        bytes.extend_from_slice(&chunk);
        if std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            .is_some()
        {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that never yields a byte and never ends, which is what an
    /// inherited pipe with no writer looks like.
    struct Silent;

    impl Read for Silent {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(60));
            Ok(0)
        }
    }

    #[test]
    fn a_silent_source_returns_empty_at_the_deadline() {
        let started = Instant::now();
        let payload = read_within(Silent, Duration::from_millis(50));
        assert_eq!(payload, "");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_complete_document_does_not_wait_for_the_source_to_end() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        std::io::Write::write_all(&mut writer, br#"{"stop_hook_active": true}"#).unwrap();

        // `writer` stays alive, so the read end never reaches end of file.
        let payload = read_within(reader, Duration::from_secs(30));

        assert!(Retry::parse(&payload).stop_hook_active, "{payload}");
        drop(writer);
    }
}
