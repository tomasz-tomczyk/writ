//! Turning what a user typed into an FTS5 query.
//!
//! `MATCH` takes a query language, not a string. `-`, `"`, `*`, `OR` and
//! `NEAR` are operators there, so passing raw input either raises a syntax
//! error or quietly searches for something else. Spec section 7.3 records
//! this as one of the two silent traps, and it is what `writ list --search`
//! has to survive.
//!
//! The fix is to tokenize. Everything that is not a letter or a digit
//! separates two tokens, and each token goes into the query as a quoted
//! phrase. A quoted `OR` is the word "or", not the operator.

/// Split input into the terms FTS5 should look for.
///
/// A term is a run of alphanumeric characters. Every other character is a
/// separator, which is what makes the operator characters harmless.
fn terms(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

/// Quote one term as an FTS5 phrase.
///
/// A term holds no quote character by construction. The escape stays so
/// that a future change to [`terms`] cannot silently open an injection.
fn quote(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

/// A query that requires every term. This is `--search`.
///
/// Returns `None` when the input holds no searchable term, which is not an
/// error: `writ list --search '***'` matches nothing.
pub fn match_all(input: &str) -> Option<String> {
    let terms = terms(input);
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|term| quote(term))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_become_quoted_phrases() {
        assert_eq!(match_all("prefer sd").unwrap(), r#""prefer" AND "sd""#);
    }

    #[test]
    fn a_hyphen_separates_rather_than_negates() {
        // Raw, `non-empty` asks FTS5 for "non" NOT "empty".
        assert_eq!(match_all("non-empty").unwrap(), r#""non" AND "empty""#);
    }

    #[test]
    fn a_quote_cannot_close_the_phrase() {
        assert_eq!(match_all(r#"say "hi""#).unwrap(), r#""say" AND "hi""#);
    }

    #[test]
    fn a_star_is_not_a_prefix_operator() {
        assert_eq!(match_all("sd*").unwrap(), r#""sd""#);
    }

    #[test]
    fn or_and_near_are_words_not_operators() {
        assert_eq!(match_all("OR").unwrap(), r#""OR""#);
        assert_eq!(match_all("NEAR").unwrap(), r#""NEAR""#);
        assert_eq!(
            match_all("a NEAR b").unwrap(),
            r#""a" AND "NEAR" AND "b""#,
            "NEAR must be searched for, not obeyed"
        );
    }

    #[test]
    fn input_with_no_term_has_no_query() {
        assert_eq!(match_all("***"), None);
        assert_eq!(match_all(" - \" "), None);
        assert_eq!(match_all(""), None);
    }
}
