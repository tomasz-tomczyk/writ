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
    AuditScope, Budget, Config, Diff, Error, Result, Store, parse_findings, rank, render_prompt,
};

use crate::git;
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
    diff: Option<String>,

    /// prompt, json or text
    #[arg(long, default_value_t = AuditFormat::Prompt, value_name = "FORMAT")]
    format: AuditFormat,

    /// Read findings JSON on stdin, write rows, update counters
    #[arg(long)]
    ingest: bool,

    /// The most learnings one prompt may carry
    #[arg(long = "max-rules", value_name = "N", conflicts_with = "ingest")]
    max_rules: Option<u32>,

    /// The most characters of rule text one prompt may carry
    #[arg(long = "max-chars", value_name = "N", conflicts_with = "ingest")]
    max_chars: Option<u32>,

    /// Select, rank and render, but write nothing. No audit row and no
    /// counters, so the findings cannot be ingested
    #[arg(long = "dry-run", conflicts_with = "ingest")]
    dry_run: bool,
}

/// Run the command and return the process exit code.
pub fn run(args: &Args, db: &Path, config: &Config) -> Result<ExitCode> {
    if args.ingest {
        return ingest(args, db);
    }
    emit(args, db, config)
}

/// Steps 1 to 4: scope, select, budget, emit.
fn emit(args: &Args, db: &Path, config: &Config) -> Result<ExitCode> {
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

    // A matcher hit selects a learning. It never creates a finding.
    candidates.retain(|candidate| {
        let verdict = evaluate(&candidate.learning, &scope.diff, &repo.root);
        if let Verdict::Unevaluable(why) = &verdict {
            eprintln!(
                "writ: keeping {} on scope alone: {why}",
                candidate.learning.id
            );
        }
        verdict.keeps()
    });

    rank(&mut candidates);
    let selected = store.take_budget(&candidates, &budget)?;

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
        eprintln!(
            "writ: this repository has no git remote, so its identity is the checkout path {}. \
             A project: scope recorded against a remote will not match it.",
            scope.identity.value()
        );
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.format {
        AuditFormat::Prompt => {
            let prompt = render_prompt(&audit_id, &scope, &selected);
            let _ = write!(out, "{prompt}");
        }
        AuditFormat::Json => {
            let report = serde_json::json!({
                "audit_id": audit_id,
                "repo": scope.identity.value(),
                "repo_identity_is_path_fallback": scope.identity.is_fallback(),
                "diff_range": scope.diff_range,
                "considered": considered,
                "sent": selected.len(),
                "dry_run": args.dry_run,
                "learnings": selected
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
            });
            let _ = writeln!(out, "{report}");
        }
        AuditFormat::Text => {
            let _ = writeln!(out, "audit {audit_id}");
            let _ = writeln!(
                out,
                "{} on {}: {considered} considered, {} sent",
                scope.diff_range,
                scope.identity.value(),
                selected.len()
            );
            for one in &selected {
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
    Ok(ExitCode::SUCCESS)
}

/// Steps 5 and 6: write the findings, then gate on them.
fn ingest(args: &Args, db: &Path) -> Result<ExitCode> {
    let input = parse_findings(&read_stdin()?)?;
    let mut store = Store::open(db)?;
    let written = store.ingest(&input)?;

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
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
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
