//! The `glob:` scope. Spec section 7.1 step 2.
//!
//! Section 7.1 gives `glob:` the job `project:` cannot do: separating
//! `api/` from `web/` inside one monorepo. That needs `**`, so a plain
//! substring test is not enough.
//!
//! The dialect is used both in SQL, through the `writ_glob_any` scalar
//! function, and in the Rust `scope_hits` second pass. `?` is one
//! character except `/`, `*` is any run of characters except `/`, and
//! `**` as a whole segment is any run of segments including none. A
//! pattern with no `/` matches the basename, which is what makes
//! `glob:*.rs` mean what a reader expects.

/// Whether `path` matches `pattern`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let path = path.trim_start_matches("./");
    if !pattern.contains('/') {
        let base = path.rsplit('/').next().unwrap_or(path);
        return match_segments(&split(pattern), &split(base));
    }
    match_segments(&split(pattern), &split(path))
}

fn split(text: &str) -> Vec<&str> {
    text.split('/').filter(|part| !part.is_empty()).collect()
}

/// Match segment lists, letting a `**` segment stand for any number of
/// segments including none.
fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            // Zero segments first, so `**/*.rs` matches a file at the root.
            for taken in 0..=path.len() {
                if match_segments(rest, &path[taken..]) {
                    return true;
                }
            }
            false
        }
        Some((head, rest)) => match path.split_first() {
            Some((first, tail)) if match_one(head, first) => match_segments(rest, tail),
            _ => false,
        },
    }
}

/// Match one segment, where `*` is any run and `?` is one character.
fn match_one(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    // The last `*` seen, and where the text stood then, so a failed tail
    // backtracks without recursion.
    let (mut star, mut resume) = (None, 0);

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            resume = t;
            p += 1;
        } else if let Some(index) = star {
            p = index + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_stops_at_a_slash() {
        assert!(glob_match("api/*.ex", "api/a.ex"));
        assert!(!glob_match("api/*.ex", "api/lib/a.ex"));
    }

    /// `**/*.rs` has to match a file at the root, or every rule written
    /// that way silently misses the top level.
    #[test]
    fn a_double_star_matches_no_directory_at_all() {
        assert!(glob_match("**/*.rs", "a.rs"));
        assert!(glob_match("**/*.rs", "src/a.rs"));
        assert!(glob_match("**/*.rs", "src/deep/a.rs"));
        assert!(!glob_match("**/*.rs", "src/a.ex"));
    }

    /// This is the monorepo case from the section 7.1 table.
    #[test]
    fn a_prefix_separates_one_monorepo_subtree_from_another() {
        assert!(glob_match("api/**", "api/lib/vetspire.ex"));
        assert!(!glob_match("api/**", "web/src/app.tsx"));
    }

    #[test]
    fn a_pattern_with_no_slash_matches_the_basename() {
        assert!(glob_match("*.rs", "crates/writ-core/src/lib.rs"));
        assert!(!glob_match("*.rs", "crates/writ-core/src/lib.ex"));
    }

    #[test]
    fn a_question_mark_is_one_character() {
        assert!(glob_match("a?.rs", "ab.rs"));
        assert!(!glob_match("a?.rs", "abc.rs"));
    }

    #[test]
    fn a_leading_dot_slash_is_not_a_segment() {
        assert!(glob_match("api/**", "./api/a.ex"));
    }
}
