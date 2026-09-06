//! Selection, ranking, the budget, the prompt, and findings. Spec
//! sections 7.1, 7.5 and 11.
//!
//! Nothing here reads a file, runs a program, or prints. The diff arrives
//! as a string and the prompt leaves as a string. Invariant 1.

use serde::{Deserialize, Serialize};

use crate::diff::Diff;
use crate::error::{Error, Result};
use crate::model::{Exemplar, ExemplarKind, Learning};
use crate::repo::RepoIdentity;

/// What one audit is looking at.
#[derive(Debug, Clone)]
pub struct AuditScope {
    /// The repository, by its normalized remote. Invariant 4.
    pub identity: RepoIdentity,
    /// The diff under review.
    pub diff: Diff,
    /// The range as the caller named it, for the `audits` row.
    pub diff_range: String,
}

/// How many characters and rules one prompt may carry. Spec section 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The most learnings one prompt may carry.
    pub max_rules: u32,
    /// The most characters of rule text one prompt may carry.
    pub max_chars: u32,
}

/// What the findings of past audits say about one learning. Spec 7.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Outcomes {
    /// The agent said it fixed the violation.
    pub fixed: i64,
    /// The agent said it ignored the violation.
    pub ignored: i64,
    /// The developer rejected the finding.
    pub rejected: i64,
}

/// The developer's word. Only this outcome comes from a person.
const REJECTED_WEIGHT: f64 = 1.0;
/// The agent's word, and a mild one.
const IGNORED_WEIGHT: f64 = 0.25;
/// The agent grading its own compliance. Kept above zero so the column is
/// visible, kept near zero because the signal is not worth more.
const FIXED_WEIGHT: f64 = 0.01;

impl Outcomes {
    /// A rank key between 0 and 1, where 1 is a rule nothing argues with.
    ///
    /// Spec section 7.5 gives only negative signals: `rejected` counts
    /// heavily because a developer set it, `ignored` a little, and `fixed`
    /// almost not at all because an agent reporting `fixed` is grading its
    /// own obedience. A rule with no settled findings scores 1: it is
    /// unproven, not disliked.
    pub fn acceptance(&self) -> f64 {
        let total = (self.fixed + self.ignored + self.rejected) as f64;
        if total == 0.0 {
            return 1.0;
        }
        let penalty = REJECTED_WEIGHT * self.rejected as f64
            + IGNORED_WEIGHT * self.ignored as f64
            + FIXED_WEIGHT * self.fixed as f64;
        (1.0 - penalty / total).clamp(0.0, 1.0)
    }
}

/// One learning that survived scope and matcher selection.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The learning itself.
    pub learning: Learning,
    /// What past findings say about it.
    pub outcomes: Outcomes,
}

/// One learning the prompt carries, with the snippets that teach it.
#[derive(Debug, Clone)]
pub struct Selected {
    /// The learning.
    pub learning: Learning,
    /// Its exemplars, oldest first.
    pub exemplars: Vec<Exemplar>,
}

/// Order candidates the way the budget spends on them. Spec 7.1 step 3.
///
/// Blocking first, then acceptance, then recency. The id is UUIDv7, so
/// descending id is newest first and the order is total: two learnings
/// never tie, which keeps the prompt reproducible.
pub fn rank(candidates: &mut [Candidate]) {
    candidates.sort_by(|left, right| {
        right
            .learning
            .blocking
            .cmp(&left.learning.blocking)
            .then_with(|| {
                right
                    .outcomes
                    .acceptance()
                    .total_cmp(&left.outcomes.acceptance())
            })
            .then_with(|| right.learning.id.cmp(&left.learning.id))
    });
}

/// Render one learning as the prompt carries it.
///
/// The budget measures this string, so the cap counts the bytes that are
/// actually sent rather than an estimate of them.
pub fn rule_block(position: usize, selected: &Selected) -> String {
    let mut block = format!("### {}. {}\n", position, selected.learning.title);
    block.push_str(&format!("id: {}\n", selected.learning.id));
    block.push_str(&format!(
        "enforcement: {}\n",
        if selected.learning.blocking {
            "blocking"
        } else {
            "advisory"
        }
    ));
    let scopes = selected
        .learning
        .scopes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    block.push_str(&format!("scopes: {scopes}\n"));
    block.push_str(&format!("rule: {}\n", selected.learning.rule));
    block.push_str(&format!("why: {}\n", selected.learning.rationale));
    for exemplar in &selected.exemplars {
        let label = match exemplar.kind {
            ExemplarKind::Good => "good",
            ExemplarKind::Bad => "bad",
        };
        let language = exemplar.language.clone().unwrap_or_default();
        block.push_str(&format!("{label}:\n```{language}\n"));
        block.push_str(exemplar.snippet.trim_end_matches('\n'));
        block.push_str("\n```\n");
    }
    block
}

/// The prompt `--format prompt` prints. Spec section 7.1 step 4.
///
/// It carries the diff, then the selected rules, then how to send the
/// findings back. It never asks the host to rewrite the tree: writ
/// reports, the developer decides.
///
/// **It names a return path, not only a JSON shape.** An earlier version
/// ended with "reply with this JSON", and a reply in the conversation
/// reaches nothing: the hook process has already exited. Everything
/// downstream of a finding died there. `times_applied` never moved, so
/// `--never-applied` and the Health bucket measured nothing. Step 3 ranks
/// by acceptance rate over `findings`, and there were none. `blocking` had
/// no effect at all, because its only teeth are the ingest gate and ingest
/// never ran. The loop looked closed and was not.
///
/// Both paths are named because writ cannot see which the host has.
///
/// How many learnings were considered is deliberately **not** in here. It
/// is audit bookkeeping, the host has no use for it, and printing it would
/// make the prompt grow by two characters between a collection of fifty
/// and one of a thousand. P3 says the two are the same prompt, so the
/// counts live in the `audits` row and in `--format json` instead.
pub fn render_prompt(audit_id: &str, scope: &AuditScope, selected: &[Selected]) -> String {
    let mut out = String::from("# writ audit\n\n");
    out.push_str(
        "Check the diff below against the learnings below it. Report every \
         violation you find.\n\n",
    );
    out.push_str(&format!("audit-id: {audit_id}\n"));
    out.push_str(&format!("repo: {}\n", scope.identity.value()));
    if scope.identity.is_fallback() {
        out.push_str(
            "repo-identity: path fallback. This repository has no git remote, \
             so a project: scope recorded elsewhere does not match it.\n",
        );
    }
    out.push_str(&format!("diff-range: {}\n", scope.diff_range));

    out.push_str("\n## Diff\n\n```diff\n");
    out.push_str(scope.diff.text.trim_end_matches('\n'));
    out.push_str("\n```\n");

    out.push_str("\n## Learnings\n");
    if selected.is_empty() {
        out.push_str("\nNo learning applies to this diff.\n");
    } else {
        for (index, one) in selected.iter().enumerate() {
            out.push('\n');
            out.push_str(&rule_block(index + 1, one));
        }
    }

    out.push_str("\n## Report back\n\n");
    out.push_str(
        "Do not rewrite the tree. Send the findings to writ. A reply left in \
         the conversation reaches nothing, because the audit has already \
         exited.\n\n",
    );
    out.push_str(
        "- Call the `writ_audit` tool with a `findings` argument holding the \
         whole document below, when MCP is available. This is the normal \
         path.\n",
    );
    out.push_str("- Otherwise pipe the same document to `writ audit --ingest`.\n\n");
    out.push_str(
        "{\"audit_id\":\"AUDIT\",\"findings\":[{\"learning_id\":\"ID\",\
         \"path\":\"PATH\",\"line\":1,\"detail\":\"WHAT IS WRONG\",\
         \"outcome\":\"open\"}]}\n",
    );
    out.push_str(
        "\nUse the audit-id above, verbatim. `outcome` is `open`, `fixed` or \
         `ignored`.\n",
    );
    out.push_str(
        "An empty findings array is the right answer when the diff breaks no \
         rule, and it still has to be sent.\n",
    );
    out
}

/// The outcome a finding carries. Spec section 7.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Reported, and nothing has happened to it yet.
    #[default]
    Open,
    /// The agent says it fixed the violation.
    Fixed,
    /// The agent says it left the violation alone.
    Ignored,
    /// The developer says the finding was wrong.
    Rejected,
}

impl Outcome {
    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Fixed => "fixed",
            Self::Ignored => "ignored",
            Self::Rejected => "rejected",
        }
    }
}

/// One finding as the host reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncomingFinding {
    /// Which learning the diff broke.
    pub learning_id: String,
    /// Where, for this audit only. An exemplar never carries this.
    #[serde(default)]
    pub path: Option<String>,
    /// Which line, for this audit only.
    #[serde(default)]
    pub line: Option<i64>,
    /// What is wrong, in the host's words.
    #[serde(default)]
    pub detail: Option<String>,
    /// What happened to it. Absent means `open`.
    #[serde(default)]
    pub outcome: Outcome,
}

/// The whole `--ingest` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingsInput {
    /// The audit these findings answer, as the prompt named it.
    pub audit_id: String,
    /// The findings themselves. An empty list is a valid answer.
    pub findings: Vec<IncomingFinding>,
}

/// Read the findings JSON the host sends back.
///
/// Malformed JSON is exit `4` and an unknown field is exit `2`, so the two
/// are separated here rather than merged into one complaint. Section 5.7.
pub fn parse_findings(text: &str) -> Result<FindingsInput> {
    if text.trim().is_empty() {
        return Err(Error::validation(
            "--ingest read nothing on stdin. Pipe the findings JSON in",
        ));
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| Error::BadJson {
        line: error.line().max(1),
        message: error.to_string(),
    })?;
    serde_json::from_value(value).map_err(|error| {
        Error::validation(format!(
            "the findings JSON is not what writ asked for: {error}"
        ))
    })
}

/// What one ingest wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ingested {
    /// The audit the findings were written against.
    pub audit_id: String,
    /// How many findings landed.
    pub findings: usize,
    /// How many of them belong to a blocking learning.
    pub blocking: usize,
    /// How many blocking findings are not `fixed`. Section 7.1 step 6
    /// exits `1` when this is not zero.
    ///
    /// A blocking finding the agent fixed lets the handoff through. Any
    /// other blocking outcome stops it, so an `ignored` violation cannot
    /// be waved past. `rejected` never reaches here: `--ingest` refuses
    /// it, because the developer owns that word.
    pub unfixed_blocking: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SourceKind, Status};

    fn learning(id: &str, blocking: bool) -> Learning {
        Learning {
            id: id.to_string(),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
            status: Status::Active,
            title: "t".into(),
            rule: "r".into(),
            rationale: "why".into(),
            blocking,
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
            scopes: vec![crate::model::Scope::global()],
        }
    }

    fn candidate(id: &str, blocking: bool, outcomes: Outcomes) -> Candidate {
        Candidate {
            learning: learning(id, blocking),
            outcomes,
        }
    }

    #[test]
    fn a_rule_nobody_has_judged_scores_full_marks() {
        assert_eq!(Outcomes::default().acceptance(), 1.0);
    }

    /// The developer's rejection is the only heavy signal, so the same
    /// count of each outcome must not cost the same.
    #[test]
    fn a_rejection_costs_far_more_than_an_agent_reported_fix() {
        let rejected = Outcomes {
            rejected: 4,
            ..Default::default()
        };
        let ignored = Outcomes {
            ignored: 4,
            ..Default::default()
        };
        let fixed = Outcomes {
            fixed: 4,
            ..Default::default()
        };
        assert!(rejected.acceptance() < ignored.acceptance());
        assert!(ignored.acceptance() < fixed.acceptance());
        assert!(fixed.acceptance() > 0.9, "a fix is almost no signal");
        assert_eq!(rejected.acceptance(), 0.0);
    }

    #[test]
    fn blocking_outranks_a_better_accepted_advisory_rule() {
        let mut candidates = vec![
            candidate("a", false, Outcomes::default()),
            candidate(
                "b",
                true,
                Outcomes {
                    rejected: 1,
                    ..Default::default()
                },
            ),
        ];
        rank(&mut candidates);
        assert_eq!(candidates[0].learning.id, "b");
    }

    #[test]
    fn acceptance_breaks_a_tie_before_recency_does() {
        let mut candidates = vec![
            candidate(
                "a",
                true,
                Outcomes {
                    rejected: 1,
                    ..Default::default()
                },
            ),
            candidate("b", true, Outcomes::default()),
            candidate("c", true, Outcomes::default()),
        ];
        rank(&mut candidates);
        // b and c are equally accepted, so the newer id wins between them,
        // and the rejected one falls to the back whatever its id.
        assert_eq!(
            candidates
                .iter()
                .map(|c| c.learning.id.as_str())
                .collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
    }

    #[test]
    fn findings_json_round_trips() {
        let input = parse_findings(
            r#"{"audit_id":"A","findings":[{"learning_id":"L","path":"a.rs","line":3}]}"#,
        )
        .unwrap();
        assert_eq!(input.audit_id, "A");
        assert_eq!(input.findings[0].outcome, Outcome::Open);
        assert_eq!(input.findings[0].line, Some(3));
    }

    #[test]
    fn no_findings_is_a_valid_answer() {
        let input = parse_findings(r#"{"audit_id":"A","findings":[]}"#).unwrap();
        assert!(input.findings.is_empty());
    }

    /// Section 5.7 gives malformed JSON its own code, so it must not come
    /// back as a validation error.
    #[test]
    fn malformed_json_is_bad_json_not_a_validation_error() {
        let error = parse_findings("{oops").unwrap_err();
        assert!(matches!(error, Error::BadJson { .. }), "{error}");
    }

    #[test]
    fn well_formed_json_of_the_wrong_shape_is_a_validation_error() {
        let error = parse_findings(r#"{"findings":[]}"#).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
    }
}
