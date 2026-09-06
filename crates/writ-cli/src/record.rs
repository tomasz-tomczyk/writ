//! `writ record`, the only way into the database. Spec sections 5 and 7.2.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;

use writ_core::{
    Config, Error, ExemplarKind, MatcherKind, NearMatch, NewExemplar, NewLearning, Recorded,
    Result, Scope, SourceKind, Status, Store, parse_jsonl,
};

use crate::context::{read_snippet, resolve_author};
use crate::output::Format;

/// Every flag in the `writ record` row of spec section 5.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// A short name for the learning
    #[arg(
        long,
        value_name = "TEXT",
        required_unless_present_any = ["json", "reinforce"],
        conflicts_with_all = ["json", "reinforce"],
    )]
    title: Option<String>,

    /// What to do
    #[arg(
        long,
        value_name = "TEXT",
        required_unless_present_any = ["json", "reinforce"],
        conflicts_with_all = ["json", "reinforce"],
    )]
    rule: Option<String>,

    /// Why the rule exists. Required, and never empty
    #[arg(
        long,
        value_name = "TEXT",
        required_unless_present_any = ["json", "reinforce"],
        conflicts_with_all = ["json", "reinforce"],
    )]
    rationale: Option<String>,

    /// Where it applies: global, project:ID, language:LANG or glob:PAT
    #[arg(
        long = "scope",
        value_name = "KIND:VALUE",
        conflicts_with_all = ["json", "reinforce"],
    )]
    scopes: Vec<String>,

    /// Demote to report-only. Blocking is the default
    #[arg(long, conflicts_with_all = ["json", "reinforce"])]
    advisory: bool,

    /// Copy a snippet in: good:FILE or bad:FILE. Never a path reference
    #[arg(long = "example", value_name = "KIND:FILE", conflicts_with = "json")]
    examples: Vec<String>,

    /// A retrieval pattern. Needs --matcher-kind
    #[arg(
        long,
        value_name = "PATTERN",
        conflicts_with_all = ["json", "reinforce"],
    )]
    matcher: Option<String>,

    /// The dialect of --matcher: ast_grep or regex
    #[arg(
        long = "matcher-kind",
        value_name = "KIND",
        conflicts_with_all = ["json", "reinforce"],
    )]
    matcher_kind: Option<String>,

    /// proposed or active. Default proposed
    #[arg(long, value_name = "STATUS", conflicts_with = "activate")]
    status: Option<String>,

    /// Sugar for --status active
    #[arg(long)]
    activate: bool,

    /// Read JSONL on stdin, one learning per line
    #[arg(long)]
    json: bool,

    /// Write despite a near-match block. No effect in MVP
    #[arg(long)]
    force: bool,

    /// Attach to an existing learning instead of creating one
    #[arg(long, value_name = "ID")]
    reinforce: Option<String>,

    /// text or json
    #[arg(long, default_value_t = Format::Text, value_name = "FORMAT")]
    format: Format,
}

/// Run the command. Returns the process exit code.
pub fn run(args: &Args, db: &Path, config: &Config) -> Result<()> {
    // `--force` is accepted so the flag exists the day `block_above` is
    // calibrated. Nothing refuses a write while block_above is false, so
    // there is nothing for it to override. Spec section 7.3.
    let _ = args.force;

    let status = args.requested_status()?;
    let exemplars = args.exemplars()?;
    let warn_top_n = config.dedupe.warn_top_n;
    let mut store = Store::open(db)?;

    let written = if let Some(id) = &args.reinforce {
        vec![store.reinforce(id, &exemplars, status)?]
    } else if args.json {
        let mut learnings = parse_jsonl(&read_stdin()?)?;
        let author = resolve_author(config);
        for learning in &mut learnings {
            learning.status = learning.status.or(status);
            learning.author = learning.author.clone().or_else(|| author.clone());
        }
        store.record_many(&learnings, warn_top_n)?
    } else {
        let mut learning = NewLearning::new(
            args.title.clone().unwrap_or_default(),
            args.rule.clone().unwrap_or_default(),
            args.rationale.clone().unwrap_or_default(),
        );
        learning.scopes = args.scopes()?;
        learning.blocking = !args.advisory;
        learning.matcher = args.matcher.clone();
        learning.matcher_kind = args
            .matcher_kind
            .as_deref()
            .map(str::parse::<MatcherKind>)
            .transpose()?;
        learning.exemplars = exemplars;
        learning.status = status;
        learning.author = resolve_author(config);
        learning.source_kind = Some(SourceKind::Manual);
        vec![store.record(&learning, warn_top_n)?]
    };

    report(&written, args.format)
}

impl Args {
    fn requested_status(&self) -> Result<Option<Status>> {
        if self.activate {
            return Ok(Some(Status::Active));
        }
        self.status
            .as_deref()
            .map(Status::parse_writable)
            .transpose()
    }

    fn scopes(&self) -> Result<Vec<Scope>> {
        self.scopes.iter().map(|text| text.parse()).collect()
    }

    /// Parse `good:FILE` and copy the snippet text in.
    fn exemplars(&self) -> Result<Vec<NewExemplar>> {
        self.examples
            .iter()
            .map(|text| {
                let (kind, path) = text.split_once(':').ok_or_else(|| Error::Validation {
                    message: format!("--example takes good:FILE or bad:FILE, not {text}"),
                })?;
                let kind: ExemplarKind = kind.parse()?;
                let snippet = read_snippet(Path::new(path))?;
                if snippet.is_empty() {
                    return Err(Error::Validation {
                        message: format!("the example file {path} is empty"),
                    });
                }
                Ok(NewExemplar {
                    kind,
                    language: None,
                    snippet,
                    note: None,
                })
            })
            .collect()
    }
}

/// Read the whole stream, and never wait on a person.
///
/// A terminal on stdin means nobody piped anything in, so the command says
/// what it wanted instead of hanging. crit #693.
///
/// The `is_terminal()` branch below has no test, and that is deliberate,
/// not an oversight. The integration harness gives the child process a
/// pipe or a closed handle, so a test cannot put a terminal on its stdin
/// without a pty. Anyone adding one needs that pty.
fn read_stdin() -> Result<String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(Error::Validation {
            message: "--json reads JSONL on stdin. Pipe a file in".to_string(),
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

/// Say what was written. crit #446: no silent no-ops on user data.
fn report(written: &[Recorded], format: Format) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match format {
        Format::Json => {
            let text = serde_json::to_string(written).expect("Recorded serializes");
            let _ = writeln!(out, "{text}");
        }
        Format::Text => {
            for record in written {
                let verb = if record.reinforced {
                    "reinforced"
                } else {
                    "recorded"
                };
                let _ = writeln!(out, "{verb} {}", record.id);
                warn_near_matches(&record.near_matches);
            }
        }
    }
    Ok(())
}

/// MVP warns and always writes. The warning is not the result, so it goes
/// to stderr and leaves stdout parseable. Spec section 7.3.
fn warn_near_matches(matches: &[NearMatch]) {
    for near in matches {
        eprintln!(
            "writ: near match {} {:?} ({}, bm25 {:.2})",
            near.id,
            near.title,
            near.status.as_str(),
            near.score
        );
    }
    if !matches.is_empty() {
        eprintln!("writ: recorded anyway. Use --reinforce ID to attach to one of these instead");
    }
}
