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

/// The commit a range's removed lines come from.
///
/// `git diff A` compares A with the working tree and `git diff A..B`
/// compares A with B, so both start at A. `git diff A...B` starts at the
/// merge-base of A and B. An empty side means HEAD, as it does to git.
pub fn range_base(root: &Path, range: &str) -> String {
    // The index is compared with HEAD.
    if range == CACHED {
        return "HEAD".to_string();
    }
    let or_head = |side: &str| {
        if side.is_empty() {
            "HEAD".to_string()
        } else {
            side.to_string()
        }
    };
    if let Some((left, right)) = range.split_once("...") {
        let (left, right) = (or_head(left), or_head(right));
        return run(root, &["merge-base", &left, &right]).unwrap_or(left);
    }
    if let Some((left, _)) = range.split_once("..") {
        return or_head(left);
    }
    range.to_string()
}

/// The commit HEAD names, or nothing in a repository with no commit.
pub fn head(root: &Path) -> Option<String> {
    run(root, &["rev-parse", "--verify", "HEAD"])
}

/// What a staged audit records as its range, and what its slice commands
/// pass to `git diff`, so they print the same diff.
pub const CACHED: &str = "--cached";

/// Read the staged diff: the index against HEAD, which is what a commit is
/// about to record. A repository with no commit yet is compared with the
/// empty tree, which is what `git diff --cached` does by itself.
///
/// A hook git runs during `git commit -a` or `git commit PATH` sees a
/// temporary index through `GIT_INDEX_FILE`. git reads that variable
/// itself, so this reads the index the commit will actually use.
pub fn diff_cached(root: &Path) -> Result<(String, String)> {
    let output = Command::new("git")
        .args(["diff", CACHED])
        .current_dir(root)
        .output()
        .map_err(|error| Error::Command {
            program: "git".into(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(Error::Validation {
            message: format!(
                "git diff --cached failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        CACHED.to_string(),
    ))
}

/// What `git diff` compares with when no range is named: HEAD, or the
/// empty tree in a repository with no commit yet.
pub fn default_base(root: &Path) -> String {
    if run(root, &["rev-parse", "--verify", "HEAD"]).is_some() {
        "HEAD".to_string()
    } else {
        EMPTY_TREE.to_string()
    }
}

/// A tree holding the working tree as it is now: tracked changes, deleted
/// files, and new files git does not ignore. Spec section 9.2, **An audit
/// records what it reviewed**.
///
/// It is built in a copy of the index, so the real index — what the
/// developer has staged — is never touched. Copying rather than starting
/// empty keeps git's record of which files are unchanged, so only the
/// files that changed are read. The objects it writes are the ones
/// `git add` would write, and nothing refers to them but the audit.
///
/// Nothing when any step fails, and the caller reads the diff without it.
/// P6.
pub fn snapshot(root: &Path) -> Option<String> {
    let index = std::env::var_os("GIT_INDEX_FILE")
        .map(PathBuf::from)
        .or_else(|| run(root, &["rev-parse", "--git-path", "index"]).map(|path| root.join(path)))?;
    let scratch = root.join(run(
        root,
        &[
            "rev-parse",
            "--git-path",
            &format!("writ-snapshot-{}.index", std::process::id()),
        ],
    )?);
    // A repository with no commit and nothing staged has no index yet.
    if index.is_file() {
        std::fs::copy(&index, &scratch).ok()?;
    }
    let in_scratch = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_INDEX_FILE", &scratch)
            .output()
            .ok()
            .filter(|output| output.status.success())
    };
    let tree = in_scratch(&["add", "-A"])
        .and_then(|_| in_scratch(&["write-tree"]))
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|tree| !tree.is_empty());
    let _ = std::fs::remove_file(&scratch);
    tree
}

/// The diff from `base` to a tree: what a working-tree audit reads once it
/// has a snapshot. For tracked files it is the same text `git diff BASE`
/// prints; it also carries the new files.
pub fn diff_to_tree(root: &Path, base: &str, tree: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", base, tree])
        .current_dir(root)
        .output()
        .map_err(|error| Error::Command {
            program: "git".into(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(Error::Validation {
            message: format!(
                "git diff {base} {tree} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The paths that differ between two trees. Nothing when git cannot read
/// one of them — a tree collected as garbage, say. P6.
pub fn changed_between(root: &Path, from: &str, to: &str) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", from, to])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// Whether `ancestor` is `descendant` or in its history.
pub fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The commit a revision names.
pub fn resolve(root: &Path, revision: &str) -> Option<String> {
    run(
        root,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )
}

/// The tree the index holds: what a commit made now would record.
///
/// `git write-tree` stores the tree object, which is what `git commit`
/// does next anyway, and changes nothing else.
pub fn index_tree(root: &Path) -> Option<String> {
    run(root, &["write-tree"])
}

/// How far back `--since-answer` looks for an answered commit before it
/// falls back to the branch point.
pub const HISTORY_DEPTH: usize = 500;

/// HEAD's first-parent history, newest first, as commit and tree pairs.
///
/// First-parent, so a merge of main into the branch does not walk into
/// main's history and find someone else's answer there.
pub fn first_parent_history(root: &Path, depth: usize) -> Vec<(String, String)> {
    let depth = format!("-n{depth}");
    run(
        root,
        &["log", "--first-parent", &depth, "--format=%H %T", "HEAD"],
    )
    .map(|text| {
        text.lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(commit, tree)| (commit.to_string(), tree.to_string()))
            .collect()
    })
    .unwrap_or_default()
}

/// Where this branch left the remote's default branch.
///
/// The remote's `HEAD` first, then `origin/main`, then `origin/master`.
/// Nothing when there is no such ref or no common history, and the caller
/// then reads the working tree against HEAD. P6.
pub fn branch_point(root: &Path) -> Option<String> {
    let remote_head = run(
        root,
        &["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"],
    );
    remote_head
        .into_iter()
        .chain(["origin/main".to_string(), "origin/master".to_string()])
        .find_map(|candidate| run(root, &["merge-base", "HEAD", &candidate]))
}

/// Raw bytes of a git object (`HEAD:path`, a blob oid, …).
///
/// Unlike [`run`], this keeps the body untrimmed and allows non-UTF-8.
/// Used by retrieval matchers for pre-image scans. Failures return the
/// reason so the caller can degrade under P6.
pub fn show_bytes(root: &Path, object: &str) -> std::result::Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(["show", object])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
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
