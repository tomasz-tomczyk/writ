//! Repository identity, as spec section 7.1 defines it.
//!
//! Identity is the normalized remote URL, never the directory path. A
//! worktree is the same repository at a different path, so a path-based
//! identity would make `project:` scopes fire in the main checkout and
//! silently miss in every worktree. Nothing would error. The audit would
//! just check fewer rules. Invariant 4.
//!
//! Running `git remote` is I/O and lives in `writ-cli`. Normalizing the
//! string it prints is pure and lives here, where the cases in the section
//! 7.1 table are tested without building fixture repositories. Invariant 1.

/// What an audit calls this repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoIdentity {
    /// The normalized remote. This is the identity a `project:` scope holds.
    Remote(String),
    /// No remote answered, so the main checkout's path stands in.
    ///
    /// Section 7.1 requires the audit to say so, because a rule recorded
    /// against the remote identity will not match this one.
    Path(String),
}

impl RepoIdentity {
    /// The string a `project:` scope is compared against.
    pub fn value(&self) -> &str {
        match self {
            Self::Remote(value) | Self::Path(value) => value,
        }
    }

    /// Whether this is the path fallback rather than a remote.
    pub fn is_fallback(&self) -> bool {
        matches!(self, Self::Path(_))
    }
}

/// Normalize a git remote URL into a repository identity.
///
/// Host, then the path, with the scheme, the user, a port, a trailing
/// slash and a trailing `.git` removed, and the whole result lowercased.
/// `git@github.com:Vetspire-VSP/Vetspire.git` and
/// `https://github.com/vetspire-vsp/vetspire` are therefore one identity.
///
/// Returns `None` when the string names no host: a bare filesystem path
/// and a `file://` URL are local, and the caller falls back to the main
/// checkout path instead.
pub fn normalize_remote(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    let rest = match url.find("://") {
        Some(index) => {
            if url[..index].eq_ignore_ascii_case("file") {
                return None;
            }
            &url[index + 3..]
        }
        // Without a scheme this must be the scp form, `host:path`. A
        // string with no colon before its first slash is a local path.
        None => {
            let colon = url.find(':')?;
            let slash = url.find('/').unwrap_or(url.len());
            if colon > slash {
                return None;
            }
            url
        }
    };

    // Only the last `@` separates a user, so a password holding one does
    // not truncate the host.
    let rest = match rest.rfind('@') {
        Some(index) => &rest[index + 1..],
        None => rest,
    };

    let separator = rest.find(['/', ':'])?;
    let host = &rest[..separator];
    let mut path = &rest[separator + 1..];

    // `ssh://host:22/owner/repo` carries a port. The scp form cannot, so
    // a numeric first segment is only a port when a colon introduced it.
    if rest.as_bytes()[separator] == b':' {
        let segment_end = path.find('/').unwrap_or(path.len());
        let segment = &path[..segment_end];
        if segment_end < path.len()
            && !segment.is_empty()
            && segment.bytes().all(|byte| byte.is_ascii_digit())
        {
            path = &path[segment_end + 1..];
        }
    }

    if host.is_empty() {
        return None;
    }

    let path = path.strip_suffix('/').unwrap_or(path);
    let path = strip_git_suffix(path);

    let segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_lowercase())
        .collect();
    if segments.is_empty() {
        return None;
    }

    Some(format!("{}/{}", host.to_lowercase(), segments.join("/")))
}

/// Remove a trailing `.git`, whatever its case.
fn strip_git_suffix(path: &str) -> &str {
    if path.len() >= 4 && path[path.len() - 4..].eq_ignore_ascii_case(".git") {
        &path[..path.len() - 4]
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four forms the task names must land on one identity, because a
    /// team writes the same remote four ways.
    #[test]
    fn every_spelling_of_one_remote_is_one_identity() {
        for url in [
            "git@github.com:Owner/Repo.git",
            "https://github.com/Owner/Repo.git",
            "https://user@github.com/owner/repo",
            "https://github.com/Owner/Repo/",
            "ssh://git@github.com:22/Owner/Repo.git",
            "git://github.com/owner/repo.git",
            "  https://github.com/OWNER/REPO.GIT  ",
            "github.com:Owner/Repo",
        ] {
            assert_eq!(
                normalize_remote(url).as_deref(),
                Some("github.com/owner/repo"),
                "{url}"
            );
        }
    }

    /// A nested GitLab group is part of the name. Keeping only the last
    /// two segments would merge `group/a/tool` and `group/b/tool`.
    #[test]
    fn a_nested_group_stays_in_the_identity() {
        assert_eq!(
            normalize_remote("git@gitlab.com:group/sub/tool.git").as_deref(),
            Some("gitlab.com/group/sub/tool")
        );
    }

    #[test]
    fn the_host_is_lowercased_too() {
        assert_eq!(
            normalize_remote("https://GitHub.COM/o/r").as_deref(),
            Some("github.com/o/r")
        );
    }

    /// A local remote names no host, so there is no identity to share and
    /// the caller falls back to the checkout path.
    #[test]
    fn a_local_remote_has_no_identity() {
        for url in [
            "/srv/git/repo.git",
            "../sibling",
            "file:///srv/git/repo.git",
            "",
            "   ",
            "https://github.com/",
        ] {
            assert_eq!(normalize_remote(url), None, "{url}");
        }
    }

    #[test]
    fn a_dot_git_inside_a_name_survives() {
        assert_eq!(
            normalize_remote("git@github.com:o/dot.github.git").as_deref(),
            Some("github.com/o/dot.github")
        );
    }

    #[test]
    fn the_fallback_says_it_is_a_fallback() {
        assert!(RepoIdentity::Path("/tmp/x".into()).is_fallback());
        assert!(!RepoIdentity::Remote("github.com/o/r".into()).is_fallback());
        assert_eq!(
            RepoIdentity::Remote("github.com/o/r".into()).value(),
            "github.com/o/r"
        );
    }
}
