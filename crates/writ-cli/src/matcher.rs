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

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use writ_core::{Diff, Learning, MatcherKind, ScopeKind, Sides, language_of};

use crate::git;

/// What one matcher said about one diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The shape is in the diff. Select the learning.
    Hit,
    /// The shape is not in the diff. Drop the learning.
    Miss,
    /// The post-image missed and a pre-image was unavailable. Drop the
    /// learning as before, but report that evaluation fell back to the
    /// post-image. P6.
    MissWithNotice(String),
    /// The matcher could not be run or could not be read. Keep the
    /// learning on scope alone, and say why. P6.
    Unevaluable(String),
}

impl Verdict {
    /// Whether the learning stays in the running.
    pub fn keeps(&self) -> bool {
        !matches!(self, Self::Miss | Self::MissWithNotice(_))
    }
}

/// Evaluate a learning's matcher against this diff.
///
/// A learning with no matcher is kept on scope alone, which is section 7.1
/// step 2, except when `sides` excludes every half the diff actually has.
pub fn evaluate(learning: &Learning, diff: &Diff, root: &Path) -> Verdict {
    if !sides_present(learning.sides, diff) {
        return Verdict::Miss;
    }
    let (Some(pattern), Some(kind)) = (&learning.matcher, learning.matcher_kind) else {
        return Verdict::Hit;
    };
    match kind {
        MatcherKind::Regex => regex_verdict(pattern, diff, learning.sides),
        MatcherKind::AstGrep => ast_grep_verdict(pattern, learning, diff, root),
    }
}

fn sides_present(sides: Sides, diff: &Diff) -> bool {
    match sides {
        Sides::Both => true,
        Sides::Added => !diff.added.is_empty(),
        Sides::Removed => !diff.removed.is_empty(),
    }
}

/// A regex runs over the halves `sides` allows, not the whole diff, so
/// file headers cannot themselves produce a hit.
fn regex_verdict(pattern: &str, diff: &Diff, sides: Sides) -> Verdict {
    match regex::Regex::new(pattern) {
        Ok(regex) => {
            let hit_added = sides.includes_added() && regex.is_match(&diff.added);
            let hit_removed = sides.includes_removed() && regex.is_match(&diff.removed);
            if hit_added || hit_removed {
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
    let check_added = learning.sides.includes_added();
    let check_removed = learning.sides.includes_removed();

    if check_added {
        let present: HashSet<&str> = diff
            .paths
            .iter()
            .map(String::as_str)
            .filter(|path| root.join(path).is_file())
            .collect();
        let files: Vec<String> = present.iter().map(|path| (*path).to_string()).collect();
        if !files.is_empty() {
            match run_ast_grep_files(pattern, learning, &files, root) {
                Verdict::Hit => return Verdict::Hit,
                Verdict::Unevaluable(why) => return Verdict::Unevaluable(why),
                Verdict::Miss | Verdict::MissWithNotice(_) => {}
            }
        }
    }

    if !check_removed {
        return Verdict::Miss;
    }

    // Add-only diffs have nothing to recover from a pre-image.
    if diff.removed.is_empty() {
        return Verdict::Miss;
    }

    let present: HashSet<&str> = diff
        .paths
        .iter()
        .map(String::as_str)
        .filter(|path| root.join(path).is_file())
        .collect();
    let language_scope = language_scope(learning);
    let mut fallback_notices = Vec::new();
    for path in &diff.paths {
        let object = format!("HEAD:{path}");
        let pre_image = match git::show_bytes(root, &object) {
            Ok(bytes) => bytes,
            Err(why) => {
                let why = format!("cannot read pre-image {object}: {why}");
                if present.contains(path.as_str()) {
                    fallback_notices.push(why);
                    continue;
                }
                return Verdict::Unevaluable(why);
            }
        };
        let language = language_scope.or_else(|| language_of(path));
        let Some(language) = language else {
            let why = format!("cannot parse pre-image {object}: its language cannot be inferred");
            if present.contains(path.as_str()) {
                fallback_notices.push(why);
                continue;
            }
            return Verdict::Unevaluable(why);
        };
        match run_ast_grep_stdin(pattern, language, &pre_image, root) {
            Verdict::Hit => return Verdict::Hit,
            Verdict::Unevaluable(why) => return Verdict::Unevaluable(why),
            Verdict::Miss | Verdict::MissWithNotice(_) => {}
        }
    }

    if fallback_notices.is_empty() {
        Verdict::Miss
    } else {
        Verdict::MissWithNotice(fallback_notices.join("; "))
    }
}

fn language_scope(learning: &Learning) -> Option<&str> {
    learning
        .scopes
        .iter()
        .find(|scope| scope.kind == ScopeKind::Language)
        .map(|scope| scope.value.as_str())
}

fn run_ast_grep_files(
    pattern: &str,
    learning: &Learning,
    files: &[String],
    root: &Path,
) -> Verdict {
    let mut command = Command::new("ast-grep");
    command.args(["run", "--pattern", pattern, "--json"]);
    // ast-grep infers the language from each file extension. A
    // `language:` scope overrides that, because a learning scoped to one
    // language means its pattern is written in that language's grammar.
    if let Some(language) = language_scope(learning) {
        command.args(["--lang", language]);
    }
    let output = command.args(files).current_dir(root).output();

    let output = match output {
        Ok(output) => output,
        // The binary is not installed. This is the P6 case section 11
        // lists as a CI check.
        Err(error) => {
            return Verdict::Unevaluable(format!("cannot run ast-grep: {error}"));
        }
    };
    read_ast_grep_output(output)
}

fn run_ast_grep_stdin(pattern: &str, language: &str, input: &[u8], root: &Path) -> Verdict {
    let mut child = match Command::new("ast-grep")
        .args([
            "run",
            "--pattern",
            pattern,
            "--lang",
            language,
            "--json",
            "--stdin",
        ])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return Verdict::Unevaluable(format!("cannot run ast-grep: {error}")),
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.wait_with_output();
        return Verdict::Unevaluable("cannot open ast-grep stdin for the pre-image".to_string());
    };
    let write_result = stdin.write_all(input);
    drop(stdin);
    if let Err(error) = write_result
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        let _ = child.wait_with_output();
        return Verdict::Unevaluable(format!("cannot send pre-image to ast-grep: {error}"));
    }
    match child.wait_with_output() {
        Ok(output) => read_ast_grep_output(output),
        Err(error) => Verdict::Unevaluable(format!("cannot read ast-grep output: {error}")),
    }
}

fn read_ast_grep_output(output: Output) -> Verdict {
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
