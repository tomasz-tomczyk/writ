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

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use writ_core::{Diff, Hit, Learning, MatcherKind, ScopeKind, Sides, language_of, matched_paths};

use crate::git;

/// What one matcher said about one diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The learning has no matcher. Keep it on scope alone.
    Scope,
    /// The shape is in the diff. Select the learning.
    ///
    /// The hits are the matches that fall on a line the diff changed, in
    /// a path the learning's scopes selected. They may be empty: selection
    /// still counts a match on an unchanged line, as section 8.2 always
    /// has, and only what the prompt shows is filtered. Spec section 9.2,
    /// **The prompt points at the diff, it does not carry it**.
    Hit(Vec<Hit>),
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

    /// What the prompt lists under `start here`, if anything.
    pub fn hits(self) -> Option<Vec<Hit>> {
        match self {
            Self::Hit(hits) => Some(hits),
            _ => None,
        }
    }
}

/// Evaluate a learning's matcher against this diff.
///
/// A learning with no matcher is kept on scope alone, which is section 7.1
/// step 2, except when `sides` excludes every half the diff actually has.
///
/// `base` is the commit the diff's removed lines come from, from
/// [`git::range_base`]. The pre-image is read there, because a Stop hook
/// audits from the merge-base and a shape removed in an earlier commit is
/// already gone from HEAD.
pub fn evaluate(learning: &Learning, diff: &Diff, root: &Path, base: &str) -> Verdict {
    if !sides_present(learning.sides, diff) {
        return Verdict::Miss;
    }
    let (Some(pattern), Some(kind)) = (&learning.matcher, learning.matcher_kind) else {
        return Verdict::Scope;
    };
    match kind {
        MatcherKind::Regex => regex_verdict(pattern, learning, diff),
        MatcherKind::AstGrep => ast_grep_verdict(pattern, learning, diff, root, base),
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
///
/// Whether it selects is decided over the joined text, as it always was,
/// so a pattern spanning lines still selects. The hits are read line by
/// line, because a line is what the prompt can point at.
fn regex_verdict(pattern: &str, learning: &Learning, diff: &Diff) -> Verdict {
    let sides = learning.sides;
    match regex::Regex::new(pattern) {
        Ok(regex) => {
            let hit_added = sides.includes_added() && regex.is_match(&diff.added);
            let hit_removed = sides.includes_removed() && regex.is_match(&diff.removed);
            if !(hit_added || hit_removed) {
                return Verdict::Miss;
            }
            // Added lines first, across every path: what the change wrote
            // is what a rule most often objects to.
            let paths = matched_paths(learning, diff);
            let mut hits = Vec::new();
            for (removed, wanted, lines) in [
                (false, sides.includes_added(), &diff.added_lines),
                (true, sides.includes_removed(), &diff.removed_lines),
            ] {
                if !wanted {
                    continue;
                }
                for path in &paths {
                    for (line, text) in lines.get(path).into_iter().flatten() {
                        if regex.is_match(text) {
                            hits.push(Hit {
                                path: path.clone(),
                                line: *line,
                                removed,
                                text: text.clone(),
                            });
                        }
                    }
                }
            }
            Verdict::Hit(hits)
        }
        Err(error) => Verdict::Unevaluable(format!("the regex does not compile: {error}")),
    }
}

/// One match ast-grep reported: the file it named, and its first and last
/// line, counted from 0 as ast-grep counts them.
struct Match {
    file: Option<String>,
    first: u32,
    last: u32,
}

/// Shell out to `ast-grep`. Spec section 8.2: shell out now, embed on a
/// trigger.
///
/// ast-grep scans whole files, so a match can sit on a line the diff never
/// touched. Such a match still selects, as it always did. It is not shown:
/// only a match that covers a changed line becomes a hit, or `start here`
/// would point at old code.
fn ast_grep_verdict(
    pattern: &str,
    learning: &Learning,
    diff: &Diff,
    root: &Path,
    base: &str,
) -> Verdict {
    let present = paths_on_disk(diff, root);
    let wanted: HashSet<String> = matched_paths(learning, diff).into_iter().collect();
    let mut matched = false;
    let mut hits = Vec::new();

    if learning.sides.includes_added() {
        let files: Vec<String> = present.iter().map(|path| (*path).to_string()).collect();
        if !files.is_empty() {
            match run_ast_grep_files(pattern, learning, &files, root) {
                Ok(matches) => {
                    matched |= !matches.is_empty();
                    for one in &matches {
                        let Some(path) = one.file.as_deref() else {
                            continue;
                        };
                        if wanted.contains(path) {
                            hits.extend(changed_hit(one, path, &diff.added_lines, false));
                        }
                    }
                }
                Err(why) => return Verdict::Unevaluable(why),
            }
        }
    }

    // Add-only diffs, or sides that ignore removals, have nothing to
    // recover from a pre-image. A post-image that already has something to
    // show needs no pre-image either.
    if !learning.sides.includes_removed() || diff.removed.is_empty() || !hits.is_empty() {
        return if matched {
            Verdict::Hit(hits)
        } else {
            Verdict::Miss
        };
    }

    let language_scope = language_scope(learning);
    let mut fallback_notices = Vec::new();
    for path in &diff.paths {
        let object = format!("{base}:{path}");
        // Once the learning is selected, a pre-image that cannot be read
        // costs only a hit, never the selection.
        let pre_image = match git::show_bytes(root, &object) {
            Ok(bytes) => bytes,
            Err(_) if matched => continue,
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
            if matched {
                continue;
            }
            let why = format!("cannot parse pre-image {object}: its language cannot be inferred");
            if present.contains(path.as_str()) {
                fallback_notices.push(why);
                continue;
            }
            return Verdict::Unevaluable(why);
        };
        match run_ast_grep_stdin(pattern, language, &pre_image, root) {
            Ok(matches) => {
                matched |= !matches.is_empty();
                if wanted.contains(path) {
                    for one in &matches {
                        hits.extend(changed_hit(one, path, &diff.removed_lines, true));
                    }
                }
                if !hits.is_empty() {
                    break;
                }
            }
            Err(_) if matched => continue,
            Err(why) => return Verdict::Unevaluable(why),
        }
    }

    if matched {
        Verdict::Hit(hits)
    } else if fallback_notices.is_empty() {
        Verdict::Miss
    } else {
        Verdict::MissWithNotice(fallback_notices.join("; "))
    }
}

/// The first line of a match that the diff changed, as a hit.
fn changed_hit(
    one: &Match,
    path: &str,
    lines: &BTreeMap<String, BTreeMap<u32, String>>,
    removed: bool,
) -> Option<Hit> {
    let changed = lines.get(path)?;
    (one.first..=one.last).find_map(|line| {
        changed.get(&(line + 1)).map(|text| Hit {
            path: path.to_string(),
            line: line + 1,
            removed,
            text: text.clone(),
        })
    })
}

/// Diff paths that still exist on disk. Used both to run post-image
/// ast-grep and to decide whether a missing pre-image is a soft fallback.
fn paths_on_disk<'a>(diff: &'a Diff, root: &Path) -> HashSet<&'a str> {
    diff.paths
        .iter()
        .map(String::as_str)
        .filter(|path| root.join(path).is_file())
        .collect()
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
) -> Result<Vec<Match>, String> {
    let mut command = Command::new("ast-grep");
    command.args(["run", "--pattern", pattern, "--json"]);
    // ast-grep infers the language from each file extension. A
    // `language:` scope overrides that, because a learning scoped to one
    // language means its pattern is written in that language's grammar.
    if let Some(language) = language_scope(learning) {
        command.args(["--lang", language]);
    }
    let output = command.args(files).current_dir(root).output();

    // The binary is not installed. This is the P6 case section 11 lists as
    // a CI check.
    let output = output.map_err(|error| format!("cannot run ast-grep: {error}"))?;
    read_ast_grep_output(output)
}

fn run_ast_grep_stdin(
    pattern: &str,
    language: &str,
    input: &[u8],
    root: &Path,
) -> Result<Vec<Match>, String> {
    let mut child = Command::new("ast-grep")
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
        .map_err(|error| format!("cannot run ast-grep: {error}"))?;

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.wait_with_output();
        return Err("cannot open ast-grep stdin for the pre-image".to_string());
    };
    let write_result = stdin.write_all(input);
    drop(stdin);
    if let Err(error) = write_result
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        let _ = child.wait_with_output();
        return Err(format!("cannot send pre-image to ast-grep: {error}"));
    }
    match child.wait_with_output() {
        Ok(output) => read_ast_grep_output(output),
        Err(error) => Err(format!("cannot read ast-grep output: {error}")),
    }
}

fn read_ast_grep_output(output: Output) -> Result<Vec<Match>, String> {
    // A pattern that does not parse is not an error to ast-grep. It prints
    // an empty result and warns, which would otherwise read as a clean
    // miss and silently drop the rule.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("ERROR node") {
        return Err("the ast-grep pattern does not parse".to_string());
    }

    // The exit code is not the answer. ast-grep exits `1` when it matched
    // nothing, which is a clean miss, so the JSON is read first and the
    // exit code only decides what an unreadable stdout means.
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<serde_json::Value>(&stdout) {
        Ok(serde_json::Value::Array(matches)) => Ok(matches
            .iter()
            .map(|one| {
                let line = |end: &str| {
                    one["range"][end]["line"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or(0)
                };
                Match {
                    file: one["file"].as_str().map(str::to_string),
                    first: line("start"),
                    last: line("end"),
                }
            })
            .collect()),
        _ => Err(format!(
            "ast-grep printed nothing writ can read: {}",
            stderr.trim()
        )),
    }
}
