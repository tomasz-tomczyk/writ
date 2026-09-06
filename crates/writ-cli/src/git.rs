//! Everything the audit needs from git.
//!
//! Running a program is I/O, so it lives here. The strings git prints go
//! straight to `writ-core`, which decides what they mean. Invariant 1.

use std::path::{Path, PathBuf};
use std::process::Command;

use writ_core::{Error, RepoIdentity, Result, normalize_remote};

/// The empty tree. `git diff HEAD` needs a HEAD, and a repository with no
/// commit yet has none, so the first diff is taken against this instead.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The repository the audit is running in.
pub struct Repo {
    /// Where to run git, and where a changed path is rooted.
    pub root: PathBuf,
    /// What a `project:` scope is compared against. Invariant 4.
    pub identity: RepoIdentity,
}

/// Find the repository around `cwd`, and name it.
///
/// The identity comes from the remote. When there is none, the fallback is
/// the **main checkout** path, taken from the common git directory rather
/// than from this worktree's own root. A worktree therefore keeps the
/// identity of its main checkout even with no remote at all, which is the
/// worktree row of the section 7.1 table.
pub fn discover(cwd: &Path) -> Result<Repo> {
    let root =
        run(cwd, &["rev-parse", "--show-toplevel"]).ok_or_else(|| Error::NotAGitRepository {
            path: cwd.to_path_buf(),
        })?;
    let root = PathBuf::from(root);

    let identity = match remote_url(&root).as_deref().and_then(normalize_remote) {
        Some(identity) => RepoIdentity::Remote(identity),
        None => RepoIdentity::Path(main_checkout(&root)),
    };

    Ok(Repo { root, identity })
}

/// The main checkout's path, so every worktree of one repository answers
/// the same string.
///
/// `git worktree list` names the main worktree first, whichever worktree
/// it is run from. That is what makes the fallback identity survive
/// `git worktree add`, which `rev-parse --show-toplevel` would not.
fn main_checkout(root: &Path) -> String {
    let listed = run(root, &["worktree", "list", "--porcelain"])
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("worktree ").map(str::to_string))
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    std::fs::canonicalize(&listed)
        .unwrap_or(listed)
        .display()
        .to_string()
}

/// The URL of `origin`, or of the first remote when there is no `origin`.
fn remote_url(root: &Path) -> Option<String> {
    if let Some(url) = run(root, &["remote", "get-url", "origin"]) {
        return Some(url);
    }
    let names = run(root, &["remote"])?;
    let first = names.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    run(root, &["remote", "get-url", first])
}

/// Read the diff, and say what range it came from.
///
/// With no `--diff` the range is the working tree against HEAD, which is
/// what a Stop hook wants: the change the agent just made, committed or
/// not.
pub fn diff(root: &Path, range: Option<&str>) -> Result<(String, String)> {
    let range = match range {
        Some(range) => range.to_string(),
        None if run(root, &["rev-parse", "--verify", "HEAD"]).is_some() => "HEAD".to_string(),
        None => EMPTY_TREE.to_string(),
    };
    let output = Command::new("git")
        .args(["diff", &range])
        .current_dir(root)
        .output()
        .map_err(|error| Error::Command {
            program: "git".into(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(Error::Validation {
            message: format!(
                "git diff {range} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok((String::from_utf8_lossy(&output.stdout).into_owned(), range))
}

/// Run git and return its trimmed stdout, or nothing when it refused.
///
/// A refusal is an answer here, not a failure: no remote, no HEAD and no
/// repository are all reported by an exit code, and each caller above
/// turns that into its own outcome.
fn run(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}
