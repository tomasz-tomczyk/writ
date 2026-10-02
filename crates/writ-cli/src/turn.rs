//! `writ turn --hook claude-code`. Spec section 9.2, **The Stop gate
//! audits only what the turn changed**.
//!
//! Stop fires in whatever repository the session's directory is in, and
//! a Bash `cd` moves that directory. So a session that only looked at a
//! worktree another session had left dirty was sent to review that work.
//! This records where each turn found each repository, and the gate
//! passes a repository whose tree the turn did not change.
//!
//! Two host events reach it:
//!
//! - `UserPromptSubmit` starts the turn, and records the repository the
//!   session is in;
//! - `PreToolUse` on a tool that can write records a repository the turn
//!   reaches later, before the tool runs. An edit names its file, and a
//!   shell command runs in the session's directory.
//!
//! It prints nothing on stdout: Claude Code adds a `UserPromptSubmit`
//! hook's stdout to the prompt.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use writ_core::{Result, Store, TelemetryBatch, TurnStart};

use crate::{git, hook};

/// Every flag of `writ turn`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The host whose hook payload is on stdin: claude-code
    #[arg(long, value_name = "HOST")]
    pub hook: TurnHost,
}

/// The hosts whose turns writ records. Only Claude Code sends a session
/// id with both events; without the record the gate behaves as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TurnHost {
    /// `UserPromptSubmit` and `PreToolUse` hooks.
    ClaudeCode,
}

/// Record what the host's event says about the turn.
///
/// A payload with no session names no turn, so it records nothing. The
/// gate then has no turn for the session and audits as before, which is
/// the safe direction.
pub fn run(args: &Args, db: &Path) -> Result<(ExitCode, TelemetryBatch)> {
    let TurnHost::ClaudeCode = args.hook;
    let payload = hook::read_payload();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else {
        return Ok((ExitCode::SUCCESS, TelemetryBatch::default()));
    };
    let Some(session) = value.get("session_id").and_then(serde_json::Value::as_str) else {
        return Ok((ExitCode::SUCCESS, TelemetryBatch::default()));
    };
    match value
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
    {
        Some("UserPromptSubmit") => {
            let start = current_dir()
                .and_then(|cwd| git::discover(&cwd).ok())
                .map(|repo| (root_key(&repo.root), git::snapshot(&repo.root)));
            Store::open(db)?.start_turn(
                session,
                start
                    .as_ref()
                    .map(|(root, tree)| (root.as_str(), tree.as_deref())),
            )?;
        }
        Some("PreToolUse") => {
            let place = tool_path(&value).or_else(current_dir);
            if let Some(repo) = place.and_then(|place| git::discover(&place).ok()) {
                let root = root_key(&repo.root);
                let store = Store::open(db)?;
                // The snapshot is the cost, so it is taken only for a
                // repository the turn has not reached yet.
                if store.turn_start(session, &root)? == TurnStart::Untouched {
                    store.reach_in_turn(session, &root, git::snapshot(&repo.root).as_deref())?;
                }
            }
        }
        _ => {}
    }
    Ok((ExitCode::SUCCESS, TelemetryBatch::default()))
}

/// Why the gate has nothing to audit in this repository this turn, or
/// nothing when it cannot tell.
///
/// It cannot tell when the payload names no session, the host recorded no
/// turn for it, or git could not snapshot the tree at either end. Each of
/// those audits as before.
pub fn unchanged(payload: &str, db: &Path) -> Result<Option<&'static str>> {
    let Some(session) = serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| value.get("session_id")?.as_str().map(str::to_string))
    else {
        return Ok(None);
    };
    let Some(repo) = current_dir().and_then(|cwd| git::discover(&cwd).ok()) else {
        return Ok(None);
    };
    let reason = "this turn changed nothing in this repository";
    Ok(
        match Store::open(db)?.turn_start(&session, &root_key(&repo.root))? {
            TurnStart::Unknown | TurnStart::At(None) => None,
            TurnStart::Untouched => Some(reason),
            TurnStart::At(Some(start)) => {
                (git::snapshot(&repo.root).as_deref() == Some(start.as_str())).then_some(reason)
            }
        },
    )
}

/// The key a repository's row is stored under: its checkout, not its
/// identity. Two worktrees of one repository are two working trees.
fn root_key(root: &Path) -> String {
    root.display().to_string()
}

fn current_dir() -> Option<PathBuf> {
    std::env::current_dir().ok()
}

/// The nearest existing directory to the file an edit tool names. A
/// `Write` may create the file and the directories above it.
fn tool_path(value: &serde_json::Value) -> Option<PathBuf> {
    let input = value.get("tool_input")?;
    let file = ["file_path", "notebook_path"]
        .iter()
        .find_map(|key| input.get(*key)?.as_str())?;
    let mut dir = Path::new(file).parent()?;
    while !dir.is_dir() {
        dir = dir.parent()?;
    }
    Some(dir.to_path_buf())
}
