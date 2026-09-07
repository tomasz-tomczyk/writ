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
    store.update_learning(&args.id, &update)?;

    if args.activate && current.status == Status::Proposed {
        store.set_status(&args.id, Status::Active)?;
    }

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

    fn exemplars(&self) -> Result<Vec<NewExemplar>> {
        self.example_texts
            .iter()
            .map(|text| {
                let (kind, snippet) = record::split_kind(text, "--example-text", "TEXT")?;
                if snippet.is_empty() {
                    return Err(Error::Validation {
                        message: "an --example-text snippet is empty".to_string(),
                    });
                }
                Ok(record::exemplar(kind, snippet.to_string()))
            })
            .collect()
    }
}

fn build_update(
    args: &Args,
    current: &Learning,
    current_exemplars: &[Exemplar],
) -> Result<LearningUpdate> {
    let matcher = if args.clear_matcher {
        None
    } else if args.matcher.is_some() || args.matcher_kind.is_some() {
        args.matcher.clone()
    } else {
        current.matcher.clone()
    };

    let matcher_kind = if args.clear_matcher {
        None
    } else if args.matcher_kind.is_some() {
        args.matcher_kind
            .as_deref()
            .map(str::parse::<MatcherKind>)
            .transpose()?
    } else if args.matcher.is_some() {
        // --matcher without --matcher-kind: keep current kind if any, but
        // validation will catch the mismatch if there is no current kind.
        current.matcher_kind
    } else {
        current.matcher_kind
    };

    let exemplars = if args.example_texts.is_empty() {
        current_exemplars
            .iter()
            .map(|e| NewExemplar {
                kind: e.kind,
                language: e.language.clone(),
                snippet: e.snippet.clone(),
                note: e.note.clone(),
            })
            .collect()
    } else {
        args.exemplars()?
    };

    Ok(LearningUpdate {
        title: args
            .title
            .as_deref()
            .map(str::trim)
            .map(String::from)
            .unwrap_or_else(|| current.title.clone()),
        rule: args
            .rule
            .as_deref()
            .map(str::trim)
            .map(String::from)
            .unwrap_or_else(|| current.rule.clone()),
        rationale: args
            .rationale
            .as_deref()
            .map(str::trim)
            .map(String::from)
            .unwrap_or_else(|| current.rationale.clone()),
        blocking: if args.advisory {
            false
        } else if args.blocking {
            true
        } else {
            current.blocking
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

    #[test]
    fn omitted_fields_keep_current_values() {
        let (current, exemplars) = sample_learning();
        let args = Args {
            id: "id".into(),
            title: Some("new title".into()),
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
        };
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
        let args = Args {
            id: "id".into(),
            title: None,
            rule: None,
            rationale: None,
            scopes: vec![],
            advisory: false,
            blocking: false,
            matcher: None,
            matcher_kind: None,
            clear_matcher: true,
            example_texts: vec![],
            activate: false,
            format: Format::Text,
        };
        let update = build_update(&args, &current, &exemplars).unwrap();
        assert!(update.matcher.is_none());
        assert!(update.matcher_kind.is_none());
    }

    #[test]
    fn example_texts_replace_the_full_set() {
        let (current, exemplars) = sample_learning();
        let args = Args {
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
            example_texts: vec!["good:let x = 1;".into()],
            activate: false,
            format: Format::Text,
        };
        let update = build_update(&args, &current, &exemplars).unwrap();
        assert_eq!(update.exemplars.len(), 1);
        assert_eq!(update.exemplars[0].kind, ExemplarKind::Good);
        assert_eq!(update.exemplars[0].snippet, "let x = 1;");
    }
}
