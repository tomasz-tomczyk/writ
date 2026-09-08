//! `writ record --json`: many learnings, one stream.
//!
//! Parsing a string is pure, so the format lives here. Reading stdin is
//! `writ-cli`'s job. See invariant 1.
//!
//! **The whole stream parses before anything is written.** The spec does
//! not say what a bulk write does with one bad line among good ones. It
//! says bad JSON has its own exit code (section 5.7) and, through crit
//! #446, that no write is a silent partial. A stream that wrote nine rows
//! and then failed would leave the caller with no way to know which nine,
//! and a re-run would duplicate them. All or nothing is re-runnable.

use crate::error::{Error, Result};
use crate::model::{NewLearning, SourceKind};

/// Parse a JSONL stream into learnings, or fail naming the line.
///
/// A blank line is skipped: a file that ends with a newline is normal.
/// Every learning defaults to `source_kind = import`, which is what
/// section 7.4 relies on to tell a shared rule from a typed one.
pub fn parse_jsonl(text: &str) -> Result<Vec<NewLearning>> {
    let mut learnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let mut learning: NewLearning =
            serde_json::from_str(line).map_err(|error| Error::BadJson {
                line: number,
                message: error.to_string(),
            })?;
        learning.source_kind = learning.source_kind.or(Some(SourceKind::Import));
        learning.validate().map_err(|error| match error {
            Error::Validation { message } => Error::validation(format!("line {number}: {message}")),
            other => other,
        })?;
        learnings.push(learning);
    }
    if learnings.is_empty() {
        return Err(Error::validation("the JSON stream holds no learning"));
    }
    Ok(learnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExemplarKind, Status};

    const ONE: &str = r#"{"title":"t","rule":"r","rationale":"why"}"#;

    #[test]
    fn a_minimal_line_parses() {
        let learnings = parse_jsonl(ONE).unwrap();
        assert_eq!(learnings.len(), 1);
        assert_eq!(learnings[0].rule, "r");
        assert_eq!(learnings[0].source_kind, Some(SourceKind::Import));
        assert!(learnings[0].blocking, "blocking is the default");
        assert_eq!(learnings[0].sides, crate::model::Sides::Both);
        assert_eq!(learnings[0].effective_status(), Status::Proposed);
    }

    #[test]
    fn every_portable_field_parses() {
        // Two narrowing kinds, not `global` beside one. Section 7.1 step 2
        // ANDs the kinds, so `global` cannot be combined with another.
        let line = r#"{"title":"t","rule":"r","rationale":"why",
          "scopes":["project:github.com/o/r","language:rust"],"blocking":false,
          "sides":"added",
          "matcher":"$A","matcher_kind":"ast_grep","author":"dev@example.com",
          "exemplars":[{"kind":"bad","snippet":"let x = 1;","language":"rust"}]}"#
            .replace('\n', " ");
        let learning = &parse_jsonl(&line).unwrap()[0];
        assert_eq!(
            learning.scopes,
            vec![
                "project:github.com/o/r".parse().unwrap(),
                "language:rust".parse().unwrap()
            ]
        );
        assert!(!learning.blocking);
        assert_eq!(learning.sides, crate::model::Sides::Added);
        assert_eq!(learning.exemplars[0].kind, ExemplarKind::Bad);
        assert_eq!(learning.exemplars[0].snippet, "let x = 1;");
    }

    #[test]
    fn blank_lines_are_skipped() {
        let text = format!("{ONE}\n\n{ONE}\n");
        assert_eq!(parse_jsonl(&text).unwrap().len(), 2);
    }

    #[test]
    fn one_bad_line_fails_the_whole_stream_and_names_it() {
        let text = format!("{ONE}\nnot json\n{ONE}\n");
        let error = parse_jsonl(&text).unwrap_err();
        assert!(matches!(error, Error::BadJson { line: 2, .. }), "{error}");
    }

    /// A pack authored under the old OR semantics carries `global` beside
    /// another kind. Importing it must fail loudly rather than keeping
    /// half of what it says. Section 7.4 imports through this path.
    #[test]
    fn an_imported_global_plus_narrow_scope_is_refused() {
        let line = r#"{"title":"t","rule":"r","rationale":"why",
          "scopes":["global","project:github.com/o/r"]}"#
            .replace('\n', " ");
        let error = parse_jsonl(&line).unwrap_err();
        assert!(error.to_string().contains("cannot be combined"), "{error}");
    }

    #[test]
    fn a_line_missing_its_rationale_is_a_validation_error_not_bad_json() {
        // The line parses. It is the content the rules refuse, and the two
        // causes carry different exit codes. See spec section 5.7.
        let text = r#"{"title":"t","rule":"r"}"#;
        let error = parse_jsonl(text).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
        assert!(error.to_string().contains("rationale"), "{error}");
    }

    #[test]
    fn an_empty_rationale_is_a_validation_error() {
        let text = r#"{"title":"t","rule":"r","rationale":"  "}"#;
        let error = parse_jsonl(text).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
        assert!(error.to_string().contains("line 1"), "{error}");
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_dropped() {
        let text = r#"{"title":"t","rule":"r","rationale":"why","id":"x"}"#;
        let error = parse_jsonl(text).unwrap_err();
        assert!(matches!(error, Error::BadJson { line: 1, .. }), "{error}");
    }

    #[test]
    fn an_empty_stream_says_so() {
        let error = parse_jsonl("\n\n").unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
    }
}
