//! Selection, ranking, the budget, the prompt, and findings. Spec
//! sections 7.1, 7.5 and 11.
//!
//! Nothing here reads a file, runs a program, or prints. The diff arrives
//! as a string and the prompt leaves as a string. Invariant 1.

use serde::{Deserialize, Serialize};

use crate::diff::Diff;
use crate::error::{Error, Result};
use crate::model::{Exemplar, ExemplarKind, Learning, Scope, ScopeKind, Sides};
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
    /// [`diff_digest`] of the diff text this scope was built from.
    pub diff_digest: String,
}

/// Hash the diff text a gate is about to audit. Spec section 9.2, **The
/// gate does not re-nag a diff it already covered**.
///
/// Over the **diff text alone**, never the prompt. The prompt carries
/// the rendered rules, so a `max_chars` change or an edit to an
/// unrelated rule would alter it and re-nag a diff nobody touched.
///
/// The output has to stay stable across builds, because it is compared
/// against rows written by an older binary. That rules out
/// `std::hash::DefaultHasher`, whose algorithm std explicitly reserves
/// the right to change between releases.
pub fn diff_digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// The paths in this diff that one learning's scopes selected.
///
/// Spec section 9.2, **The digest has to match the granularity of
/// selection**. Kinds AND across each other and rows OR within one kind,
/// exactly as selection itself does, so this narrows by intersection:
///
/// - `global` selects every path. Not a special case to write around — a
///   global rule does care about every byte, and keying its coverage on
///   the whole diff is the behaviour it should have.
/// - `project:` is repository-level and narrows no path.
/// - `language:` and `glob:` each narrow to the paths they match.
///
/// This recomputes what `writ_glob_any` already decided in SQL, because
/// that function answers *whether* a pattern matched and not *which paths*
/// did. It only ever runs over the selected set, which the budget caps at
/// `max_rules`, so it is not the linear cost invariant 5 is about.
pub fn matched_paths(learning: &Learning, diff: &Diff) -> Vec<String> {
    if learning
        .scopes
        .iter()
        .any(|one| one.kind == ScopeKind::Global)
    {
        return diff.paths.clone();
    }

    let languages = diff.languages();
    let mut paths = diff.paths.clone();
    for kind in [ScopeKind::Language, ScopeKind::Glob] {
        let rows: Vec<&Scope> = learning
            .scopes
            .iter()
            .filter(|one| one.kind == kind)
            .collect();
        if rows.is_empty() {
            continue;
        }
        paths.retain(|path| {
            rows.iter().any(|one| match kind {
                ScopeKind::Language => {
                    crate::diff::language_of(path) == Some(one.value.as_str())
                        && languages.contains(&one.value)
                }
                ScopeKind::Glob => crate::glob::glob_match(&one.value, path),
                ScopeKind::Global | ScopeKind::Project => true,
            })
        });
    }
    paths
}

/// The coverage key for one learning against one diff.
///
/// A hash of [`Diff::slice`] over [`matched_paths`], so an edit in a path
/// the learning does not scope leaves it untouched and an edit in a path it
/// does scope moves it. That is the whole correction in section 9.2: the
/// key has to be as narrow as the match that produced it.
/// The coverage keys for a whole selection, in selection order.
///
/// The gate needs these twice — once to ask whether every learning is
/// already covered, and once to record what this audit asked about — and
/// each one materializes a slice of the diff and hashes it. Computing them
/// once and carrying them is what keeps a blocked turn from building and
/// hashing the same bytes twice.
pub fn slice_digests(diff: &Diff, selected: &[Selected]) -> Vec<String> {
    selected
        .iter()
        .map(|one| learning_slice_digest(&one.learning, diff))
        .collect()
}

pub fn learning_slice_digest(learning: &Learning, diff: &Diff) -> String {
    diff_digest(&diff.slice(&matched_paths(learning, diff)))
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
    if selected.learning.sides != Sides::Both {
        block.push_str(&format!("sides: {}\n", selected.learning.sides));
    }
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
    // A fence with no language is a fence the reader's tooling cannot
    // highlight and the model has to infer from the code. The exemplar
    // names its own language when it was read from a file; otherwise the
    // learning's `language:` scope is the best answer available, and a
    // rule scoped to one language cannot have an exemplar in another.
    let scoped_language = selected.learning.scope_language();
    for exemplar in &selected.exemplars {
        let label = match exemplar.kind {
            ExemplarKind::Good => "good",
            ExemplarKind::Bad => "bad",
        };
        let language = exemplar
            .language
            .as_deref()
            .or(scoped_language)
            .unwrap_or_default();
        block.push_str(&format!("{label}:\n```{language}\n"));
        block.push_str(exemplar.snippet.trim_end_matches('\n'));
        block.push_str("\n```\n");
    }
    block
}

/// The prompt `--format prompt` prints. Spec section 7.1 step 4.
///
/// It carries the diff, then the selected rules, then what to do about
/// each one and how to send that decision back.
///
/// **It asks the agent to act.** A learning exists to change what the
/// agent writes, so the prompt asks for the change and fixes only the
/// order: report the decision first, because the hook process exits when
/// it has printed, then carry it out.
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
    // The agent is asked for a decision, not an observation. It reports
    // before it acts, so `fixed` is a commitment to correct the finding in
    // this turn and never a claim that the tree is already clean. Each
    // value has to say when it applies: naming the three without that, and
    // showing `open` in the example, yields `open` on every finding, and
    // `open` weighs nothing in `Outcomes::acceptance`.
    out.push_str(
        "Decide what you will do about each learning the diff breaks, then \
         send that decision to writ before you act on it. A reply left in \
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
         \"outcome\":\"fixed\"}]}\n",
    );
    out.push_str(
        "\nUse the audit-id above, verbatim. `outcome` is what you have \
         decided to do, not a guess at what the code already is:\n\n",
    );
    out.push_str(
        "- `fixed` \u{2014} you are correcting this before you hand back. \
         Report first, then make the change.\n",
    );
    out.push_str(
        "- `ignored` \u{2014} you are leaving it as it stands. Say why in \
         `detail`. A blocking learning still refuses the handoff.\n",
    );
    out.push_str(
        "- `open` \u{2014} you cannot settle it without the developer. It \
         blocks a blocking learning, so use it when you mean to ask, not as \
         a default.\n",
    );
    out.push_str(
        "\nPick one for every finding. An empty findings array is the right \
         answer when the diff breaks no rule, and it still has to be sent.\n",
    );
    out
}

/// What a gate emits in the host's protocol. Spec section 9.2, **The gate
/// points, it does not paste**.
///
/// The prompt carries the whole diff, and a host renders a Stop hook's
/// block verbatim into the transcript. On a long-lived branch that is
/// 100 KB of diff in front of the developer after every turn, for a
/// document written for the agent. So the gate emits this instead: the
/// audit id, and the two ways to fetch the document behind it.
///
/// Both paths are named for the same reason [`render_prompt`] names both:
/// writ cannot see whether the host has MCP, and a pointer whose only path
/// is a tool the host does not serve is a gate that blocks forever.
///
/// The count is here and the rules are not. It is the one number that
/// tells the agent the fetch is worth making, and it is two characters
/// whether the collection holds fifty learnings or a thousand, so P3 still
/// holds.
pub fn render_pointer(audit_id: &str, sent: usize) -> String {
    let (count, subject, verb) = if sent == 1 {
        ("1 learning", "it", "applies")
    } else {
        // A gate never emits a pointer for nothing: `gates()` is false
        // when the selection is empty, so this branch is the plural one.
        ("learnings", "them", "apply")
    };
    let count = if sent == 1 {
        count.to_string()
    } else {
        format!("{sent} {count}")
    };
    let mut out = String::from("# writ audit\n\n");
    out.push_str(&format!(
        "{count} {verb} to this diff. Fetch {subject}, check the diff against \
         {subject}, and report every violation you find.\n\n"
    ));
    out.push_str(&format!("audit-id: {audit_id}\n"));
    out.push_str("\n## Fetch\n\n");
    out.push_str(
        "- Call the `writ_audit` tool with a `fetch` argument set to the \
         audit-id above, when MCP is available. This is the normal path.\n",
    );
    out.push_str(&format!(
        "- Otherwise run `writ audit --fetch {audit_id}`.\n"
    ));
    out.push_str(
        "\nWhat comes back carries the diff, the learnings, and how to report \
         the findings. The audit is not finished until those findings reach \
         writ.\n",
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
    /// The ids of the findings this ingest left `open`, in the order they
    /// were reported.
    ///
    /// `--resolve` settles a finding by its id, and until this existed no
    /// caller was ever told one: ids appear only on the web UI detail
    /// page, which P5 reserves for a human and an audit may not open. So
    /// the one surface that can answer the question an `open` finding
    /// asks could not name the finding it was answering.
    pub open: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Scope, SourceKind, Status};

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
            sides: Sides::Both,
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
            scopes: vec![Scope::global()],
        }
    }

    /// The prompt has to say *when* each outcome applies, not merely name
    /// the three values, and the example must carry a decision rather than
    /// a default. An agent copies the template it is shown, and `open`
    /// weighs nothing in `Outcomes::acceptance`.
    #[test]
    fn the_prompt_asks_for_a_decision_on_every_finding() {
        let scope = AuditScope {
            identity: crate::repo::RepoIdentity::Remote("github.com/o/r".into()),
            diff: crate::diff::Diff::parse("diff --git a/a.rs b/a.rs\n+let x = 1;\n"),
            diff_range: "HEAD".into(),
            diff_digest: "d".into(),
        };
        let prompt = render_prompt("AUDIT", &scope, &[]);

        // The example must not hand back a default.
        assert!(
            prompt.contains(r#""outcome":"fixed""#),
            "the template should carry a decision, not `open`: {prompt}"
        );
        assert!(!prompt.contains(r#""outcome":"open""#), "{prompt}");

        // Each value has to say what it commits the agent to.
        for clause in [
            "`fixed` \u{2014} you are correcting this",
            "`ignored` \u{2014} you are leaving it",
            "`open` \u{2014} you cannot settle it",
        ] {
            assert!(prompt.contains(clause), "missing {clause}: {prompt}");
        }

        // `fixed` is a commitment made before acting, not an observation.
        assert!(
            prompt.contains("Report first, then make the change."),
            "{prompt}"
        );
        assert!(prompt.contains("Pick one for every finding."), "{prompt}");
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
    fn rule_block_names_sides_only_when_not_both() {
        let both = Selected {
            learning: learning("a", true),
            exemplars: vec![],
        };
        let both_block = rule_block(1, &both);
        assert!(
            !both_block.contains("sides:"),
            "both costs no prompt budget: {both_block}"
        );

        let mut added = learning("b", true);
        added.sides = Sides::Added;
        let added_block = rule_block(
            1,
            &Selected {
                learning: added,
                exemplars: vec![],
            },
        );
        assert!(
            added_block.contains("sides: added\n"),
            "narrow sides must reach the reviewer: {added_block}"
        );
    }

    /// An exemplar's fence carries a language, so the snippet reaches the
    /// reviewer as code rather than as text.
    ///
    /// The column had a reader here and no writer anywhere, so every
    /// fence rendered bare. The learning's own `language:` scope answers
    /// it whenever the exemplar does not.
    #[test]
    fn an_exemplar_fence_is_labelled_from_the_exemplar_or_the_scope() {
        let exemplar = |language: Option<&str>| Exemplar {
            id: "e".into(),
            kind: ExemplarKind::Bad,
            language: language.map(str::to_string),
            snippet: "x = 1".into(),
            note: None,
        };
        let language = |value: &str| Scope {
            kind: ScopeKind::Language,
            value: value.to_string(),
        };

        // What the exemplar knows wins.
        let mut scoped = learning("a", true);
        scoped.scopes = vec![language("elixir")];
        let block = rule_block(
            1,
            &Selected {
                learning: scoped.clone(),
                exemplars: vec![exemplar(Some("rust"))],
            },
        );
        assert!(block.contains("```rust\n"), "{block}");

        // Otherwise the single `language:` scope answers.
        let block = rule_block(
            1,
            &Selected {
                learning: scoped,
                exemplars: vec![exemplar(None)],
            },
        );
        assert!(block.contains("```elixir\n"), "{block}");

        // Two languages name no one language, so the fence stays bare
        // rather than claiming the wrong one.
        let mut ambiguous = learning("a", true);
        ambiguous.scopes = vec![language("elixir"), language("rust")];
        let block = rule_block(
            1,
            &Selected {
                learning: ambiguous,
                exemplars: vec![exemplar(None)],
            },
        );
        assert!(block.contains("```\n"), "{block}");
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

#[cfg(test)]
mod slice_digest_tests {
    use super::*;
    use crate::diff::Diff;
    use crate::model::{Scope, ScopeKind, SourceKind, Status};

    const DIFF: &str = "diff --git a/.github/workflows/deploy.yml b/.github/workflows/deploy.yml\n\
        --- a/.github/workflows/deploy.yml\n\
        +++ b/.github/workflows/deploy.yml\n\
        @@ -1 +1 @@\n\
        -uses: actions/checkout@v4\n\
        +uses: actions/checkout@v7\n\
        diff --git a/src/app.ts b/src/app.ts\n\
        --- a/src/app.ts\n\
        +++ b/src/app.ts\n\
        @@ -1 +1 @@\n\
        -const a = 1;\n\
        +const a = 2;\n";

    fn scoped(scopes: Vec<Scope>) -> Learning {
        Learning {
            id: "01".into(),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
            status: Status::Active,
            title: "t".into(),
            rule: "r".into(),
            rationale: "why".into(),
            blocking: true,
            sides: Sides::Both,
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
            scopes,
        }
    }

    fn glob(pattern: &str) -> Scope {
        Scope {
            kind: ScopeKind::Glob,
            value: pattern.to_string(),
        }
    }

    fn language(name: &str) -> Scope {
        Scope {
            kind: ScopeKind::Language,
            value: name.to_string(),
        }
    }

    /// A `glob:` scope narrows the slice to the paths it matched.
    #[test]
    fn a_glob_scope_selects_only_its_paths() {
        let diff = Diff::parse(DIFF);
        let one = scoped(vec![glob(".github/workflows/**")]);

        assert_eq!(matched_paths(&one, &diff), [".github/workflows/deploy.yml"]);
    }

    /// A `global` scope cares about every byte, so its slice is the whole
    /// diff. Not a special case — the deliberate behaviour.
    #[test]
    fn a_global_scope_selects_every_path() {
        let diff = Diff::parse(DIFF);
        let one = scoped(vec![Scope::global()]);

        assert_eq!(matched_paths(&one, &diff), diff.paths);
    }

    /// `project:` is repo-level, so it narrows nothing about paths.
    #[test]
    fn a_project_scope_narrows_no_path() {
        let diff = Diff::parse(DIFF);
        let one = scoped(vec![Scope {
            kind: ScopeKind::Project,
            value: "github.com/o/r".into(),
        }]);

        assert_eq!(matched_paths(&one, &diff), diff.paths);
    }

    /// Kinds AND across each other, so two kinds intersect.
    #[test]
    fn kinds_intersect_across_each_other() {
        let diff = Diff::parse(DIFF);
        let one = scoped(vec![language("typescript"), glob("src/**")]);

        assert_eq!(matched_paths(&one, &diff), ["src/app.ts"]);

        // The same language against a glob that excludes it leaves nothing.
        let neither = scoped(vec![language("typescript"), glob(".github/**")]);
        assert!(matched_paths(&neither, &diff).is_empty());
    }

    /// Rows inside one kind are alternatives, so two globs union.
    #[test]
    fn rows_within_one_kind_union() {
        let diff = Diff::parse(DIFF);
        let one = scoped(vec![glob(".github/**"), glob("src/**")]);

        assert_eq!(matched_paths(&one, &diff), diff.paths);
    }

    /// The whole point. Spec 9.2, *The digest has to match the
    /// granularity of selection*: eleven distinct whole-diff digests
    /// re-served one settled rule ten times. The slice digest holds still.
    #[test]
    fn an_unscoped_edit_leaves_the_slice_digest_alone() {
        let one = scoped(vec![glob(".github/workflows/**")]);
        let before = Diff::parse(DIFF);
        let after = Diff::parse(&DIFF.replace("const a = 2;", "const a = 3;"));

        assert_ne!(
            diff_digest(&before.text),
            diff_digest(&after.text),
            "the whole-diff digest does move, which is the defect"
        );
        assert_eq!(
            learning_slice_digest(&one, &before),
            learning_slice_digest(&one, &after),
            "the slice digest must not"
        );
    }

    /// Touch the scoped file and the gate has to come back.
    #[test]
    fn a_scoped_edit_moves_the_slice_digest() {
        let one = scoped(vec![glob(".github/workflows/**")]);
        let before = Diff::parse(DIFF);
        let after = Diff::parse(&DIFF.replace("checkout@v7", "checkout@v6"));

        assert_ne!(
            learning_slice_digest(&one, &before),
            learning_slice_digest(&one, &after)
        );
    }

    /// A global rule keeps exactly today's behaviour: its slice digest
    /// moves with any byte, because it scopes every byte.
    #[test]
    fn a_global_rule_still_moves_with_any_byte() {
        let one = scoped(vec![Scope::global()]);
        let before = Diff::parse(DIFF);
        let after = Diff::parse(&DIFF.replace("const a = 2;", "const a = 3;"));

        assert_ne!(
            learning_slice_digest(&one, &before),
            learning_slice_digest(&one, &after)
        );
    }

    /// The digest is a hash, not the slice. It has to be stable across
    /// builds, because it is compared against rows an older binary wrote.
    #[test]
    fn the_slice_digest_is_the_hash_of_the_slice() {
        let one = scoped(vec![glob(".github/workflows/**")]);
        let diff = Diff::parse(DIFF);

        assert_eq!(
            learning_slice_digest(&one, &diff),
            diff_digest(&diff.slice(&[".github/workflows/deploy.yml".to_string()]))
        );
    }
}
