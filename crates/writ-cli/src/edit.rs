//! `writ edit ID`, the agent-facing way to mutate an existing learning.
//!
//! The Detail UI can already edit; this command and the `writ_edit` MCP
//! tool give agents the same ability without opening a browser. Spec
//! section 5 (the CLI table) and section 9.1 (MCP tools).

use std::io::Write;
use std::path::Path;

use writ_core::{
    Error, Exemplar, Learning, LearningUpdate, MatcherKind, NewExemplar, Result, Scope, Status,
    Store, TelemetryBatch,
};

use crate::output::Format;
use crate::record;

/// Every flag in the `writ edit ID` row of spec section 5.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The learning to edit
    #[arg(value_name = "ID")]
    id: String,

    /// A short name for the learning
    #[arg(long, value_name = "TEXT")]
    title: Option<String>,

    /// What to do
    #[arg(long, value_name = "TEXT")]
    rule: Option<String>,

    /// Why the rule exists. Required, and never empty
    #[arg(long, value_name = "TEXT")]
    rationale: Option<String>,

    /// Where it applies: global, project:ID, language:LANG or glob:PAT.
    /// Any --scope replaces the full set
    #[arg(long = "scope", value_name = "KIND:VALUE")]
    scopes: Vec<String>,

    /// Demote to report-only
    #[arg(long, conflicts_with = "blocking")]
    advisory: bool,

    /// Promote to blocking (the default for new learnings)
    #[arg(long, conflicts_with = "advisory")]
    blocking: bool,

    /// A retrieval pattern. Needs --matcher-kind
    #[arg(long, value_name = "PATTERN", conflicts_with = "clear_matcher")]
    matcher: Option<String>,

    /// The dialect of --matcher: ast_grep or regex
    #[arg(
        long = "matcher-kind",
        value_name = "KIND",
        conflicts_with = "clear_matcher"
    )]
    matcher_kind: Option<String>,

    /// Remove any matcher and matcher-kind
    #[arg(long = "clear-matcher", conflicts_with_all = ["matcher", "matcher_kind"])]
    clear_matcher: bool,

    /// The snippet inline: good:TEXT or bad:TEXT. Any --example-text
    /// replaces the full exemplar set
    #[arg(long = "example-text", value_name = "KIND:TEXT")]
    example_texts: Vec<String>,

    /// Sugar for proposed → active after a successful edit. No-op if
    /// already active. Rejected for archived learnings
    #[arg(long)]
    activate: bool,

    /// text or json
    #[arg(long, default_value_t = Format::Text, value_name = "FORMAT")]
    pub format: Format,
}

/// Run the command and report what changed.
pub fn run(args: &Args, db: &Path) -> Result<TelemetryBatch> {
    let learning = execute(args, db)?;
    report(&learning, args.format)?;
    Ok(TelemetryBatch::default())
}

/// Apply the edit and return the learning as it now stands. This prints
/// nothing, so the MCP tool can share the same path.
pub fn execute(args: &Args, db: &Path) -> Result<Learning> {
    let mut store = Store::open(db)?;
    let current = store.get(&args.id)?;
    let current_exemplars = store.exemplars_of(&args.id)?;

    if args.activate && current.status == Status::Archived {
        return Err(Error::Validation {
            message: format!(
                "learning {} is archived; activate it with `writ archive` first",
                args.id
            ),
        });
    }

    if !args.has_mutation() {
        return Err(Error::Validation {
            message: "nothing to edit; pass at least one of --title, --rule, --rationale, \
                 --scope, --advisory, --blocking, --matcher, --matcher-kind, \
                 --clear-matcher, --example-text, or --activate"
                .to_string(),
        });
    }

    let update = build_update(args, &current, &current_exemplars)?;
    let status = (args.activate && current.status == Status::Proposed).then_some(Status::Active);
    store.update_learning_and_set_status(&args.id, &update, status)?;

    store.get(&args.id)
}

impl Args {
    fn has_mutation(&self) -> bool {
        self.title.is_some()
            || self.rule.is_some()
            || self.rationale.is_some()
            || !self.scopes.is_empty()
            || self.advisory
            || self.blocking
            || self.matcher.is_some()
            || self.matcher_kind.is_some()
            || self.clear_matcher
            || !self.example_texts.is_empty()
            || self.activate
    }

    fn scopes(&self) -> Result<Vec<Scope>> {
        self.scopes.iter().map(|text| text.parse()).collect()
    }
}

fn build_update(
    args: &Args,
    current: &Learning,
    current_exemplars: &[Exemplar],
) -> Result<LearningUpdate> {
    let (matcher, matcher_kind) = resolve_matcher(args, current)?;
    let exemplars = if args.example_texts.is_empty() {
        current_exemplars.iter().map(as_new_exemplar).collect()
    } else {
        record::parse_example_texts(&args.example_texts)?
    };

    Ok(LearningUpdate {
        title: keep_text(&args.title, &current.title),
        rule: keep_text(&args.rule, &current.rule),
        rationale: keep_text(&args.rationale, &current.rationale),
        blocking: match (args.advisory, args.blocking) {
            (true, _) => false,
            (_, true) => true,
            _ => current.blocking,
        },
        matcher_kind,
        matcher,
        scopes: if args.scopes.is_empty() {
            current.scopes.clone()
        } else {
            args.scopes()?
        },
        exemplars,
    })
}

/// Keep the current value unless the flag was passed; trim when it was.
fn keep_text(next: &Option<String>, current: &str) -> String {
    next.as_deref()
        .map(str::trim)
        .map(String::from)
        .unwrap_or_else(|| current.to_string())
}

fn resolve_matcher(
    args: &Args,
    current: &Learning,
) -> Result<(Option<String>, Option<MatcherKind>)> {
    if args.clear_matcher {
        return Ok((None, None));
    }

    // Touching either matcher flag replaces the pair as a unit: a kind-only
    // edit clears the pattern (args.matcher is None). Validation refuses a
    // pattern with no kind when the learning has none either.
    let matcher = if args.matcher.is_some() || args.matcher_kind.is_some() {
        args.matcher.clone()
    } else {
        current.matcher.clone()
    };
    let matcher_kind = match args.matcher_kind.as_deref() {
        Some(kind) => Some(kind.parse()?),
        None => current.matcher_kind,
    };
    Ok((matcher, matcher_kind))
}

fn as_new_exemplar(exemplar: &Exemplar) -> NewExemplar {
    NewExemplar {
        kind: exemplar.kind,
        language: exemplar.language.clone(),
        snippet: exemplar.snippet.clone(),
        note: exemplar.note.clone(),
    }
}

/// Say what was edited. crit #446: no silent no-ops on user data.
fn report(learning: &Learning, format: Format) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match format {
        Format::Json => {
            let text = serde_json::to_string(learning).expect("the report serializes");
            let _ = writeln!(out, "{text}");
        }
        Format::Text => {
            let _ = writeln!(out, "edited {}", learning.id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::{Exemplar, ExemplarKind, Scope, ScopeKind, SourceKind};

    fn sample_learning() -> (Learning, Vec<Exemplar>) {
        let learning = Learning {
            id: "id".into(),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
            status: Status::Proposed,
            title: "title".into(),
            rule: "rule".into(),
            rationale: "rationale".into(),
            blocking: true,
            matcher_kind: None,
            matcher: None,
            source_kind: SourceKind::Manual,
            source_adapter: None,
            source_ref: None,
            author: None,
            activated_at: None,
            reinforced: 0,
            times_selected: 0,
            last_selected_at: None,
            times_applied: 0,
            last_applied_at: None,
            last_verified: None,
            scopes: vec![Scope {
                kind: ScopeKind::Language,
                value: "rust".into(),
            }],
        };
        let exemplars = vec![Exemplar {
            id: "ex".into(),
            kind: ExemplarKind::Bad,
            language: None,
            snippet: "bad".into(),
            note: None,
        }];
        (learning, exemplars)
    }

    fn bare_args() -> Args {
        Args {
            id: "id".into(),
            title: None,
            rule: None,
            rationale: None,
            scopes: vec![],
            advisory: false,
            blocking: false,
            matcher: None,
            matcher_kind: None,
            clear_matcher: false,
            example_texts: vec![],
            activate: false,
            format: Format::Text,
        }
    }

    #[test]
    fn omitted_fields_keep_current_values() {
        let (current, exemplars) = sample_learning();
        let mut args = bare_args();
        args.title = Some("new title".into());
        let update = build_update(&args, &current, &exemplars).unwrap();
        assert_eq!(update.title, "new title");
        assert_eq!(update.rule, current.rule);
        assert_eq!(update.rationale, current.rationale);
        assert_eq!(update.blocking, current.blocking);
        assert_eq!(update.scopes, current.scopes);
        assert_eq!(update.exemplars.len(), exemplars.len());
    }

    #[test]
    fn clear_matcher_wipes_both_fields() {
        let (mut current, exemplars) = sample_learning();
        current.matcher = Some("$A".into());
        current.matcher_kind = Some(MatcherKind::AstGrep);
        let mut args = bare_args();
        args.clear_matcher = true;
        let update = build_update(&args, &current, &exemplars).unwrap();
        assert!(update.matcher.is_none());
        assert!(update.matcher_kind.is_none());
    }

    #[test]
    fn example_texts_replace_the_full_set() {
        let (current, exemplars) = sample_learning();
        let mut args = bare_args();
        args.example_texts = vec!["good:let x = 1;".into()];
        let update = build_update(&args, &current, &exemplars).unwrap();
        assert_eq!(update.exemplars.len(), 1);
        assert_eq!(update.exemplars[0].kind, ExemplarKind::Good);
        assert_eq!(update.exemplars[0].snippet, "let x = 1;");
    }
}
