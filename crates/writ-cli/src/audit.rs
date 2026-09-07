//! `writ audit`. Spec section 7.1.
//!
//! Two halves. Without `--ingest` it reads a diff, selects the learnings
//! that apply, and prints a prompt. With `--ingest` it reads the findings
//! the host sent back, writes them, and exits `1` when a blocking one
//! landed. That exit code is the gate the Stop hook uses.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use writ_core::{
    AuditScope, BucketMetric, Budget, Config, CounterMetric, Diff, Error, FindingOutcomeMetric,
    GateResultMetric, Ingested, LanguageMetric, MatcherResultMetric, Outcome, Result, Selected,
    Store, TelemetryBatch, parse_findings, rank, render_prompt,
};

use crate::git;
use crate::hook::{self, Host};
use crate::matcher::{Verdict, evaluate};
use crate::output::AuditFormat;

/// What a dry run prints where an audit id would be.
///
/// It is deliberately not a UUID, because nothing was recorded and
/// nothing can be ingested against it: `--ingest` says so with exit `5`.
/// It is deliberately the **same width** as a UUID, because a preview
/// that is five bytes short of the real prompt is not a preview.
const DRY_RUN_ID: &str = "dry-run-nothing-recorded-no-audit-id";

/// Every flag in the `writ audit` row of spec section 5.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Git range. Defaults to the working tree against HEAD
    #[arg(long, value_name = "RANGE", conflicts_with = "ingest")]
    pub diff: Option<String>,

    /// prompt, json or text
    #[arg(long, default_value_t = AuditFormat::Prompt, value_name = "FORMAT")]
    pub format: AuditFormat,

    /// Read findings JSON on stdin, write rows, update counters
    #[arg(long)]
    pub ingest: bool,

    /// The most learnings one prompt may carry
    #[arg(long = "max-rules", value_name = "N", conflicts_with = "ingest")]
    pub max_rules: Option<u32>,

    /// The most characters of rule text one prompt may carry
    #[arg(long = "max-chars", value_name = "N", conflicts_with = "ingest")]
    pub max_chars: Option<u32>,

    /// Select, rank and render, but write nothing. No audit row and no
    /// counters, so the findings cannot be ingested
    #[arg(long = "dry-run", conflicts_with = "ingest")]
    pub dry_run: bool,

    /// Emit the verdict in a host's gate protocol: claude-code, codex or
    /// cursor. Spec section 9.2
    ///
    /// It conflicts with `--ingest` because both want stdin, and the two
    /// documents are different: `--ingest` reads findings, `--hook` reads
    /// the host's retry signal. Reading one and guessing at the other is
    /// the silent misread P7 forbids.
    #[arg(long, value_name = "HOST", conflicts_with = "ingest")]
    pub hook: Option<Host>,

    /// How many times Cursor may resubmit before the gate gives up
    #[arg(
        long = "loop-limit",
        value_name = "N",
        default_value_t = hook::DEFAULT_LOOP_LIMIT,
        requires = "hook",
    )]
    pub loop_limit: u64,
}

/// What one selection run produced, before anything is printed.
///
/// [`select`] returns this and prints nothing, so the CLI, the MCP tool
/// and the host gate all render the same result three ways instead of
/// three code paths computing it three times. Invariant 1 keeps this out
/// of `writ-core`, because building it runs git.
#[derive(Debug)]
pub struct Selection {
    /// The `audits` row this run wrote, or [`DRY_RUN_ID`].
    pub audit_id: String,
    /// The repository and the diff the run looked at.
    pub scope: AuditScope,
    /// How many learnings the scope query returned.
    pub considered: usize,
    /// The learnings that fit the budget.
    pub selected: Vec<Selected>,
    /// Things worth saying on stderr. They are not the result, so the
    /// caller decides whether a hook protocol wants them.
    pub notices: Vec<String>,
    /// Aggregate-only observations produced while selecting.
    pub telemetry: TelemetryBatch,
}

impl Selection {
    /// Whether the gate should send the agent back. Spec section 9.2.
    ///
    /// **Any** selected learning gates, blocking or advisory. `blocking`
    /// decides whether an unfixed violation stops the work at ingest, not
    /// whether the agent is sent back to review. Gating here on
    /// `blocking` would select an advisory learning, count it against the
    /// budget, and drop it unread.
    pub fn gates(&self) -> bool {
        !self.selected.is_empty()
    }

    /// The prompt the host sends back to the agent.
    pub fn prompt(&self) -> String {
        render_prompt(&self.audit_id, &self.scope, &self.selected)
    }
}

/// Run the command and return the process exit code.
pub fn run(args: &Args, db: &Path, config: &Config) -> Result<(ExitCode, TelemetryBatch)> {
    if args.ingest {
        return ingest(args, db);
    }
    if let Some(host) = args.hook {
        return hook::run(host, args, db, config);
    }
    emit(args, db, config)
}

/// Steps 1 to 4: scope, select, budget, emit.
fn emit(args: &Args, db: &Path, config: &Config) -> Result<(ExitCode, TelemetryBatch)> {
    let mut run = select(args, db, config)?;
    for notice in &run.notices {
        eprintln!("{notice}");
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.format {
        AuditFormat::Prompt => {
            let _ = write!(out, "{}", run.prompt());
        }
        AuditFormat::Json => {
            let _ = writeln!(out, "{}", report(&run, args.dry_run));
        }
        AuditFormat::Text => {
            let _ = writeln!(out, "audit {}", run.audit_id);
            let _ = writeln!(
                out,
                "{} on {}: {} considered, {} sent",
                run.scope.diff_range,
                run.scope.identity.value(),
                run.considered,
                run.selected.len()
            );
            for one in &run.selected {
                let _ = writeln!(
                    out,
                    "  {}  {:<9}  {}",
                    one.learning.id,
                    if one.learning.blocking {
                        "blocking"
                    } else {
                        "advisory"
                    },
                    one.learning.title
                );
            }
        }
    }
    let telemetry = std::mem::take(&mut run.telemetry);
    Ok((ExitCode::SUCCESS, telemetry))
}

/// The `--format json` body. The MCP tool returns this same value, so the
/// two surfaces cannot describe one audit differently.
pub fn report(run: &Selection, dry_run: bool) -> serde_json::Value {
    serde_json::json!({
        "audit_id": run.audit_id,
        "repo": run.scope.identity.value(),
        "repo_identity_is_path_fallback": run.scope.identity.is_fallback(),
        "diff_range": run.scope.diff_range,
        "considered": run.considered,
        "sent": run.selected.len(),
        "dry_run": dry_run,
        "learnings": run.selected
            .iter()
            .map(|one| serde_json::json!({
                "id": one.learning.id,
                "title": one.learning.title,
                "rule": one.learning.rule,
                "rationale": one.learning.rationale,
                "blocking": one.learning.blocking,
                "exemplars": one.exemplars,
            }))
            .collect::<Vec<_>>(),
    })
}

/// Steps 1 to 3: scope, select, budget. This prints nothing.
pub fn select(args: &Args, db: &Path, config: &Config) -> Result<Selection> {
    let cwd = std::env::current_dir().map_err(|error| Error::Command {
        program: "getcwd".into(),
        message: error.to_string(),
    })?;
    let repo = git::discover(&cwd)?;
    let (text, range) = git::diff(&repo.root, args.diff.as_deref())?;
    let diff = Diff::parse(&text);
    if diff.is_empty() {
        return Err(Error::EmptyDiff { range });
    }
    let mut telemetry = TelemetryBatch::default();
    telemetry
        .buckets
        .push(BucketMetric::DiffFiles(diff.paths.len() as u64));
    for path in &diff.paths {
        telemetry
            .counters
            .push(CounterMetric::Language(LanguageMetric::from_path(path)));
    }

    let scope = AuditScope {
        identity: repo.identity,
        diff,
        diff_range: range,
    };
    let budget = Budget {
        max_rules: args.max_rules.unwrap_or(config.audit.max_rules),
        max_chars: args.max_chars.unwrap_or(config.audit.max_chars),
    };

    let mut store = Store::open(db)?;
    let mut candidates = store.candidates(&scope)?;
    let considered = candidates.len();
    let mut notices = Vec::new();

    // A matcher hit selects a learning. It never creates a finding.
    candidates.retain(|candidate| {
        telemetry
            .counters
            .push(CounterMetric::MatcherKind(candidate.learning.matcher_kind));
        let verdict = evaluate(&candidate.learning, &scope.diff, &repo.root);
        if candidate.learning.matcher_kind.is_some() {
            telemetry
                .counters
                .push(CounterMetric::MatcherResult(match &verdict {
                    Verdict::Hit => MatcherResultMetric::Hit,
                    Verdict::Miss | Verdict::MissWithNotice(_) => MatcherResultMetric::Miss,
                    Verdict::Unevaluable(_) => MatcherResultMetric::Unevaluable,
                }));
        }
        if let Verdict::Unevaluable(why) = &verdict {
            notices.push(format!(
                "writ: keeping {} on scope alone: {why}",
                candidate.learning.id
            ));
        }
        if let Verdict::MissWithNotice(why) = &verdict {
            notices.push(format!(
                "writ: matcher {} fell back to the post-image only: {why}",
                candidate.learning.id
            ));
        }
        verdict.keeps()
    });

    rank(&mut candidates);
    let selected = store.take_budget(&candidates, &budget)?;
    telemetry
        .buckets
        .push(BucketMetric::AuditConsidered(considered as u64));
    telemetry
        .buckets
        .push(BucketMetric::AuditSent(selected.len() as u64));

    // A dry run writes nothing: no `audits` row and no `times_selected`.
    // Seeing what an audit would select had no cost-free path before, and
    // emit moves up to `max_rules` counters. The placeholder is not an id
    // on purpose, so an ingest of a dry run fails with exit 5 rather than
    // attaching findings to some other audit.
    let audit_id = if args.dry_run {
        DRY_RUN_ID.to_string()
    } else {
        store.start_audit(
            scope.identity.value(),
            &scope.diff_range,
            considered,
            &selected,
        )?
    };

    if scope.identity.is_fallback() {
        notices.push(format!(
            "writ: this repository has no git remote, so its identity is the checkout path {}. \
             A project: scope recorded against a remote will not match it.",
            scope.identity.value()
        ));
    }

    let mut run = Selection {
        audit_id,
        scope,
        considered,
        selected,
        notices,
        telemetry,
    };
    run.telemetry.buckets.push(BucketMetric::PromptChars(
        run.prompt().chars().count() as u64
    ));
    Ok(run)
}

/// Steps 5 and 6: write the findings, then gate on them.
fn ingest(args: &Args, db: &Path) -> Result<(ExitCode, TelemetryBatch)> {
    let (written, telemetry) = ingest_with_telemetry(&read_stdin()?, db)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.format {
        AuditFormat::Json => {
            let text = serde_json::to_string(&written).expect("Ingested serializes");
            let _ = writeln!(out, "{text}");
        }
        _ => {
            let _ = writeln!(
                out,
                "audit {}: {} findings, {} blocking, {} not fixed",
                written.audit_id, written.findings, written.blocking, written.unfixed_blocking
            );
        }
    }

    // Step 6. A blocking finding the agent fixed lets the handoff
    // through. An advisory finding never stops it. Anything else does.
    if written.unfixed_blocking > 0 {
        return Ok((ExitCode::from(1), telemetry));
    }
    Ok((ExitCode::SUCCESS, telemetry))
}

/// Write the findings and update the counters.
///
/// `--ingest` reads the same text off stdin and the MCP tool passes it as
/// an argument, because stdin there is the transport. Both land here, so
/// neither surface can drift from the other.
pub fn ingest_text(text: &str, db: &Path) -> Result<Ingested> {
    ingest_with_telemetry(text, db).map(|(written, _)| written)
}

/// Write findings and return the aggregate observations for the caller's
/// surface. This keeps CLI and MCP ingestion on one validation/storage path.
pub fn ingest_with_telemetry(text: &str, db: &Path) -> Result<(Ingested, TelemetryBatch)> {
    let input = parse_findings(text)?;
    let mut telemetry = TelemetryBatch::default();
    telemetry
        .buckets
        .push(BucketMetric::AuditFindings(input.findings.len() as u64));
    for finding in &input.findings {
        let outcome = match finding.outcome {
            Outcome::Fixed => Some(FindingOutcomeMetric::Fixed),
            Outcome::Ignored => Some(FindingOutcomeMetric::Ignored),
            Outcome::Rejected => Some(FindingOutcomeMetric::Rejected),
            Outcome::Open => None,
        };
        if let Some(outcome) = outcome {
            telemetry
                .counters
                .push(CounterMetric::FindingOutcome(outcome));
        }
    }
    let mut store = Store::open(db)?;
    let written = store.ingest(&input)?;
    telemetry
        .counters
        .push(CounterMetric::GateResult(if written.unfixed_blocking > 0 {
            GateResultMetric::Block
        } else {
            GateResultMetric::Pass
        }));
    Ok((written, telemetry))
}

/// Read the whole stream, and never wait on a person. crit #693.
fn read_stdin() -> Result<String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(Error::Validation {
            message: "--ingest reads findings JSON on stdin. Pipe a file in".to_string(),
        });
    }
    let mut text = String::new();
    stdin
        .read_to_string(&mut text)
        .map_err(|error| Error::Validation {
            message: format!("cannot read stdin: {error}"),
        })?;
    Ok(text)
}
