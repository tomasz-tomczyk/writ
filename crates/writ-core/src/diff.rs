//! Reading a unified diff. Spec section 7.1 step 1.
//!
//! Running `git diff` is I/O and lives in `writ-cli`. Reading the text it
//! prints is pure and lives here. Invariant 1.

use std::collections::{BTreeMap, BTreeSet};

use crate::glob::glob_match;

/// The one extension-to-language allowlist used by scoping and telemetry.
///
/// Telemetry must never record an extension supplied by a caller. Keeping the
/// allowlist in this module means retrieval and telemetry cannot quietly grow
/// different language vocabularies.
const EXTENSION_LANGUAGES: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("erl", "erlang"),
    ("hrl", "erlang"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "tsx"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascript"),
    ("py", "python"),
    ("pyi", "python"),
    ("go", "go"),
    ("rb", "ruby"),
    ("rake", "ruby"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("swift", "swift"),
    ("scala", "scala"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
    ("cs", "csharp"),
    ("php", "php"),
    ("lua", "lua"),
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "bash"),
    ("sql", "sql"),
    ("html", "html"),
    ("htm", "html"),
    ("css", "css"),
    ("scss", "css"),
    ("json", "json"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("toml", "toml"),
    ("md", "markdown"),
    ("markdown", "markdown"),
];

/// One diff, as an audit sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    /// The whole unified diff, as git printed it. This goes in the prompt.
    pub text: String,
    /// Every path the diff touches, in the order git names them.
    pub paths: Vec<String>,
    /// The added lines only, with their `+` removed.
    pub added: String,
    /// The removed lines only, with their `-` removed.
    pub removed: String,
    /// The diff text belonging to each path, keyed by path.
    ///
    /// A `BTreeMap` because [`Diff::slice`] concatenates in sorted-path
    /// order, so the digest a slice hashes to does not depend on the order
    /// git happened to name the files in.
    pub sections: BTreeMap<String, String>,
}

impl Diff {
    /// Read a unified diff.
    pub fn parse(text: &str) -> Self {
        let mut paths: Vec<String> = Vec::new();
        let mut added = String::new();
        let mut removed = String::new();
        let mut sections: BTreeMap<String, String> = BTreeMap::new();
        // File headers (`---` / `+++`) only appear outside hunks. Inside a
        // hunk a content line can begin `-- `, which the patch renders as
        // `--- …` and must not be mistaken for a header.
        let mut in_hunk = false;
        // The lines of the file currently being read, and the path they
        // belong to once a header names it. The path arrives *after* the
        // `diff --git` line, so the buffer fills before it has a key.
        let mut buffer = String::new();
        let mut buffer_path: Option<String> = None;

        // Close the open section, if a header has named one.
        macro_rules! flush {
            () => {
                if let Some(path) = buffer_path.take() {
                    sections
                        .entry(path)
                        .or_default()
                        .push_str(std::mem::take(&mut buffer).as_str());
                } else {
                    buffer.clear();
                }
            };
        }

        for line in text.lines() {
            // A `diff --git` line is the only section boundary. `git diff`
            // always emits one per file, and it is also the only line that
            // clears `in_hunk` — so a bare unified diff with no such header
            // reads as one section, exactly as it already reads as one file.
            // Loosening this to treat an out-of-hunk `--- ` as a boundary
            // would mean clearing `in_hunk` on it too, and that is the
            // guard keeping a `--- ` content line from being read as a
            // header.
            if line.starts_with("diff ") {
                flush!();
                in_hunk = false;
            }
            buffer.push_str(line);
            buffer.push('\n');

            if line.starts_with("diff ") {
                continue;
            }
            if line.starts_with("@@") {
                in_hunk = true;
                continue;
            }
            if !in_hunk {
                if let Some(rest) = line.strip_prefix("+++ ") {
                    if let Some(path) = strip_prefix_marker(rest) {
                        if !paths.iter().any(|seen| seen == path) {
                            paths.push(path.to_string());
                        }
                        // The post-image name wins: a rename's hunk belongs
                        // to the file as it now exists, which is the name a
                        // `glob:` scope is written against.
                        buffer_path = Some(path.to_string());
                    }
                    continue;
                }
                if let Some(rest) = line.strip_prefix("--- ") {
                    // A deletion has `+++ /dev/null`, so the old side is the
                    // only place the path appears.
                    if let Some(path) = strip_prefix_marker(rest) {
                        if !paths.iter().any(|seen| seen == path) {
                            paths.push(path.to_string());
                        }
                        buffer_path.get_or_insert_with(|| path.to_string());
                    }
                    continue;
                }
            }
            if let Some(rest) = line.strip_prefix('+') {
                added.push_str(rest);
                added.push('\n');
            } else if let Some(rest) = line.strip_prefix('-') {
                removed.push_str(rest);
                removed.push('\n');
            }
        }

        flush!();

        Self {
            text: text.to_string(),
            paths,
            added,
            removed,
            sections,
        }
    }

    /// The diff text belonging to `paths` alone, in sorted-path order.
    ///
    /// This is what a learning's coverage digest hashes. Spec section 9.2,
    /// **The digest has to match the granularity of selection**: keying
    /// coverage on the whole diff re-served a `glob:`-scoped rule on every
    /// turn that edited any other file, because the whole-diff digest moved
    /// and the rule's own concern had not.
    ///
    /// Sorted rather than in git's order so the digest is reproducible: the
    /// same change has to hash the same however git chose to lay it out.
    /// A path the diff does not carry contributes nothing rather than
    /// erroring, which is what a `language:` scope naming a language the
    /// diff no longer touches leaves behind.
    pub fn slice(&self, paths: &[String]) -> String {
        let wanted: BTreeSet<&String> = paths.iter().collect();
        let mut out = String::new();
        for (path, section) in &self.sections {
            if wanted.contains(path) {
                out.push_str(section);
            }
        }
        out
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
    EXTENSION_LANGUAGES
        .iter()
        .find_map(|(candidate, language)| (*candidate == extension).then_some(*language))
}

/// Keep a language label only when it occurs in the fixed extension table.
/// Unknown or bespoke labels collapse to `other` rather than identifying a
/// codebase.
pub fn telemetry_language(label: &str) -> &'static str {
    let label = label.to_ascii_lowercase();
    EXTENSION_LANGUAGES
        .iter()
        .find_map(|(_, language)| (*language == label).then_some(*language))
        .unwrap_or("other")
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

    /// The `---` header is not a removed line, or every diff would carry
    /// its own old file names into the regex matcher.
    #[test]
    fn the_removed_text_holds_the_removed_lines_only() {
        let diff = Diff::parse(SAMPLE);
        assert_eq!(diff.removed, "let x = 1;\n");
    }

    /// A removed content line that begins `-- ` (SQL/shell comments) is
    /// rendered as `--- …` in the unified diff. Headers must only be
    /// recognised outside hunks, or that line becomes a fake path.
    #[test]
    fn a_removed_line_starting_with_dashes_is_not_a_file_header() {
        let text = "\
diff --git a/q.sql b/q.sql
--- a/q.sql
+++ b/q.sql
@@ -1,2 +1,2 @@
--- comment
-select 1;
+select 2;
";
        let diff = Diff::parse(text);
        assert_eq!(diff.paths, ["q.sql"]);
        assert_eq!(diff.removed, "-- comment\nselect 1;\n");
        assert_eq!(diff.added, "select 2;\n");
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
    fn telemetry_language_labels_come_only_from_the_extension_table() {
        assert_eq!(telemetry_language("rust"), "rust");
        assert_eq!(telemetry_language("RuSt"), "rust");
        assert_eq!(telemetry_language("company-secret"), "other");
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

#[cfg(test)]
mod slice_tests {
    use super::*;

    const TWO_FILES: &str = "diff --git a/.github/workflows/deploy.yml b/.github/workflows/deploy.yml\n\
        --- a/.github/workflows/deploy.yml\n\
        +++ b/.github/workflows/deploy.yml\n\
        @@ -1,1 +1,1 @@\n\
        -uses: actions/checkout@v4\n\
        +uses: actions/checkout@v7\n\
        diff --git a/src/app.ts b/src/app.ts\n\
        --- a/src/app.ts\n\
        +++ b/src/app.ts\n\
        @@ -1,1 +1,1 @@\n\
        -const a = 1;\n\
        +const a = 2;\n";

    /// A slice is the hunks of the paths a rule's scopes selected, and
    /// nothing else. Spec 9.2, *The digest has to match the granularity
    /// of selection*.
    #[test]
    fn a_slice_carries_only_the_paths_it_was_asked_for() {
        let diff = Diff::parse(TWO_FILES);
        let slice = diff.slice(&[".github/workflows/deploy.yml".to_string()]);

        assert!(slice.contains("checkout@v7"), "{slice}");
        assert!(
            !slice.contains("const a"),
            "a slice must not carry an unscoped path: {slice}"
        );
    }

    /// Sorted-path order, so the digest is reproducible whatever order
    /// git named the files in.
    #[test]
    fn a_slice_is_ordered_by_path_not_by_git() {
        let diff = Diff::parse(TWO_FILES);
        let both = diff.slice(&[
            "src/app.ts".to_string(),
            ".github/workflows/deploy.yml".to_string(),
        ]);
        let reversed = diff.slice(&[
            ".github/workflows/deploy.yml".to_string(),
            "src/app.ts".to_string(),
        ]);

        assert_eq!(both, reversed);
        assert!(
            both.find(".github").unwrap() < both.find("src/app.ts").unwrap(),
            "{both}"
        );
    }

    /// The whole diff is a slice too, which is what a `global` scope asks
    /// for. It must equal the text git printed, not a rebuild of it.
    #[test]
    fn slicing_every_path_is_the_whole_diff() {
        let diff = Diff::parse(TWO_FILES);
        let slice = diff.slice(&diff.paths);
        for needle in ["checkout@v7", "const a = 2;"] {
            assert!(slice.contains(needle), "{needle} missing from {slice}");
        }
    }

    /// The measured defect: an edit in a path no rule scopes must leave
    /// that rule's slice alone.
    #[test]
    fn an_edit_outside_the_slice_does_not_change_it() {
        let before = Diff::parse(TWO_FILES);
        let after = Diff::parse(&TWO_FILES.replace("const a = 2;", "const a = 3;"));
        let scoped = [".github/workflows/deploy.yml".to_string()];

        assert_ne!(before.text, after.text, "the diffs do differ");
        assert_eq!(before.slice(&scoped), after.slice(&scoped));
    }

    /// And an edit inside it must change it, or the gate never returns.
    #[test]
    fn an_edit_inside_the_slice_changes_it() {
        let before = Diff::parse(TWO_FILES);
        let after = Diff::parse(&TWO_FILES.replace("checkout@v7", "checkout@v6"));
        let scoped = [".github/workflows/deploy.yml".to_string()];

        assert_ne!(before.slice(&scoped), after.slice(&scoped));
    }

    /// A content line reading `--- ` inside a hunk is not a file header.
    #[test]
    fn a_dashed_content_line_does_not_split_a_slice() {
        let text = "diff --git a/one.md b/one.md\n--- a/one.md\n+++ b/one.md\n\
                    @@ -1,2 +1,2 @@\n--- old bullet\n+-- new bullet\n";
        let diff = Diff::parse(text);
        assert_eq!(diff.paths, ["one.md"]);
        assert!(diff.slice(&["one.md".to_string()]).contains("new bullet"));
    }
}
