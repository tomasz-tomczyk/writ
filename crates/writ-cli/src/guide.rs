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
    /// `git config --local` in this repository: `.git/config`, which its
    /// worktrees share and which is never committed.
    Local(PathBuf),
    /// One file, for tests.
    File(PathBuf),
}

impl GitConfig {
    fn scope(&self) -> Vec<String> {
        match self {
            Self::Global => vec!["--global".to_string()],
            Self::Local(_) => vec!["--local".to_string()],
            Self::File(path) => vec!["--file".to_string(), path.display().to_string()],
        }
    }

    /// `git`, run where this config is read: the repository for `--local`.
    fn git(&self) -> Command {
        let mut command = Command::new("git");
        if let Self::Local(root) = self {
            command.current_dir(root);
        }
        command
    }

    fn get_all(&self, key: &str) -> Vec<String> {
        let mut args = vec!["config".to_string()];
        args.extend(self.scope());
        args.extend(["--get-all".to_string(), key.to_string()]);
        self.git()
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
        let output = self
            .git()
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

/// Where the guided setup writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The home directory and the global git config: every repository.
    Global,
    /// One repository: its agent files and its own git config.
    Project,
}

/// The machine a guided install looks at.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Where host configuration lives. `project` is the repository around
    /// the working directory, when there is one.
    pub roots: Roots,
    /// `git --version`, as major and minor. `None` when git is missing or
    /// prints something unreadable.
    pub git_version: Option<(u32, u32)>,
    /// Where the commit gate goes for a global setup. A project setup
    /// uses the repository's own config.
    pub git_config: GitConfig,
    /// The scope, when the caller named it. `None` asks, inside a
    /// repository, and is global outside one.
    pub scope: Option<Scope>,
}

impl Machine {
    /// This machine: `$HOME`, the git on `PATH`, and the global config.
    /// `project` names the scope up front, and needs a repository.
    pub fn here(roots: Roots, project: bool) -> Result<Self> {
        if project && roots.project.is_none() {
            return Err(Error::Validation {
                message: "writ install --project needs a repository. Run it inside one".to_string(),
            });
        }
        let git_version = Command::new("git")
            .arg("--version")
            .output()
            .ok()
            .and_then(|output| parse_git_version(&String::from_utf8_lossy(&output.stdout)));
        Ok(Self {
            roots,
            git_version,
            git_config: GitConfig::Global,
            scope: project.then_some(Scope::Project),
        })
    }

    /// Where the commit gate goes for `scope`.
    fn git_config_for(&self, scope: Scope) -> GitConfig {
        match (scope, &self.roots.project) {
            (Scope::Project, Some(root)) => GitConfig::Local(root.clone()),
            _ => self.git_config.clone(),
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

/// A question the guided setup puts to the reader. The kind is passed
/// along with the words, so `--yes` can answer each one for what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// Global or project. Enter means global.
    Scope,
    /// Which of the listed agents. Enter means all of them.
    Agents,
    /// Yes or no. Enter means no.
    Confirm,
}

/// The answer `--yes` gives: global, every agent, and yes.
pub fn default_yes(question: Question) -> &'static str {
    match question {
        Question::Scope | Question::Agents => "",
        Question::Confirm => "y",
    }
}

/// Run the guided install. `ask` puts one question to the reader and
/// returns the line they typed; everything else goes to `out`.
pub fn guide(
    machine: &Machine,
    ask: &mut dyn FnMut(Question, &str, &mut String) -> String,
    out: &mut String,
) -> Result<()> {
    let _ = writeln!(out, "writ install\n");
    let _ = writeln!(
        out,
        "writ connects to your coding agents and checks their work against your \
         learnings. It shows each change before it makes it. Nothing is written \
         without a yes.\n"
    );

    let scope = choose_scope(machine, ask, out);
    let project = scope == Scope::Project;

    let found = hosts_present(&machine.roots);
    let chosen = choose_agents(&found, ask, out);
    agents_step(machine, &chosen, project, ask, out)?;
    git_step(machine, scope, ask, out)?;

    let _ = writeln!(
        out,
        "Restart your agent sessions, or reconnect the writ MCP server, so they \
         load the new setup."
    );
    Ok(())
}

fn choose_scope(
    machine: &Machine,
    ask: &mut dyn FnMut(Question, &str, &mut String) -> String,
    out: &mut String,
) -> Scope {
    let scope = match (machine.scope, &machine.roots.project) {
        (Some(scope), _) => scope,
        (None, None) => {
            let _ = writeln!(
                out,
                "Not inside a repository, so this sets up every repository on \
                 this machine.\n"
            );
            Scope::Global
        }
        (None, Some(root)) => {
            let _ = writeln!(out, "Where should writ be set up?");
            let _ = writeln!(
                out,
                "  g  Global: every repository on this machine, now and later. \
                 Writes to your home directory and your global git config."
            );
            let _ = writeln!(
                out,
                "  p  Project: only {}. Writes to files in the repository and \
                 to its own git config.",
                root.display()
            );
            let answer = ask(Question::Scope, "Global or project? [G/p] ", out);
            let _ = writeln!(out);
            match answer.trim().to_ascii_lowercase().as_str() {
                "p" | "project" => Scope::Project,
                _ => Scope::Global,
            }
        }
    };
    if scope == Scope::Project {
        let _ = writeln!(
            out,
            "Project setup. The agent files go in the repository: if you commit \
             them, the setup applies to everyone who works on it. The commit gate \
             goes in the repository's own git config, which is never committed.\n"
        );
    }
    scope
}

fn choose_agents(
    found: &[Host],
    ask: &mut dyn FnMut(Question, &str, &mut String) -> String,
    out: &mut String,
) -> Vec<Host> {
    if found.is_empty() {
        let _ = writeln!(
            out,
            "No coding agent found in your home directory (~/.claude, ~/.codex, \
             ~/.cursor, ~/.config/opencode).\n"
        );
        return Vec::new();
    }
    let _ = writeln!(out, "Coding agents found:");
    for (index, host) in found.iter().enumerate() {
        let _ = writeln!(out, "  {}  {}", index + 1, title(*host));
    }
    let answer = ask(
        Question::Agents,
        "Which should writ set up? Numbers separated by commas, or Enter for all: ",
        out,
    );
    let answer = answer.trim();
    if answer.is_empty() || answer.eq_ignore_ascii_case("all") {
        let _ = writeln!(out);
        return found.to_vec();
    }
    let mut chosen = Vec::new();
    for word in answer.split(|c: char| c == ',' || c.is_whitespace()) {
        if word.is_empty() {
            continue;
        }
        match word
            .parse::<usize>()
            .ok()
            .and_then(|n| found.get(n.wrapping_sub(1)))
        {
            Some(host) if !chosen.contains(host) => chosen.push(*host),
            Some(_) => {}
            None => {
                let _ = writeln!(out, "  `{word}` is not on the list. Ignored.");
            }
        }
    }
    let _ = writeln!(out);
    chosen
}

/// Show every change for the chosen agents, then ask once.
fn agents_step(
    machine: &Machine,
    chosen: &[Host],
    project: bool,
    ask: &mut dyn FnMut(Question, &str, &mut String) -> String,
    out: &mut String,
) -> Result<()> {
    let mut plans = Vec::new();
    for &host in chosen {
        let _ = writeln!(out, "{}", title(host));
        let mut full = plan(host, project, &machine.roots)?;

        // The plugin carries its own gate. Writing a second copy would run
        // the gate twice per turn, so that step is left out rather than
        // forced. The plugin is enabled in the home settings and gates
        // every repository, so a project setup asks there too.
        let plugin = host == Host::ClaudeCode
            && (plugin_gates(&full) || plugin_gates(&plan(host, false, &machine.roots)?));
        if plugin {
            full.steps.retain(|step| step.role != Role::Gate);
            let _ = writeln!(
                out,
                "  gate: the writ@writ plugin provides it. Nothing to write."
            );
        }
        if let Some(reason) = full.no_gate {
            let _ = writeln!(out, "  gate: this agent cannot enforce one. {reason}");
        }

        // A forced preview shows the difference from what writ writes
        // today, including an older writ entry the plain merge leaves alone.
        let preview = execute(&full, true, true)?;
        let changes: Vec<_> = preview
            .iter()
            .filter(|report| matches!(report.outcome, Outcome::Wrote { .. }))
            .collect();
        if changes.is_empty() {
            let _ = writeln!(out, "  Already set up.");
            continue;
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
        plans.push(full);
    }
    if plans.is_empty() {
        if !chosen.is_empty() {
            let _ = writeln!(out);
        }
        return Ok(());
    }

    let answer = ask(
        Question::Confirm,
        "Write these changes? A backup of each file is kept. [y/N] ",
        out,
    );
    if !is_yes(&answer) {
        let _ = writeln!(out, "  Nothing written.\n");
        return Ok(());
    }
    for full in &plans {
        for report in execute(full, false, true)? {
            if let Outcome::Wrote { backup } = report.outcome {
                let _ = writeln!(out, "  wrote {}", tilde(&report.path, &machine.roots));
                if let Some(backup) = backup {
                    let _ = writeln!(out, "    backup {}", tilde(&backup, &machine.roots));
                }
            }
        }
    }
    let _ = writeln!(out);
    Ok(())
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// A path as the reader would type it: relative inside the repository,
/// `~/` under the home directory.
fn tilde(path: &std::path::Path, roots: &Roots) -> String {
    if let Some(rest) = roots
        .project
        .as_ref()
        .and_then(|root| path.strip_prefix(root).ok())
    {
        return rest.display().to_string();
    }
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

/// What the setup loses without the commit gate. Said whenever the gate
/// is offered or cannot be, because it is the half that reviews work while
/// it is small.
const WITHOUT_GIT_GATE: &str = "Without it, writ does not work fully: it \
    checks only when a turn ends, so work the agent already committed is \
    reviewed afterwards, all at once, instead of commit by commit.";

fn git_step(
    machine: &Machine,
    scope: Scope,
    ask: &mut dyn FnMut(Question, &str, &mut String) -> String,
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
                 This git is {have_major}.{have_minor}. Skipped. {WITHOUT_GIT_GATE}\n"
            );
            return Ok(());
        }
        None => {
            let _ = writeln!(out, "  git was not found. Skipped. {WITHOUT_GIT_GATE}\n");
            return Ok(());
        }
    }

    let config = machine.git_config_for(scope);
    if is_git_gate_set(&config) {
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
    let _ = writeln!(out, "  {WITHOUT_GIT_GATE}\n");
    let _ = writeln!(
        out,
        "  It adds two lines to {}:",
        match &config {
            GitConfig::Global => {
                "your global git config, and changes no file in any repository".to_string()
            }
            GitConfig::Local(_) => {
                "this repository's own git config (.git/config), which is not committed".to_string()
            }
            GitConfig::File(path) => tilde(path, &machine.roots),
        }
    );
    let _ = writeln!(out, "    hook.writ.command = {GIT_HOOK_COMMAND}");
    let _ = writeln!(out, "    hook.writ.event = {GIT_HOOK_EVENT}");
    if scope == Scope::Global {
        let _ = writeln!(
            out,
            "  Hooks a repository already has keep running. To turn it off in one \
             repository: git config hook.writ.enabled false"
        );
    } else {
        let _ = writeln!(out, "  Hooks this repository already has keep running.");
    }
    let _ = writeln!(out, "  To remove it: {}", config.undo());
    let answer = ask(Question::Confirm, "  Add it? Recommended. [y/N] ", out);
    if !is_yes(&answer) {
        let _ = writeln!(out, "  Skipped.\n");
        return Ok(());
    }
    set_git_gate(&config)?;
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
    let config = machine.git_config_for(machine.scope.unwrap_or(Scope::Global));
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
    if is_git_gate_set(&config) {
        out.push_str("  gate: already installed in git config\n");
        return Ok(out);
    }
    let verb = if print { "would set" } else { "set" };
    let _ = writeln!(out, "  gate: {verb} hook.writ.command = {GIT_HOOK_COMMAND}");
    let _ = writeln!(out, "  gate: {verb} hook.writ.event = {GIT_HOOK_EVENT}");
    if !print {
        set_git_gate(&config)?;
        let _ = writeln!(out, "  to remove it: {}", config.undo());
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
