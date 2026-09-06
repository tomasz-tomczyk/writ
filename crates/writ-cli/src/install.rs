//! `writ install HOST`. Spec section 5, `writ install`.
//!
//! Four hosts want the same two things — a stdio MCP registration and a
//! stop-time gate — in four different files and four different shapes.
//! The alternative is four README sections that tell a reader to paste
//! JSON, and those drift from the binary the moment a flag changes. That
//! is P8, and it is the failure this project has hit most often.
//!
//! Two rules govern every write here.
//!
//! **Merge, never replace.** Each of these files already holds other
//! tools. A real `~/.claude/settings.json` carries a dozen hook events,
//! and a real `~/.cursor/hooks.json` carries a `stop` entry from
//! something else. Every edit adds one key or one array element and
//! leaves the rest of the document as it was. A backup is written first
//! and its path is printed.
//!
//! **Say what was not done.** OpenCode has no stop-equivalent, so
//! `writ install opencode` reports the absence rather than printing
//! success and letting the reader assume a gate exists. Promising a gate
//! the host cannot enforce is the crit #873 failure with worse
//! consequences than a wrong flag.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use writ_core::{Error, Result};

/// The hosts `writ install` knows how to configure. Spec section 9.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Host {
    /// `.mcp.json` or `~/.claude.json`, and a `Stop` hook in `settings.json`.
    ClaudeCode,
    /// `.codex/config.toml`, and a `Stop` hook in `.codex/hooks.json`.
    Codex,
    /// `.cursor/mcp.json`, and a `stop` hook in `.cursor/hooks.json`.
    Cursor,
    /// `opencode.json`. No gate: the host has no stop-equivalent.
    Opencode,
}

impl Host {
    /// The name the reader typed, for use in a message.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
        }
    }

    /// The file this host reads its agent instructions from. Section 9.2.
    ///
    /// Claude Code reads `CLAUDE.md` and never `AGENTS.md`. The other
    /// three read `AGENTS.md`.
    pub fn instructions_file(self) -> &'static str {
        match self {
            Self::ClaudeCode => "CLAUDE.md",
            _ => "AGENTS.md",
        }
    }
}

/// Write the registration and the gate into a host's configuration.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Which host to configure
    #[arg(value_enum)]
    pub host: Host,

    /// Show what would be written and change nothing
    #[arg(long)]
    pub print: bool,

    /// Target the repository instead of the user's home
    #[arg(long)]
    pub project: bool,

    /// Replace a writ entry that is already there and differs
    #[arg(long)]
    pub force: bool,
}

/// Where a plan's paths are rooted.
#[derive(Debug, Clone)]
pub struct Roots {
    /// The user's home directory. `--project` does not need it.
    pub home: Option<PathBuf>,
    /// The repository root. Only `--project` needs it.
    pub project: Option<PathBuf>,
}

/// What one file in a plan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The MCP registration, which is discovery.
    Mcp,
    /// The stop-time gate, which is what makes the audit not optional.
    Gate,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Gate => "gate",
        }
    }
}

/// The edit one file needs. Each variant is one document shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// `mcpServers.writ`, the shape Claude Code and Cursor both read.
    /// The flag says whether to write the optional `type: "stdio"`.
    McpServers {
        /// Cursor's field table marks `type` required. Claude Code's
        /// own `.mcp.json` example omits it for a stdio server.
        typed: bool,
    },
    /// OpenCode's `mcp.writ`, where `command` is an array and not a
    /// command plus args.
    OpencodeMcp,
    /// Codex's `[mcp_servers.writ]` table in TOML.
    CodexMcp,
    /// A `Stop` event holding matcher groups, which Claude Code and
    /// Codex share.
    StopGroups,
    /// Cursor's flat `stop` array, plus the `version` the file needs.
    CursorStop,
}

/// One file the install touches.
#[derive(Debug, Clone)]
pub struct Step {
    /// The file to merge into. It does not have to exist.
    pub path: PathBuf,
    /// Whether this file carries discovery or the gate.
    pub role: Role,
    /// The document shape to merge.
    pub edit: Edit,
}

/// Everything one `writ install` run will do.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The host the reader named.
    pub host: Host,
    /// The files to merge into, in order.
    pub steps: Vec<Step>,
    /// Why no gate was installed, when none was.
    pub no_gate: Option<&'static str>,
}

/// The reason OpenCode gets no gate, in the words section 9.2 uses.
const OPENCODE_NO_GATE: &str = "OpenCode has no stop-equivalent. Every plugin hook returns \
     Promise<void>, and `session.idle` reaches only the fire-and-forget `event` hook, so \
     nothing there can block a turn or inject a prompt.";

/// Build the plan for one host and one scope.
///
/// The paths come from spec section 9.2 and from each host's own
/// documentation. They are a contract, so they live in one function that
/// a test can read back.
pub fn plan(host: Host, project: bool, roots: &Roots) -> Result<Plan> {
    let root = if project {
        roots.project.clone().ok_or_else(|| Error::Validation {
            message: "writ install --project needs a repository. Run it inside one".to_string(),
        })?
    } else {
        roots.home.clone().ok_or_else(|| Error::Validation {
            message: "writ install needs HOME, or --project to target a repository".to_string(),
        })?
    };

    let steps = match (host, project) {
        // Claude Code keeps a user-scoped server in ~/.claude.json and a
        // project-scoped one in .mcp.json at the repository root.
        (Host::ClaudeCode, false) => vec![
            step(
                root.join(".claude.json"),
                Role::Mcp,
                Edit::McpServers { typed: false },
            ),
            step(
                root.join(".claude").join("settings.json"),
                Role::Gate,
                Edit::StopGroups,
            ),
        ],
        (Host::ClaudeCode, true) => vec![
            step(
                root.join(".mcp.json"),
                Role::Mcp,
                Edit::McpServers { typed: false },
            ),
            step(
                root.join(".claude").join("settings.json"),
                Role::Gate,
                Edit::StopGroups,
            ),
        ],
        // Codex reads config.toml and hooks.json from the same layer.
        // Keeping the hooks in hooks.json rather than an inline [hooks]
        // table is deliberate: Codex warns when one layer holds both.
        (Host::Codex, _) => vec![
            step(
                root.join(".codex").join("config.toml"),
                Role::Mcp,
                Edit::CodexMcp,
            ),
            step(
                root.join(".codex").join("hooks.json"),
                Role::Gate,
                Edit::StopGroups,
            ),
        ],
        (Host::Cursor, _) => vec![
            step(
                root.join(".cursor").join("mcp.json"),
                Role::Mcp,
                Edit::McpServers { typed: true },
            ),
            step(
                root.join(".cursor").join("hooks.json"),
                Role::Gate,
                Edit::CursorStop,
            ),
        ],
        (Host::Opencode, false) => vec![step(
            root.join(".config").join("opencode").join("opencode.json"),
            Role::Mcp,
            Edit::OpencodeMcp,
        )],
        (Host::Opencode, true) => vec![step(
            root.join("opencode.json"),
            Role::Mcp,
            Edit::OpencodeMcp,
        )],
    };

    Ok(Plan {
        host,
        steps,
        no_gate: (host == Host::Opencode).then_some(OPENCODE_NO_GATE),
    })
}

fn step(path: PathBuf, role: Role, edit: Edit) -> Step {
    Step { path, role, edit }
}

/// What merging one file produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    /// The whole file as it should be written.
    pub text: String,
    /// Whether that differs from what was there.
    pub changed: bool,
    /// What the reader needs to know that the diff does not say.
    pub note: Option<String>,
}

impl Merge {
    fn unchanged(text: String, note: Option<String>) -> Self {
        Self {
            text,
            changed: false,
            note,
        }
    }
}

/// The one line that says a writ entry was there already and differs.
fn conflict(what: &str) -> String {
    format!("{what} is already configured and differs. Left alone. Pass --force to replace it")
}

impl Edit {
    /// Merge writ's entry into `existing`, or into an empty document.
    ///
    /// `existing` is the file as it is on disk, or `None` when there is
    /// no file yet. The returned text is the whole file.
    pub fn apply(self, existing: Option<&str>, force: bool) -> Result<Merge> {
        match self {
            Self::McpServers { typed } => {
                let mut server = Map::new();
                if typed {
                    server.insert("type".to_string(), json!("stdio"));
                }
                server.insert("command".to_string(), json!("writ"));
                server.insert("args".to_string(), json!(["mcp"]));
                merge_named(existing, force, &["mcpServers"], Value::Object(server))
            }
            Self::OpencodeMcp => merge_named(
                existing,
                force,
                &["mcp"],
                json!({ "type": "local", "command": ["writ", "mcp"], "enabled": true }),
            ),
            Self::CodexMcp => merge_codex_toml(existing, force),
            Self::StopGroups => merge_stop_groups(existing, force),
            Self::CursorStop => merge_cursor_stop(existing, force),
        }
    }
}

/// Read a JSON document, or start an empty one.
///
/// A file that is not a JSON object is never overwritten. Replacing a
/// document writ cannot read is exactly the clobbering this command
/// exists to avoid, so it is an error the reader can act on.
fn document(existing: Option<&str>, path_hint: &str) -> Result<Map<String, Value>> {
    let Some(text) = existing else {
        return Ok(Map::new());
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Error::Validation {
            message: format!("{path_hint} is not a JSON object. Refusing to replace it"),
        }),
        Err(error) => Err(Error::Validation {
            message: format!("{path_hint} is not valid JSON ({error}). Refusing to replace it"),
        }),
    }
}

/// Serialize a document the way these files are usually written: two
/// spaces, and a trailing newline.
fn render(document: &Map<String, Value>) -> Result<String> {
    let mut text = serde_json::to_string_pretty(document).map_err(|error| Error::Storage {
        message: format!("cannot serialize the merged configuration: {error}"),
    })?;
    text.push('\n');
    Ok(text)
}

/// Put `value` at `path...writ`, keeping everything else.
fn merge_named(existing: Option<&str>, force: bool, path: &[&str], value: Value) -> Result<Merge> {
    let original = existing.unwrap_or_default().to_string();
    let mut document = document(existing, path[0])?;

    let mut cursor = &mut document;
    for key in path {
        let entry = cursor
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        cursor = entry.as_object_mut().ok_or_else(|| Error::Validation {
            message: format!("`{key}` is not an object. Refusing to replace it"),
        })?;
    }

    let label = format!("{}.writ", path.join("."));
    match cursor.get("writ") {
        Some(current) if *current == value => {
            return Ok(Merge::unchanged(original, None));
        }
        Some(_) if !force => {
            return Ok(Merge::unchanged(original, Some(conflict(&label))));
        }
        _ => {}
    }

    cursor.insert("writ".to_string(), value);
    let text = render(&document)?;
    Ok(Merge {
        changed: text != original,
        text,
        note: None,
    })
}

/// The command a gate hook runs, per host protocol. Section 9.2.
fn gate_command(host: &str) -> String {
    format!("writ audit --hook {host}")
}

/// True when this hook entry already runs writ's audit.
///
/// The test is the command, not an exact match on the whole entry, so a
/// reader who added a timeout or a matcher keeps it.
fn is_writ_hook(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains("writ audit"))
}

/// Merge a `Stop` matcher group. Claude Code and Codex share this shape:
/// an event name holds groups, and each group holds handlers.
fn merge_stop_groups(existing: Option<&str>, force: bool) -> Result<Merge> {
    let original = existing.unwrap_or_default().to_string();
    let mut document = document(existing, "the settings file")?;

    let hooks = document
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Validation {
            message: "`hooks` is not an object. Refusing to replace it".to_string(),
        })?;
    let stop = hooks
        .entry("Stop".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| Error::Validation {
            message: "`hooks.Stop` is not an array. Refusing to replace it".to_string(),
        })?;

    let existing_group = stop.iter().position(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|handlers| handlers.iter().any(is_writ_hook))
    });

    if let Some(index) = existing_group {
        if !force {
            return Ok(Merge::unchanged(
                original,
                Some(
                    "a Stop hook already runs `writ audit`. Left alone. Pass --force to replace it"
                        .to_string(),
                ),
            ));
        }
        stop.remove(index);
    }

    // The host is not known to this function, so it writes a placeholder
    // and the caller substitutes. Keeping one placeholder is simpler than
    // threading the host through every merge signature, and the
    // substitution is asserted in a test.
    stop.push(json!({
        "hooks": [{
            "type": "command",
            "command": gate_command(HOST_PLACEHOLDER),
            "timeout": 60
        }]
    }));

    let text = render(&document)?;
    Ok(Merge {
        changed: text != original,
        text,
        note: None,
    })
}

/// Cursor's `stop` is a flat array of scripts, and the file carries a
/// `version`. Section 9.2 and Cursor's hooks reference.
fn merge_cursor_stop(existing: Option<&str>, force: bool) -> Result<Merge> {
    let original = existing.unwrap_or_default().to_string();
    let mut document = document(existing, ".cursor/hooks.json")?;

    // Only set the version when the file does not have one. A reader on a
    // later version keeps it.
    document
        .entry("version".to_string())
        .or_insert_with(|| json!(1));

    let hooks = document
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Validation {
            message: "`hooks` is not an object. Refusing to replace it".to_string(),
        })?;
    let stop = hooks
        .entry("stop".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| Error::Validation {
            message: "`hooks.stop` is not an array. Refusing to replace it".to_string(),
        })?;

    if let Some(index) = stop.iter().position(is_writ_hook) {
        if !force {
            return Ok(Merge::unchanged(
                original,
                Some(
                    "a stop hook already runs `writ audit`. Left alone. Pass --force to replace it"
                        .to_string(),
                ),
            ));
        }
        stop.remove(index);
    }

    // `loop_limit` is Cursor's own retry cap and it defaults to 5, which
    // is the number section 9.2 names. Writing it makes the cap visible
    // in the file rather than implied by a default.
    stop.push(json!({ "command": gate_command("cursor"), "loop_limit": 5 }));

    let text = render(&document)?;
    Ok(Merge {
        changed: text != original,
        text,
        note: None,
    })
}

/// The `[mcp_servers.writ]` table, as text.
const CODEX_TABLE: &str = "[mcp_servers.writ]\ncommand = \"writ\"\nargs = [\"mcp\"]\n";

/// Merge Codex's TOML by editing text, not by reparsing and reprinting.
///
/// A `~/.codex/config.toml` is hand-written and full of comments. A
/// round trip through a TOML value drops every one of them, which is a
/// worse outcome than the merge this command promises. So the parse is
/// used only to answer "is writ already here", and the edit is either an
/// append or a replacement of writ's own block.
fn merge_codex_toml(existing: Option<&str>, force: bool) -> Result<Merge> {
    let original = existing.unwrap_or_default().to_string();

    if !original.trim().is_empty() {
        // A file writ cannot parse is never rewritten, for the same
        // reason an unreadable JSON document is not.
        toml::from_str::<toml::Table>(&original).map_err(|error| Error::Validation {
            message: format!("config.toml is not valid TOML ({error}). Refusing to edit it"),
        })?;
    }

    let Some(span) = codex_table_span(&original) else {
        let mut text = original.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(CODEX_TABLE);
        return Ok(Merge {
            changed: true,
            text,
            note: None,
        });
    };

    if original[span.clone()].trim() == CODEX_TABLE.trim() {
        return Ok(Merge::unchanged(original, None));
    }
    if !force {
        return Ok(Merge::unchanged(
            original,
            Some(conflict("[mcp_servers.writ]")),
        ));
    }

    let mut text = String::with_capacity(original.len());
    text.push_str(&original[..span.start]);
    text.push_str(CODEX_TABLE);
    text.push_str(&original[span.end..]);
    Ok(Merge {
        changed: text != original,
        text,
        note: None,
    })
}

/// Where writ's own TOML table starts and ends.
///
/// The end is the next table header at any nesting, or the end of the
/// file. Only writ's block moves, so a neighbouring `[mcp_servers.other]`
/// and every comment around it survive.
fn codex_table_span(text: &str) -> Option<std::ops::Range<usize>> {
    let mut offset = 0;
    let mut start = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let is_header = trimmed.starts_with('[');
        if let Some(start) = start
            && is_header
        {
            return Some(start..offset);
        }
        if is_header && header_name(trimmed).as_deref() == Some("mcp_servers.writ") {
            start = Some(offset);
        }
        offset += line.len();
    }
    start.map(|start| start..text.len())
}

/// The dotted name inside a `[table]` header, with quotes removed so
/// `[mcp_servers."writ"]` is recognized as the same table.
fn header_name(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?;
    let inner = inner.split(']').next()?;
    Some(
        inner
            .split('.')
            .map(|part| part.trim().trim_matches('"').trim_matches('\''))
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// The token `merge_stop_groups` writes in place of the host name.
const HOST_PLACEHOLDER: &str = "{host}";

/// Put the real host into a merged document.
fn substitute_host(text: &str, host: Host) -> String {
    text.replace(
        &gate_command(HOST_PLACEHOLDER),
        &gate_command(host.as_str()),
    )
}

/// What one executed step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The file was written, and this is where the old one went.
    Wrote {
        /// `None` when the file did not exist, so nothing needed saving.
        backup: Option<PathBuf>,
    },
    /// The entry was already exactly right.
    Present,
    /// Nothing was written, and the note says why.
    Skipped,
}

/// One line of the report `writ install` prints.
#[derive(Debug, Clone)]
pub struct Report {
    /// The file this line is about.
    pub path: PathBuf,
    /// Discovery or the gate.
    pub role: Role,
    /// What happened.
    pub outcome: Outcome,
    /// What the reader needs to know.
    pub note: Option<String>,
    /// The merged file, when `--print` asked to see it.
    pub preview: Option<String>,
}

/// Run a plan. With `print`, read and merge but write nothing.
pub fn execute(plan: &Plan, print: bool, force: bool) -> Result<Vec<Report>> {
    let mut reports = Vec::with_capacity(plan.steps.len());
    for step in &plan.steps {
        let existing = read(&step.path)?;
        let merge = step.edit.apply(existing.as_deref(), force)?;
        let text = substitute_host(&merge.text, plan.host);
        let changed = merge.changed && Some(&text) != existing.as_ref();

        let settled = if merge.note.is_some() {
            Outcome::Skipped
        } else {
            Outcome::Present
        };

        let (outcome, preview) = if print {
            let outcome = if changed {
                Outcome::Wrote { backup: None }
            } else {
                settled
            };
            (outcome, Some(text))
        } else if !changed {
            (settled, None)
        } else {
            let backup = match &existing {
                Some(original) => Some(back_up(&step.path, original)?),
                None => None,
            };
            write(&step.path, &text)?;
            (Outcome::Wrote { backup }, None)
        };

        reports.push(Report {
            path: step.path.clone(),
            role: step.role,
            outcome,
            note: merge.note,
            preview,
        });
    }
    Ok(reports)
}

fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Storage {
            message: format!("cannot read {}: {error}", path.display()),
        }),
    }
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::CreateDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, text).map_err(|error| Error::Storage {
        message: format!("cannot write {}: {error}", path.display()),
    })
}

/// Copy the file aside before changing it, and say where it went.
///
/// The name carries a timestamp so a second run never overwrites the
/// backup the first run took, which would leave the reader holding a
/// copy of writ's own output instead of their configuration.
fn back_up(path: &Path, original: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".writ-backup-{stamp}"));
    let backup = path.with_file_name(name);
    std::fs::write(&backup, original).map_err(|error| Error::Storage {
        message: format!("cannot write the backup {}: {error}", backup.display()),
    })?;
    Ok(backup)
}

/// Render the report a reader sees.
///
/// Every host prints its instructions file, because writ never writes
/// into `CLAUDE.md` or `AGENTS.md`: those are the reader's own words, and
/// a tool that edits them has to be trusted with the whole file.
pub fn render_report(plan: &Plan, reports: &[Report], print: bool) -> String {
    let mut out = String::new();
    let verb = if print { "would write" } else { "wrote" };

    let _ = writeln!(out, "writ install {}", plan.host.as_str());
    for report in reports {
        let path = report.path.display();
        let role = report.role.as_str();
        match &report.outcome {
            Outcome::Wrote { backup } => {
                let _ = writeln!(out, "  {role}: {verb} {path}");
                if let Some(backup) = backup {
                    let _ = writeln!(out, "        backup {}", backup.display());
                }
            }
            Outcome::Present => {
                let _ = writeln!(out, "  {role}: already installed in {path}");
            }
            Outcome::Skipped => {
                let _ = writeln!(out, "  {role}: unchanged {path}");
            }
        }
        if let Some(note) = &report.note {
            let _ = writeln!(out, "        {note}");
        }
        if let Some(preview) = &report.preview {
            for line in preview.lines() {
                let _ = writeln!(out, "        | {line}");
            }
        }
    }

    if let Some(reason) = plan.no_gate {
        let _ = writeln!(out, "  gate: NOT INSTALLED.");
        for line in wrap(reason, 66) {
            let _ = writeln!(out, "        {line}");
        }
        let _ = writeln!(
            out,
            "        The audit is reachable over MCP and can be asked for in"
        );
        let _ = writeln!(
            out,
            "        {}, but nothing enforces it.",
            plan.host.instructions_file()
        );
    }

    let _ = writeln!(
        out,
        "  next: add the writ block to {}. writ does not write that file.",
        plan.host.instructions_file()
    );
    out
}

/// Break a sentence at spaces so a note fits an 80-column terminal.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Run the subcommand.
pub fn run(args: &Args, roots: &Roots) -> Result<()> {
    let plan = plan(args.host, args.project, roots)?;
    let reports = execute(&plan, args.print, args.force)?;
    print!("{}", render_report(&plan, &reports, args.print));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(edit: Edit, existing: Option<&str>) -> Value {
        let merge = edit.apply(existing, false).expect("merge");
        serde_json::from_str(&merge.text).expect("valid JSON")
    }

    #[test]
    fn mcp_servers_keeps_every_other_server() {
        let existing = r#"{"mcpServers":{"other":{"command":"other"}},"extra":1}"#;
        let value = merged(Edit::McpServers { typed: false }, Some(existing));
        assert_eq!(value["mcpServers"]["other"]["command"], json!("other"));
        assert_eq!(value["extra"], json!(1));
        assert_eq!(value["mcpServers"]["writ"]["command"], json!("writ"));
        assert_eq!(value["mcpServers"]["writ"]["args"], json!(["mcp"]));
        assert_eq!(value["mcpServers"]["writ"].get("type"), None);
    }

    #[test]
    fn cursor_mcp_declares_the_stdio_type_its_field_table_requires() {
        let value = merged(Edit::McpServers { typed: true }, None);
        assert_eq!(value["mcpServers"]["writ"]["type"], json!("stdio"));
    }

    #[test]
    fn opencode_takes_one_command_array_and_not_command_plus_args() {
        let value = merged(Edit::OpencodeMcp, None);
        assert_eq!(value["mcp"]["writ"]["command"], json!(["writ", "mcp"]));
        assert_eq!(value["mcp"]["writ"]["type"], json!("local"));
        assert_eq!(value["mcp"]["writ"]["enabled"], json!(true));
        assert_eq!(value["mcp"]["writ"].get("args"), None);
    }

    #[test]
    fn a_second_run_adds_nothing() {
        let first = Edit::McpServers { typed: false }
            .apply(None, false)
            .expect("first");
        let second = Edit::McpServers { typed: false }
            .apply(Some(&first.text), false)
            .expect("second");
        assert!(!second.changed);
        assert_eq!(second.text, first.text);
        assert_eq!(second.note, None);
    }

    #[test]
    fn a_different_writ_entry_is_left_alone_without_force() {
        let existing = r#"{"mcpServers":{"writ":{"command":"/opt/writ"}}}"#;
        let merge = Edit::McpServers { typed: false }
            .apply(Some(existing), false)
            .expect("merge");
        assert!(!merge.changed);
        assert_eq!(merge.text, existing);
        assert!(merge.note.expect("note").contains("--force"));
    }

    #[test]
    fn force_replaces_a_different_writ_entry() {
        let existing = r#"{"mcpServers":{"writ":{"command":"/opt/writ"}}}"#;
        let merge = Edit::McpServers { typed: false }
            .apply(Some(existing), true)
            .expect("merge");
        assert!(merge.changed);
        let value: Value = serde_json::from_str(&merge.text).expect("valid JSON");
        assert_eq!(value["mcpServers"]["writ"]["command"], json!("writ"));
    }

    #[test]
    fn a_settings_file_keeps_every_other_hook_event() {
        let existing = r#"{
          "permissions": {"allow": ["Bash"]},
          "hooks": {
            "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "guard"}]}],
            "Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]
          }
        }"#;
        let value = merged(Edit::StopGroups, Some(existing));
        assert_eq!(value["permissions"]["allow"], json!(["Bash"]));
        assert!(value["hooks"]["PreToolUse"].is_array());
        let stop = value["hooks"]["Stop"].as_array().expect("array");
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], json!("say done"));
        assert_eq!(
            stop[1]["hooks"][0]["command"],
            json!("writ audit --hook {host}")
        );
    }

    #[test]
    fn the_host_name_reaches_the_written_command() {
        let merge = Edit::StopGroups.apply(None, false).expect("merge");
        let text = substitute_host(&merge.text, Host::Codex);
        assert!(text.contains("writ audit --hook codex"), "{text}");
        assert!(!text.contains(HOST_PLACEHOLDER), "{text}");
    }

    #[test]
    fn a_stop_hook_that_already_runs_writ_is_not_duplicated() {
        let first = Edit::StopGroups.apply(None, false).expect("first");
        let second = Edit::StopGroups
            .apply(Some(&first.text), false)
            .expect("second");
        assert!(!second.changed);
        assert!(second.note.is_some());
    }

    #[test]
    fn cursor_keeps_another_tools_stop_entry() {
        let existing = r#"{"version": 1, "hooks": {"stop": [{"command": "./audit.sh"}]}}"#;
        let value = merged(Edit::CursorStop, Some(existing));
        let stop = value["hooks"]["stop"].as_array().expect("array");
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["command"], json!("./audit.sh"));
        assert_eq!(stop[1]["command"], json!("writ audit --hook cursor"));
        assert_eq!(stop[1]["loop_limit"], json!(5));
        assert_eq!(value["version"], json!(1));
    }

    #[test]
    fn cursor_does_not_downgrade_a_later_version() {
        let value = merged(Edit::CursorStop, Some(r#"{"version": 2}"#));
        assert_eq!(value["version"], json!(2));
    }

    #[test]
    fn codex_toml_appends_and_keeps_every_comment() {
        let existing =
            "# my settings\nmodel = \"gpt-5.5\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        let merge = Edit::CodexMcp.apply(Some(existing), false).expect("merge");
        assert!(merge.changed);
        assert!(merge.text.starts_with(existing), "{}", merge.text);
        assert!(merge.text.contains("[mcp_servers.writ]"));
        assert!(merge.text.contains("# my settings"));
        let table: toml::Table = toml::from_str(&merge.text).expect("valid TOML");
        assert!(table["mcp_servers"]["other"].is_table());
    }

    #[test]
    fn codex_toml_is_idempotent() {
        let first = Edit::CodexMcp.apply(None, false).expect("first");
        let second = Edit::CodexMcp
            .apply(Some(&first.text), false)
            .expect("second");
        assert!(!second.changed);
        assert_eq!(second.text, first.text);
    }

    #[test]
    fn codex_force_replaces_only_writs_own_table() {
        let existing = "[mcp_servers.writ]\ncommand = \"/opt/writ\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        let merge = Edit::CodexMcp.apply(Some(existing), true).expect("merge");
        assert!(merge.changed);
        assert!(merge.text.contains("[mcp_servers.other]"));
        assert!(!merge.text.contains("/opt/writ"));
        let table: toml::Table = toml::from_str(&merge.text).expect("valid TOML");
        assert_eq!(
            table["mcp_servers"]["writ"]["command"].as_str(),
            Some("writ")
        );
    }

    #[test]
    fn a_file_that_is_not_json_is_never_replaced() {
        let error = Edit::McpServers { typed: false }
            .apply(Some("not json at all"), true)
            .expect_err("refusal");
        assert!(matches!(error, Error::Validation { .. }));
    }

    #[test]
    fn a_file_that_is_not_toml_is_never_replaced() {
        let error = Edit::CodexMcp
            .apply(Some("[unclosed\n"), true)
            .expect_err("refusal");
        assert!(matches!(error, Error::Validation { .. }));
    }

    #[test]
    fn opencode_has_no_gate_step_and_says_so() {
        let roots = Roots {
            home: Some(PathBuf::from("/home/x")),
            project: None,
        };
        let plan = plan(Host::Opencode, false, &roots).expect("plan");
        assert!(plan.steps.iter().all(|step| step.role == Role::Mcp));
        assert!(plan.no_gate.is_some());
        let rendered = render_report(&plan, &[], false);
        assert!(rendered.contains("gate: NOT INSTALLED"), "{rendered}");
    }

    #[test]
    fn every_other_host_plans_a_gate() {
        let roots = Roots {
            home: Some(PathBuf::from("/home/x")),
            project: Some(PathBuf::from("/repo")),
        };
        for host in [Host::ClaudeCode, Host::Codex, Host::Cursor] {
            for project in [false, true] {
                let plan = plan(host, project, &roots).expect("plan");
                assert!(
                    plan.steps.iter().any(|step| step.role == Role::Gate),
                    "{host:?} project={project}"
                );
                assert!(plan.no_gate.is_none(), "{host:?}");
            }
        }
    }

    #[test]
    fn project_scope_needs_a_repository() {
        let roots = Roots {
            home: Some(PathBuf::from("/home/x")),
            project: None,
        };
        let error = plan(Host::Cursor, true, &roots).expect_err("refusal");
        assert!(matches!(error, Error::Validation { .. }));
    }
}
