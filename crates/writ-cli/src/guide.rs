//! `writ install` with no host: one guided setup. Spec section 5,
//! `writ install`.
//!
//! It finds the coding agents on this machine and the git commit gate, and
//! for each one says what it would change and where, then asks. Nothing is
//! written without a yes. `writ install HOST` still writes one host
//! without asking, for scripts and for documentation that names a host.
//!
//! It asks on a terminal and nowhere else. Without one it needs `--yes`,
//! and it never waits on a stdin nobody will write to. P7.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

use writ_core::{Error, Result};

use crate::install::{Host, Outcome, Role, Roots, execute, plan, plugin_gates};

/// The oldest git with hooks defined in config (`hook.<name>.command`).
/// Before it, a hook is a file in `.git/hooks`, one repository at a time.
pub const GIT_HOOKS_SINCE: (u32, u32) = (2, 54);

/// What the git commit gate writes into git's config.
pub const GIT_HOOK_COMMAND: &str = "writ audit --hook git";
/// The event it runs on.
pub const GIT_HOOK_EVENT: &str = "pre-commit";

/// Where git keeps the config the gate goes into.
#[derive(Debug, Clone)]
pub enum GitConfig {
    /// `git config --global`: every repository on the machine.
    Global,
    /// One file, for tests.
    File(PathBuf),
}

impl GitConfig {
    fn scope(&self) -> Vec<String> {
        match self {
            Self::Global => vec!["--global".to_string()],
            Self::File(path) => vec!["--file".to_string(), path.display().to_string()],
        }
    }

    fn get_all(&self, key: &str) -> Vec<String> {
        let mut args = vec!["config".to_string()];
        args.extend(self.scope());
        args.extend(["--get-all".to_string(), key.to_string()]);
        Command::new("git")
            .args(&args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut args = vec!["config".to_string()];
        args.extend(self.scope());
        args.extend([
            "--replace-all".to_string(),
            key.to_string(),
            value.to_string(),
        ]);
        let output = Command::new("git")
            .args(&args)
            .output()
            .map_err(|error| Error::Command {
                program: "git".into(),
                message: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::Validation {
                message: format!(
                    "git config {key} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(())
    }

    /// The undo command, as the reader would type it.
    fn undo(&self) -> String {
        format!(
            "git config {} --remove-section hook.writ",
            self.scope().join(" ")
        )
    }
}

/// The machine a guided install looks at.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Where host configuration lives.
    pub roots: Roots,
    /// `git --version`, as major and minor. `None` when git is missing or
    /// prints something unreadable.
    pub git_version: Option<(u32, u32)>,
    /// Where the git gate is written.
    pub git_config: GitConfig,
}

impl Machine {
    /// This machine: `$HOME`, the git on `PATH`, and the global config.
    pub fn here(roots: Roots) -> Self {
        let git_version = Command::new("git")
            .arg("--version")
            .output()
            .ok()
            .and_then(|output| parse_git_version(&String::from_utf8_lossy(&output.stdout)));
        Self {
            roots,
            git_version,
            git_config: GitConfig::Global,
        }
    }
}

/// Read `git version 2.55.0` or `git version 2.50.1 (Apple Git-155)`.
pub fn parse_git_version(text: &str) -> Option<(u32, u32)> {
    let number = text.trim().strip_prefix("git version ")?;
    let mut parts = number.split(|c: char| c == '.' || c.is_whitespace());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// The hosts whose configuration directory exists under `home`.
fn hosts_present(roots: &Roots) -> Vec<Host> {
    let Some(home) = &roots.home else {
        return Vec::new();
    };
    [
        (Host::ClaudeCode, home.join(".claude")),
        (Host::Codex, home.join(".codex")),
        (Host::Cursor, home.join(".cursor")),
        (Host::Opencode, home.join(".config").join("opencode")),
    ]
    .into_iter()
    .filter(|(_, dir)| dir.is_dir())
    .map(|(host, _)| host)
    .collect()
}

fn title(host: Host) -> &'static str {
    match host {
        Host::ClaudeCode => "Claude Code",
        Host::Codex => "Codex",
        Host::Cursor => "Cursor",
        Host::Opencode => "OpenCode",
    }
}

/// Run the guided install. `ask` puts one yes-or-no question to the
/// reader; everything else goes to `out`.
pub fn guide(
    machine: &Machine,
    ask: &mut dyn FnMut(&str, &mut String) -> bool,
    out: &mut String,
) -> Result<()> {
    let _ = writeln!(out, "writ install\n");
    let _ = writeln!(
        out,
        "writ connects to your coding agents and checks their work against your \
         learnings. For each change below it says what it would write and \
         where, then asks. Nothing is written without a yes.\n"
    );

    let hosts = hosts_present(&machine.roots);
    if hosts.is_empty() {
        let _ = writeln!(
            out,
            "No coding agent found in your home directory (~/.claude, ~/.codex, \
             ~/.cursor, ~/.config/opencode).\n"
        );
    }
    for host in hosts {
        host_step(machine, host, ask, out)?;
    }
    git_step(machine, ask, out)?;

    let _ = writeln!(
        out,
        "Restart your agent sessions, or reconnect the writ MCP server, so they \
         load the new setup."
    );
    Ok(())
}

fn host_step(
    machine: &Machine,
    host: Host,
    ask: &mut dyn FnMut(&str, &mut String) -> bool,
    out: &mut String,
) -> Result<()> {
    let _ = writeln!(out, "{}", title(host));
    let mut full = plan(host, false, &machine.roots)?;

    // The plugin carries its own gate. Writing a second copy would run the
    // gate twice per turn, so that step is left out rather than forced.
    if host == Host::ClaudeCode && plugin_gates(&full) {
        full.steps.retain(|step| step.role != Role::Gate);
        let _ = writeln!(
            out,
            "  gate: the writ@writ plugin provides it. Nothing to write."
        );
    }
    if let Some(reason) = full.no_gate {
        let _ = writeln!(out, "  gate: this host cannot enforce one. {reason}");
    }

    // A forced preview shows the difference from what writ writes today,
    // including an older writ entry the plain merge would leave alone.
    let preview = execute(&full, true, true)?;
    let changes: Vec<_> = preview
        .iter()
        .filter(|report| matches!(report.outcome, Outcome::Wrote { .. }))
        .collect();
    if changes.is_empty() {
        let _ = writeln!(out, "  Already set up.\n");
        return Ok(());
    }
    for report in &changes {
        let _ = writeln!(
            out,
            "  {}: {}",
            role_name(report.role),
            tilde(&report.path, &machine.roots)
        );
        if let Some(lines) = &report.preview {
            for line in lines.lines() {
                let _ = writeln!(out, "    | {line}");
            }
        }
    }
    if !ask("  Write this? A backup of each file is kept. [y/N] ", out) {
        let _ = writeln!(out, "  Skipped.\n");
        return Ok(());
    }
    for report in execute(&full, false, true)? {
        if let Outcome::Wrote { backup } = report.outcome {
            let _ = writeln!(out, "  wrote {}", tilde(&report.path, &machine.roots));
            if let Some(backup) = backup {
                let _ = writeln!(out, "    backup {}", tilde(&backup, &machine.roots));
            }
        }
    }
    let _ = writeln!(out);
    Ok(())
}

/// A path under the home directory as the reader would type it.
fn tilde(path: &std::path::Path, roots: &Roots) -> String {
    roots
        .home
        .as_ref()
        .and_then(|home| path.strip_prefix(home).ok())
        .map(|rest| format!("~/{}", rest.display()))
        .unwrap_or_else(|| path.display().to_string())
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Mcp => "mcp",
        Role::Gate => "gate",
    }
}

fn git_step(
    machine: &Machine,
    ask: &mut dyn FnMut(&str, &mut String) -> bool,
    out: &mut String,
) -> Result<()> {
    let _ = writeln!(out, "Git commit gate");
    let (major, minor) = GIT_HOOKS_SINCE;
    match machine.git_version {
        Some(version) if version >= GIT_HOOKS_SINCE => {}
        Some((have_major, have_minor)) => {
            let _ = writeln!(
                out,
                "  Needs git {major}.{minor} or newer, for hooks set in git config. \
                 This git is {have_major}.{have_minor}. Skipped.\n"
            );
            return Ok(());
        }
        None => {
            let _ = writeln!(out, "  git was not found. Skipped.\n");
            return Ok(());
        }
    }

    if is_git_gate_set(&machine.git_config) {
        let _ = writeln!(out, "  Already set up.\n");
        return Ok(());
    }

    let _ = writeln!(
        out,
        "  Checks each commit a coding agent makes, before git records it. When a \
         learning applies to the staged change, the commit is refused and the \
         agent is told to review it and commit again. Your own commits are not \
         touched: it acts only when Claude Code, Cursor or Gemini CLI is making \
         the commit.\n"
    );
    let _ = writeln!(
        out,
        "  It adds two lines to {}, and nothing to any repository:",
        match &machine.git_config {
            GitConfig::Global => "your global git config".to_string(),
            GitConfig::File(path) => tilde(path, &machine.roots),
        }
    );
    let _ = writeln!(out, "    hook.writ.command = {GIT_HOOK_COMMAND}");
    let _ = writeln!(out, "    hook.writ.event = {GIT_HOOK_EVENT}");
    let _ = writeln!(
        out,
        "  Hooks a repository already has keep running. To turn it off in one \
         repository: git config hook.writ.enabled false"
    );
    let _ = writeln!(out, "  To remove it: {}", machine.git_config.undo());
    if !ask("  Add it? [y/N] ", out) {
        let _ = writeln!(out, "  Skipped.\n");
        return Ok(());
    }
    set_git_gate(&machine.git_config)?;
    let _ = writeln!(out, "  Added.\n");
    Ok(())
}

fn is_git_gate_set(config: &GitConfig) -> bool {
    config.get_all("hook.writ.command") == [GIT_HOOK_COMMAND]
        && config.get_all("hook.writ.event") == [GIT_HOOK_EVENT]
}

/// Write the git gate. `writ install git` and the guided step share this.
pub fn set_git_gate(config: &GitConfig) -> Result<()> {
    config.set("hook.writ.command", GIT_HOOK_COMMAND)?;
    config.set("hook.writ.event", GIT_HOOK_EVENT)
}

/// `writ install git`: the git gate alone, without asking. Naming it is
/// the consent, as naming a host is.
pub fn install_git(machine: &Machine, print: bool) -> Result<String> {
    let mut out = String::from("writ install git\n");
    let (major, minor) = GIT_HOOKS_SINCE;
    match machine.git_version {
        Some(version) if version >= GIT_HOOKS_SINCE => {}
        _ => {
            return Err(Error::Validation {
                message: format!(
                    "the git commit gate needs git {major}.{minor} or newer, for hooks \
                     set in git config"
                ),
            });
        }
    }
    if is_git_gate_set(&machine.git_config) {
        out.push_str("  gate: already installed in git config\n");
        return Ok(out);
    }
    let verb = if print { "would set" } else { "set" };
    let _ = writeln!(out, "  gate: {verb} hook.writ.command = {GIT_HOOK_COMMAND}");
    let _ = writeln!(out, "  gate: {verb} hook.writ.event = {GIT_HOOK_EVENT}");
    if !print {
        set_git_gate(&machine.git_config)?;
        let _ = writeln!(out, "  to remove it: {}", machine.git_config.undo());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_git_version_is_read_with_or_without_a_vendor_suffix() {
        assert_eq!(parse_git_version("git version 2.55.0\n"), Some((2, 55)));
        assert_eq!(
            parse_git_version("git version 2.50.1 (Apple Git-155)"),
            Some((2, 50))
        );
        assert_eq!(parse_git_version("git version 3.0"), Some((3, 0)));
        assert_eq!(parse_git_version("nonsense"), None);
    }

    #[test]
    fn hooks_in_config_arrived_in_2_54() {
        assert!((2, 53) < GIT_HOOKS_SINCE);
        assert!((2, 54) >= GIT_HOOKS_SINCE);
        assert!((3, 0) >= GIT_HOOKS_SINCE);
    }
}
