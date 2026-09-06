//! Evaluating a retrieval matcher. Spec section 8.2.
//!
//! A matcher answers one question: is this rule in play for this diff? A
//! hit selects the learning into the prompt. It is **not** a finding.
//! Judgment stays with the host, so nothing here reports a violation.
//!
//! P6 governs the failure mode. When `ast-grep` is not installed, or the
//! pattern does not parse, the learning is kept on scope alone and the
//! audit still succeeds. An optional capability degrades the result. It
//! never fails it.

use std::path::Path;
use std::process::Command;

use writ_core::{Diff, Learning, MatcherKind, ScopeKind};

/// What one matcher said about one diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The shape is in the diff. Select the learning.
    Hit,
    /// The shape is not in the diff. Drop the learning.
    Miss,
    /// The matcher could not be run or could not be read. Keep the
    /// learning on scope alone, and say why. P6.
    Unevaluable(String),
}

impl Verdict {
    /// Whether the learning stays in the running.
    pub fn keeps(&self) -> bool {
        !matches!(self, Self::Miss)
    }
}

/// Evaluate a learning's matcher against this diff.
///
/// A learning with no matcher is kept on scope alone, which is section 7.1
/// step 2.
pub fn evaluate(learning: &Learning, diff: &Diff, root: &Path) -> Verdict {
    let (Some(pattern), Some(kind)) = (&learning.matcher, learning.matcher_kind) else {
        return Verdict::Hit;
    };
    match kind {
        MatcherKind::Regex => regex_verdict(pattern, diff),
        MatcherKind::AstGrep => ast_grep_verdict(pattern, learning, diff, root),
    }
}

/// A regex runs over the **added** lines, not the whole diff.
///
/// The whole diff carries the removed lines and the file headers, so a
/// pattern would hit the very code the change deleted and select a rule
/// about a shape that is no longer there.
fn regex_verdict(pattern: &str, diff: &Diff) -> Verdict {
    match regex::Regex::new(pattern) {
        Ok(regex) => {
            if regex.is_match(&diff.added) {
                Verdict::Hit
            } else {
                Verdict::Miss
            }
        }
        Err(error) => Verdict::Unevaluable(format!("the regex does not compile: {error}")),
    }
}

/// Shell out to `ast-grep`. Spec section 8.2: shell out now, embed on a
/// trigger.
fn ast_grep_verdict(pattern: &str, learning: &Learning, diff: &Diff, root: &Path) -> Verdict {
    let files: Vec<String> = diff
        .paths
        .iter()
        .filter(|path| root.join(path).is_file())
        .cloned()
        .collect();
    if files.is_empty() {
        return Verdict::Unevaluable(
            "no changed file is present in the working tree to parse".to_string(),
        );
    }

    let mut command = Command::new("ast-grep");
    command.args(["run", "--pattern", pattern, "--json"]);
    // ast-grep infers the language from each file extension. A
    // `language:` scope overrides that, because a learning scoped to one
    // language means its pattern is written in that language's grammar.
    if let Some(language) = learning
        .scopes
        .iter()
        .find(|scope| scope.kind == ScopeKind::Language)
    {
        command.args(["--lang", &language.value]);
    }
    let output = command.args(&files).current_dir(root).output();

    let output = match output {
        Ok(output) => output,
        // The binary is not installed. This is the P6 case section 11
        // lists as a CI check.
        Err(error) => {
            return Verdict::Unevaluable(format!("cannot run ast-grep: {error}"));
        }
    };
    // A pattern that does not parse is not an error to ast-grep. It prints
    // an empty result and warns, which would otherwise read as a clean
    // miss and silently drop the rule.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("ERROR node") {
        return Verdict::Unevaluable("the ast-grep pattern does not parse".to_string());
    }

    // The exit code is not the answer. ast-grep exits `1` when it matched
    // nothing, which is a clean miss, so the JSON is read first and the
    // exit code only decides what an unreadable stdout means.
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<serde_json::Value>(&stdout) {
        Ok(serde_json::Value::Array(hits)) if hits.is_empty() => Verdict::Miss,
        Ok(serde_json::Value::Array(_)) => Verdict::Hit,
        _ => Verdict::Unevaluable(format!(
            "ast-grep printed nothing writ can read: {}",
            stderr.trim()
        )),
    }
}
