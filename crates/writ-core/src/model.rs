//! The shapes a write and a read carry.
//!
//! Every type here parses and validates itself. Parsing is pure, so it
//! lives in `writ-core` and is tested without a database or a terminal.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::audit::Outcome;
use crate::error::{Error, Result};

/// Where a learning stands. `archived` is reached by `writ archive`, never
/// by a write, so [`Status::parse_writable`] refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Written but not approved. The default. See invariant 2.
    Proposed,
    /// Approved, and therefore selectable by an audit.
    Active,
    /// Pruned. Kept as evidence, never selected. See P4.
    Archived,
}

impl Status {
    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    /// Parse a status a write may ask for.
    ///
    /// `archived` is a valid status but not a valid write: pruning is
    /// `writ archive`, which keeps the audit trail the CLI reference
    /// describes.
    pub fn parse_writable(text: &str) -> Result<Self> {
        match text {
            "proposed" => Ok(Self::Proposed),
            "active" => Ok(Self::Active),
            "archived" => Err(Error::validation(
                "a write cannot set status archived. Use `writ archive ID`",
            )),
            other => Err(Error::validation(format!(
                "unknown status {other}. Use proposed or active"
            ))),
        }
    }
}

impl FromStr for Status {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        match text {
            "proposed" => Ok(Self::Proposed),
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            other => Err(Error::validation(format!(
                "unknown status {other}. Use proposed, active or archived"
            ))),
        }
    }
}

/// How a learning arrived. Spec section 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// A person typed it.
    Manual,
    /// A session adapter proposed it.
    Session,
    /// It came in through `writ record --json`.
    Import,
}

impl SourceKind {
    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Session => "session",
            Self::Import => "import",
        }
    }
}

/// The dialect a matcher is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatcherKind {
    /// An ast-grep pattern.
    AstGrep,
    /// A regular expression.
    Regex,
}

impl MatcherKind {
    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AstGrep => "ast_grep",
            Self::Regex => "regex",
        }
    }
}

impl FromStr for MatcherKind {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        match text {
            "ast_grep" => Ok(Self::AstGrep),
            "regex" => Ok(Self::Regex),
            other => Err(Error::validation(format!(
                "unknown matcher kind {other}. Use ast_grep or regex"
            ))),
        }
    }
}

/// Which side of the pair an exemplar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExemplarKind {
    /// The preferred form.
    Good,
    /// The anti-pattern.
    Bad,
}

impl ExemplarKind {
    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Bad => "bad",
        }
    }
}

impl FromStr for ExemplarKind {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        match text {
            "good" => Ok(Self::Good),
            "bad" => Ok(Self::Bad),
            other => Err(Error::validation(format!(
                "unknown exemplar kind {other}. Use good or bad"
            ))),
        }
    }
}

/// One scope row. `global` carries no value, the other three require one.
///
/// The wire form is the string a user types, `language:rust`, so the CLI
/// flag, the JSONL field and `writ export` all speak one dialect.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    /// `global`, `project`, `language` or `glob`.
    pub kind: ScopeKind,
    /// Empty for `global`. Never NULL: spec section 6 explains why.
    pub value: String,
}

/// The four scope kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeKind {
    /// Every diff.
    Global,
    /// One repository, by its normalized remote.
    Project,
    /// One language.
    Language,
    /// One path pattern.
    Glob,
}

impl ScopeKind {
    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Language => "language",
            Self::Glob => "glob",
        }
    }
}

impl Scope {
    /// The scope every write falls back to when none is given.
    pub fn global() -> Self {
        Self {
            kind: ScopeKind::Global,
            value: String::new(),
        }
    }
}

impl FromStr for Scope {
    type Err = Error;

    /// Parse `KIND:VALUE`. A `glob:` pattern may hold a colon, so only the
    /// first one separates.
    fn from_str(text: &str) -> Result<Self> {
        let (kind, value) = match text.split_once(':') {
            Some((kind, value)) => (kind, value),
            None => (text, ""),
        };
        let kind = match kind {
            "global" => ScopeKind::Global,
            "project" => ScopeKind::Project,
            "language" => ScopeKind::Language,
            "glob" => ScopeKind::Glob,
            other => {
                return Err(Error::validation(format!(
                    "unknown scope kind {other}. Use global, project, language or glob"
                )));
            }
        };
        if kind == ScopeKind::Global {
            if !value.is_empty() {
                return Err(Error::validation(
                    "the global scope takes no value. Write `--scope global`",
                ));
            }
        } else if value.is_empty() {
            return Err(Error::validation(format!(
                "the {} scope needs a value. Write `--scope {}:VALUE`",
                kind.as_str(),
                kind.as_str()
            )));
        }
        Ok(Self {
            kind,
            value: value.to_string(),
        })
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.kind == ScopeKind::Global {
            f.write_str("global")
        } else {
            write!(f, "{}:{}", self.kind.as_str(), self.value)
        }
    }
}

impl Serialize for Scope {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// An exemplar as it arrives on a write.
///
/// It holds snippet text. It never holds a path or a line number: bookmarks
/// rot, and teaching material has to travel. See invariant 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewExemplar {
    /// `good` or `bad`.
    pub kind: ExemplarKind,
    /// The language the snippet is written in, when the caller knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The snippet itself.
    pub snippet: String,
    /// A note that travels with the snippet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The editable fields on a stored learning.
///
/// Status, provenance, counters and activation history are deliberately
/// absent: the Detail editor must not rewrite runtime state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearningUpdate {
    /// A short name.
    pub title: String,
    /// What to do.
    pub rule: String,
    /// Why.
    pub rationale: String,
    /// Whether breaking the rule stops the handoff.
    pub blocking: bool,
    /// The dialect of `matcher`.
    pub matcher_kind: Option<MatcherKind>,
    /// A retrieval matcher.
    pub matcher: Option<String>,
    /// The full replacement scope set. At least one scope is required.
    pub scopes: Vec<Scope>,
    /// The full replacement exemplar set.
    pub exemplars: Vec<NewExemplar>,
}

impl LearningUpdate {
    pub(crate) fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("title", &self.title),
            ("rule", &self.rule),
            ("rationale", &self.rationale),
        ] {
            if value.trim().is_empty() {
                return Err(Error::validation(format!("{field} is required and empty")));
            }
        }
        match (&self.matcher, &self.matcher_kind) {
            (Some(_), None) => {
                return Err(Error::validation(
                    "--matcher needs --matcher-kind: ast_grep or regex",
                ));
            }
            (None, Some(_)) => {
                return Err(Error::validation("--matcher-kind needs --matcher"));
            }
            _ => {}
        }
        if self.scopes.is_empty() {
            return Err(Error::validation("at least one scope is required"));
        }
        let mut scopes = self.scopes.clone();
        scopes.sort();
        let before = scopes.len();
        scopes.dedup();
        if scopes.len() != before {
            return Err(Error::validation("the same scope is given twice"));
        }
        reject_global_with_another_kind(&scopes)?;
        for exemplar in &self.exemplars {
            if exemplar.snippet.is_empty() {
                return Err(Error::validation("an exemplar snippet is empty"));
            }
        }
        Ok(())
    }
}

/// An exemplar as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Exemplar {
    /// UUIDv7.
    pub id: String,
    /// `good` or `bad`.
    pub kind: ExemplarKind,
    /// The language the snippet is written in.
    pub language: Option<String>,
    /// The snippet itself.
    pub snippet: String,
    /// A note that travels with the snippet.
    pub note: Option<String>,
}

/// One finding as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// UUIDv7.
    pub id: String,
    /// The audit that produced this finding.
    pub audit_id: String,
    /// The learning the diff broke.
    pub learning_id: String,
    /// Where, for this audit only.
    pub path: Option<String>,
    /// Which line, for this audit only.
    pub line: Option<i64>,
    /// What was wrong.
    pub detail: Option<String>,
    /// What happened to the finding.
    pub outcome: Outcome,
}

fn blocking_default() -> bool {
    true
}

/// Everything one `writ record` writes.
///
/// The three timestamp fields exist for import only. Invariant 7 forbids
/// setting `updated_at` by hand in an UPDATE. This is an INSERT, where the
/// trigger does not run, and an imported learning must keep the timestamps
/// it arrived with.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLearning {
    /// A short name.
    ///
    /// The three text fields carry `default` so that a JSON line which
    /// omits one is a validation error naming the field, not a parse
    /// error. The JSON is well formed. It is the content the rules
    /// refuse, and section 5.7 gives the two causes different codes.
    #[serde(default)]
    pub title: String,
    /// What to do.
    #[serde(default)]
    pub rule: String,
    /// Why. Required, and never empty: the reason is what lets a rule
    /// transfer to a situation that is similar but not identical.
    #[serde(default)]
    pub rationale: String,
    /// Where the rule applies. Defaults to `global` when empty.
    #[serde(default)]
    pub scopes: Vec<Scope>,
    /// Whether breaking the rule stops the handoff. Blocking is the default.
    #[serde(default = "blocking_default")]
    pub blocking: bool,
    /// The dialect of `matcher`.
    #[serde(default)]
    pub matcher_kind: Option<MatcherKind>,
    /// A retrieval matcher. Optional.
    #[serde(default)]
    pub matcher: Option<String>,
    /// The good-and-bad pair.
    #[serde(default)]
    pub exemplars: Vec<NewExemplar>,
    /// `proposed` or `active`. `None` means `proposed`.
    #[serde(default)]
    pub status: Option<Status>,
    /// Who wrote it. `None` outside a git repository with no config key.
    #[serde(default)]
    pub author: Option<String>,
    /// How it arrived.
    #[serde(default)]
    pub source_kind: Option<SourceKind>,
    /// Which adapter produced it.
    #[serde(default)]
    pub source_adapter: Option<String>,
    /// The adapter's own reference.
    #[serde(default)]
    pub source_ref: Option<String>,
    /// Import only: keep the creation timestamp it arrived with.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Import only: keep the update timestamp it arrived with.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Import only: keep the activation timestamp it arrived with.
    #[serde(default)]
    pub activated_at: Option<String>,
}

impl NewLearning {
    /// Build the minimum a write needs. Tests and the CLI both start here.
    pub fn new(
        title: impl Into<String>,
        rule: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            rule: rule.into(),
            rationale: rationale.into(),
            scopes: Vec::new(),
            blocking: true,
            matcher_kind: None,
            matcher: None,
            exemplars: Vec::new(),
            status: None,
            author: None,
            source_kind: None,
            source_adapter: None,
            source_ref: None,
            created_at: None,
            updated_at: None,
            activated_at: None,
        }
    }

    /// The status this write asks for. Absent means `proposed`, which is
    /// invariant 2: a caller that forgets `--activate` fails safe.
    pub fn effective_status(&self) -> Status {
        self.status.unwrap_or(Status::Proposed)
    }

    /// The scopes this write stores. An empty list becomes `global`.
    ///
    /// A learning with no scope row would never be selected by an audit and
    /// would never say so. That is the invisible failure P7 exists to
    /// prevent, so the write refuses to produce one.
    pub fn effective_scopes(&self) -> Vec<Scope> {
        if self.scopes.is_empty() {
            vec![Scope::global()]
        } else {
            self.scopes.clone()
        }
    }

    /// Refuse anything the schema or the CLI reference does not allow.
    pub fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("title", &self.title),
            ("rule", &self.rule),
            ("rationale", &self.rationale),
        ] {
            if value.trim().is_empty() {
                return Err(Error::validation(format!("{field} is required and empty")));
            }
        }
        if self.status == Some(Status::Archived) {
            return Err(Error::validation(
                "a write cannot set status archived. Use `writ archive ID`",
            ));
        }
        match (&self.matcher, &self.matcher_kind) {
            (Some(_), None) => {
                return Err(Error::validation(
                    "--matcher needs --matcher-kind: ast_grep or regex",
                ));
            }
            (None, Some(_)) => {
                return Err(Error::validation("--matcher-kind needs --matcher"));
            }
            _ => {}
        }
        let mut scopes = self.effective_scopes();
        scopes.sort();
        let before = scopes.len();
        scopes.dedup();
        if scopes.len() != before {
            return Err(Error::validation("the same scope is given twice"));
        }
        reject_global_with_another_kind(&scopes)?;
        for exemplar in &self.exemplars {
            if exemplar.snippet.is_empty() {
                return Err(Error::validation("an exemplar snippet is empty"));
            }
        }
        Ok(())
    }
}

/// Refuse `global` beside any other scope kind. Spec section 7.1 step 2.
///
/// Kinds AND across each other, so every other kind narrows a rule and
/// `global` does not. "Everywhere, and also only Elixir" has no meaning,
/// and silently keeping one half of the pair is the invisible failure P7
/// exists to prevent.
///
/// Both `NewLearning` and `LearningUpdate` call this, so a write and an
/// edit cannot drift apart on it.
fn reject_global_with_another_kind(scopes: &[Scope]) -> Result<()> {
    if scopes.iter().any(|one| one.kind == ScopeKind::Global) && scopes.len() > 1 {
        return Err(Error::validation(
            "the global scope cannot be combined with another scope. \
             Every other scope kind narrows the rule, and global does not",
        ));
    }
    Ok(())
}

/// One learning as it is read back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Learning {
    /// UUIDv7.
    pub id: String,
    /// When it was written.
    pub created_at: String,
    /// When it last changed. Maintained by a trigger. See invariant 7.
    pub updated_at: String,
    /// Where it stands.
    pub status: Status,
    /// A short name.
    pub title: String,
    /// What to do.
    pub rule: String,
    /// Why.
    pub rationale: String,
    /// Whether breaking it stops the handoff.
    pub blocking: bool,
    /// The dialect of `matcher`.
    pub matcher_kind: Option<MatcherKind>,
    /// The retrieval matcher.
    pub matcher: Option<String>,
    /// How it arrived.
    pub source_kind: SourceKind,
    /// Which adapter produced it.
    pub source_adapter: Option<String>,
    /// The adapter's own reference.
    pub source_ref: Option<String>,
    /// Who wrote it.
    pub author: Option<String>,
    /// When it first became active.
    pub activated_at: Option<String>,
    /// How many times it was reinforced.
    pub reinforced: i64,
    /// How many audits put it in front of a reviewer. Moved at emit.
    pub times_selected: i64,
    /// When an audit last selected it. Moved at emit.
    pub last_selected_at: Option<String>,
    /// How many findings it has produced. Moved at ingest.
    pub times_applied: i64,
    /// When it last caught something. Moved at ingest.
    pub last_applied_at: Option<String>,
    /// When it was last confirmed to still hold.
    pub last_verified: Option<String>,
    /// Where it applies.
    pub scopes: Vec<Scope>,
}

/// What a write did, so the caller can report it. crit #446: every write
/// reports what it wrote.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recorded {
    /// The learning that was written, or reinforced.
    pub id: String,
    /// Whether an existing learning was reinforced instead of created.
    pub reinforced: bool,
}

/// The filters `writ list` accepts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListFilter {
    /// Only this status.
    pub status: Option<Status>,
    /// Only learnings carrying this exact scope row.
    pub scope: Option<Scope>,
    /// Only learnings whose last selection, or creation when no audit has
    /// reached them yet, is at least this many days old. This is the reach
    /// axis: has anything put the rule in front of a reviewer?
    ///
    /// `times_selected` on each row says which case a match is. `0` means
    /// misscoped and needs rewording. A larger number means the rule used
    /// to be reached and stopped, so it is probably dead. Splitting that
    /// into two flags is right about the diagnosis and wrong about the
    /// interface: the caller reads it off the output.
    pub unused_days: Option<u32>,
    /// Only learnings that were selected at least once and have never
    /// caught anything. This is the usefulness axis.
    ///
    /// `times_selected > 0` is part of the filter, not an accident. A rule
    /// nothing ever selected has caught nothing trivially, and reporting
    /// that here would put one row in both buckets with two contradictory
    /// suggested fixes.
    pub never_applied: bool,
    /// Only learnings whose title, rule or rationale match.
    pub search: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_round_trips_through_its_string_form() {
        for text in [
            "global",
            "project:github.com/a/b",
            "language:rust",
            "glob:**/*.rs",
        ] {
            let scope: Scope = text.parse().unwrap();
            assert_eq!(scope.to_string(), text);
        }
    }

    #[test]
    fn a_glob_keeps_a_colon_in_its_value() {
        let scope: Scope = "glob:a:b".parse().unwrap();
        assert_eq!(scope.value, "a:b");
    }

    #[test]
    fn a_valued_global_scope_is_refused() {
        let error: Error = "global:x".parse::<Scope>().unwrap_err();
        assert!(error.to_string().contains("takes no value"), "{error}");
    }

    #[test]
    fn a_valueless_project_scope_is_refused() {
        let error: Error = "project".parse::<Scope>().unwrap_err();
        assert!(error.to_string().contains("needs a value"), "{error}");
        let error: Error = "project:".parse::<Scope>().unwrap_err();
        assert!(error.to_string().contains("needs a value"), "{error}");
    }

    #[test]
    fn an_unknown_scope_kind_names_the_four_that_work() {
        let error: Error = "team:core".parse::<Scope>().unwrap_err();
        assert!(error.to_string().contains("global, project"), "{error}");
    }

    #[test]
    fn status_defaults_to_proposed() {
        // Invariant 2.
        let learning = NewLearning::new("t", "r", "why");
        assert_eq!(learning.effective_status(), Status::Proposed);
    }

    #[test]
    fn a_write_cannot_ask_for_archived() {
        let error = Status::parse_writable("archived").unwrap_err();
        assert!(error.to_string().contains("writ archive"), "{error}");
    }

    #[test]
    fn no_scope_means_global() {
        let learning = NewLearning::new("t", "r", "why");
        assert_eq!(learning.effective_scopes(), vec![Scope::global()]);
    }

    #[test]
    fn an_empty_rationale_is_refused() {
        let mut learning = NewLearning::new("t", "r", "   ");
        let error = learning.validate().unwrap_err();
        assert!(error.to_string().contains("rationale"), "{error}");
        learning.rationale = "because".into();
        learning.validate().unwrap();
    }

    #[test]
    fn a_matcher_and_its_kind_travel_together() {
        let mut learning = NewLearning::new("t", "r", "why");
        learning.matcher = Some("$A == $A".into());
        assert!(learning.validate().is_err());
        learning.matcher_kind = Some(MatcherKind::AstGrep);
        learning.validate().unwrap();
        learning.matcher = None;
        assert!(learning.validate().is_err());
    }

    /// Section 7.1 step 2: kinds AND across each other, so "everywhere,
    /// and also only Elixir" has no meaning. Keeping one half of the pair
    /// silently is the invisible failure P7 exists to prevent.
    #[test]
    fn global_cannot_be_combined_with_another_scope() {
        let mut learning = NewLearning::new("t", "r", "why");
        learning.scopes = vec!["global".parse().unwrap(), "language:rust".parse().unwrap()];
        let error = learning.validate().unwrap_err();
        assert!(error.to_string().contains("cannot be combined"), "{error}");

        learning.scopes = vec!["global".parse().unwrap()];
        learning.validate().unwrap();
        learning.scopes = vec![
            "language:rust".parse().unwrap(),
            "project:github.com/o/r".parse().unwrap(),
        ];
        learning.validate().unwrap();
    }

    #[test]
    fn a_repeated_scope_is_refused_before_sqlite_sees_it() {
        let mut learning = NewLearning::new("t", "r", "why");
        learning.scopes = vec!["language:rust".parse().unwrap(); 2];
        let error = learning.validate().unwrap_err();
        assert!(error.to_string().contains("twice"), "{error}");
    }
}
