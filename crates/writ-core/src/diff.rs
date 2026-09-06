//! Reading a unified diff. Spec section 7.1 step 1.
//!
//! Running `git diff` is I/O and lives in `writ-cli`. Reading the text it
//! prints is pure and lives here. Invariant 1.

use crate::glob::glob_match;

/// One diff, as an audit sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    /// The whole unified diff, as git printed it. This goes in the prompt.
    pub text: String,
    /// Every path the diff touches, in the order git names them.
    pub paths: Vec<String>,
    /// The added lines only, with their `+` removed.
    ///
    /// A `regex` matcher runs against this rather than the whole diff, so
    /// a pattern cannot hit the very line the change removed.
    pub added: String,
}

impl Diff {
    /// Read a unified diff.
    pub fn parse(text: &str) -> Self {
        let mut paths: Vec<String> = Vec::new();
        let mut added = String::new();

        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("+++ ") {
                if let Some(path) = strip_prefix_marker(rest)
                    && !paths.iter().any(|seen| seen == path)
                {
                    paths.push(path.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("--- ") {
                // A deletion has `+++ /dev/null`, so the old side is the
                // only place the path appears.
                if let Some(path) = strip_prefix_marker(rest)
                    && !paths.iter().any(|seen| seen == path)
                {
                    paths.push(path.to_string());
                }
            } else if let Some(rest) = line.strip_prefix('+') {
                added.push_str(rest);
                added.push('\n');
            }
        }

        Self {
            text: text.to_string(),
            paths,
            added,
        }
    }

    /// Whether the diff changed nothing. Section 5.7 gives this exit `7`.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The languages this diff touches, sorted and without repeats.
    pub fn languages(&self) -> Vec<String> {
        let mut languages: Vec<String> = self
            .paths
            .iter()
            .filter_map(|path| language_of(path))
            .map(str::to_string)
            .collect();
        languages.sort();
        languages.dedup();
        languages
    }

    /// Whether any changed path matches this `glob:` scope value.
    pub fn matches_glob(&self, pattern: &str) -> bool {
        self.paths.iter().any(|path| glob_match(pattern, path))
    }
}

/// Take the path out of a `+++ b/src/main.rs` line.
///
/// `/dev/null` is not a path, and neither is the `a/` or `b/` git puts in
/// front of one.
fn strip_prefix_marker(rest: &str) -> Option<&str> {
    let path = rest.split('\t').next().unwrap_or(rest).trim_end();
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    Some(
        path.strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path),
    )
}

/// The language name for a path, by extension.
///
/// The names are ast-grep's, because section 8.1 makes ast-grep the
/// matcher dialect and a `language:` scope has to mean the same thing in
/// both places. An unknown extension yields nothing rather than a guess.
pub fn language_of(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => "rust",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" | "pyi" => "python",
        "go" => "go",
        "rb" | "rake" => "ruby",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "scala" => "scala",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "lua" => "lua",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" | "scss" => "css",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/src/main.rs b/src/main.rs\n\
        index 111..222 100644\n\
        --- a/src/main.rs\n\
        +++ b/src/main.rs\n\
        @@ -1,2 +1,2 @@\n\
        -let x = 1;\n\
        +let y = 2;\n\
        diff --git a/lib/app.ex b/lib/app.ex\n\
        --- /dev/null\n\
        +++ b/lib/app.ex\n\
        @@ -0,0 +1 @@\n\
        +defmodule App do\n";

    #[test]
    fn every_touched_path_is_read_once() {
        let diff = Diff::parse(SAMPLE);
        assert_eq!(diff.paths, ["src/main.rs", "lib/app.ex"]);
    }

    #[test]
    fn a_deleted_file_is_still_a_touched_path() {
        let text = "diff --git a/gone.rs b/gone.rs\n--- a/gone.rs\n+++ /dev/null\n";
        assert_eq!(Diff::parse(text).paths, ["gone.rs"]);
    }

    /// The `+++` header is not an added line, or every diff would carry
    /// its own file names into the regex matcher.
    #[test]
    fn the_added_text_holds_the_added_lines_only() {
        let diff = Diff::parse(SAMPLE);
        assert_eq!(diff.added, "let y = 2;\ndefmodule App do\n");
    }

    #[test]
    fn languages_come_from_the_extensions() {
        let diff = Diff::parse(SAMPLE);
        assert_eq!(diff.languages(), ["elixir", "rust"]);
    }

    #[test]
    fn an_unknown_extension_yields_no_language() {
        assert_eq!(language_of("data.qqq"), None);
        assert_eq!(language_of("Makefile"), None);
    }

    #[test]
    fn an_empty_diff_says_so() {
        assert!(Diff::parse("").is_empty());
        assert!(Diff::parse("   \n").is_empty());
        assert!(!Diff::parse(SAMPLE).is_empty());
    }

    #[test]
    fn a_glob_scope_is_matched_against_the_touched_paths() {
        let diff = Diff::parse(SAMPLE);
        assert!(diff.matches_glob("lib/**"));
        assert!(!diff.matches_glob("web/**"));
    }
}
