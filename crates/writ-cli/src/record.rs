//! `writ record`, the only way into the database. Spec sections 5 and 7.2.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;

use writ_core::{
    Config, CounterMetric, Error, ExemplarKind, LanguageMetric, MatcherKind, NewExemplar,
    NewLearning, RecordSourceMetric, RecordStatusMetric, Recorded, Result, Scope, ScopeKind,
    SourceKind, Status, Store, TelemetryBatch, parse_jsonl,
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

    /// The snippet inline: good:TEXT or bad:TEXT
    ///
    /// This is the form MCP uses. An agent holds snippet text, not a
    /// path, so without it the MCP surface cannot attach an exemplar at
    /// all -- and an exemplar is what makes a rule teach instead of
    /// assert. Invariant 3 holds either way: both forms store text.
    #[arg(
        long = "example-text",
        value_name = "KIND:TEXT",
        conflicts_with = "json"
    )]
    example_texts: Vec<String>,

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

    /// Attach to an existing learning instead of creating one
    #[arg(long, value_name = "ID")]
    reinforce: Option<String>,

    /// text or json
    #[arg(long, default_value_t = Format::Text, value_name = "FORMAT")]
    pub format: Format,
}

/// Run the command and print what it wrote.
pub fn run(args: &Args, db: &Path, config: &Config) -> Result<TelemetryBatch> {
    let (written, telemetry) = execute_with_telemetry(args, db, config)?;
    report(&written, args.format)?;
    Ok(telemetry)
}

/// Write the learnings and return them. This prints nothing.
///
/// The MCP tool calls this, so the two surfaces cannot validate or store
/// one learning differently. Spec section 9.1.
pub fn execute(args: &Args, db: &Path, config: &Config) -> Result<Vec<Recorded>> {
    execute_with_telemetry(args, db, config).map(|(written, _)| written)
}

/// Write learnings and return aggregate-only observations to the calling
/// surface without changing the shared validation or storage path.
pub fn execute_with_telemetry(
    args: &Args,
    db: &Path,
    config: &Config,
) -> Result<(Vec<Recorded>, TelemetryBatch)> {
    let status = args.requested_status()?;
    let exemplars = args.exemplars()?;
    let mut store = Store::open(db)?;
    let mut telemetry = TelemetryBatch::default();
    telemetry
        .counters
        .push(CounterMetric::RecordSource(if args.json {
            RecordSourceMetric::Json
        } else {
            RecordSourceMetric::Manual
        }));

    let written = if let Some(id) = &args.reinforce {
        vec![store.reinforce(id, &exemplars, status)?]
    } else if args.json {
        let mut learnings = parse_jsonl(&read_stdin()?)?;
        let author = resolve_author(config);
        for learning in &mut learnings {
            learning.status = learning.status.or(status);
            learning.author = learning.author.clone().or_else(|| author.clone());
            observe_learning(&mut telemetry, learning);
        }
        store.record_many(&learnings)?
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
        observe_learning(&mut telemetry, &learning);
        vec![store.record(&learning)?]
    };

    Ok((written, telemetry))
}

fn observe_learning(batch: &mut TelemetryBatch, learning: &NewLearning) {
    batch
        .counters
        .push(CounterMetric::RecordStatus(match learning.status {
            Some(Status::Active) => RecordStatusMetric::Active,
            _ => RecordStatusMetric::Proposed,
        }));
    batch
        .counters
        .push(CounterMetric::MatcherKind(learning.matcher_kind));
    if learning.scopes.is_empty() {
        batch
            .counters
            .push(CounterMetric::ScopeKind(ScopeKind::Global));
        return;
    }
    for scope in &learning.scopes {
        batch.counters.push(CounterMetric::ScopeKind(scope.kind));
        if scope.kind == ScopeKind::Language {
            batch
                .counters
                .push(CounterMetric::Language(LanguageMetric::from_label(
                    &scope.value,
                )));
        }
    }
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

    /// Every exemplar, from both forms. Files first, then inline text.
    fn exemplars(&self) -> Result<Vec<NewExemplar>> {
        let mut out = Vec::with_capacity(self.examples.len() + self.example_texts.len());
        for text in &self.examples {
            let (kind, path) = split_kind(text, "--example", "FILE")?;
            let snippet = read_snippet(Path::new(path))?;
            if snippet.is_empty() {
                return Err(Error::Validation {
                    message: format!("the example file {path} is empty"),
                });
            }
            out.push(exemplar(kind, snippet));
        }
        out.extend(parse_example_texts(&self.example_texts)?);
        Ok(out)
    }
}

/// Split `good:REST`, and say which flag wanted it.
pub(crate) fn split_kind<'a>(
    text: &'a str,
    flag: &str,
    rest: &str,
) -> Result<(ExemplarKind, &'a str)> {
    let (kind, value) = text.split_once(':').ok_or_else(|| Error::Validation {
        message: format!("{flag} takes good:{rest} or bad:{rest}, not {text}"),
    })?;
    Ok((kind.parse()?, value))
}

pub(crate) fn exemplar(kind: ExemplarKind, snippet: String) -> NewExemplar {
    NewExemplar {
        kind,
        language: None,
        snippet,
        note: None,
    }
}

/// Parse repeated `--example-text KIND:TEXT` values. Shared by `record` and
/// `edit` so the two commands refuse the same empty snippet the same way.
pub(crate) fn parse_example_texts(texts: &[String]) -> Result<Vec<NewExemplar>> {
    texts
        .iter()
        .map(|text| {
            let (kind, snippet) = split_kind(text, "--example-text", "TEXT")?;
            if snippet.is_empty() {
                return Err(Error::Validation {
                    message: "an --example-text snippet is empty".to_string(),
                });
            }
            Ok(exemplar(kind, snippet.to_string()))
        })
        .collect()
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
            }
        }
    }
    Ok(())
}
