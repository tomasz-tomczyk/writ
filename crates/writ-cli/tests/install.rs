//! `writ install HOST` against real files. Spec section 5, `writ install`.
//!
//! The unit tests in `install.rs` cover the merge as a pure function.
//! These cover what it does on disk: which files each host and scope
//! touch, that a live file keeps everything it had, that the backup is
//! real, and that `--print` writes nothing at all.
//!
//! Nothing here reads the machine's own configuration. Every path is
//! rooted in a temporary directory.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;
use writ_cli::install::{Host, Outcome, Role, Roots, execute, plan, render_report};

/// A home directory and a repository, both temporary.
struct Sandbox {
    _dir: TempDir,
    roots: Roots,
}

fn sandbox() -> Sandbox {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("home");
    let project = dir.path().join("repo");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("repo");
    Sandbox {
        _dir: dir,
        roots: Roots {
            home: Some(home),
            project: Some(project),
        },
    }
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("read {}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("parse {}", path.display()))
}

fn install(roots: &Roots, host: Host, project: bool, force: bool) -> Vec<PathBuf> {
    let plan = plan(host, project, roots).expect("plan");
    let reports = execute(&plan, false, force).expect("execute");
    reports.into_iter().map(|report| report.path).collect()
}

#[test]
fn each_host_writes_the_files_section_9_2_names() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let repo = sandbox.roots.project.clone().expect("repo");

    let cases: Vec<(Host, bool, Vec<PathBuf>)> = vec![
        (
            Host::ClaudeCode,
            false,
            vec![
                home.join(".claude.json"),
                home.join(".claude").join("settings.json"),
            ],
        ),
        (
            Host::ClaudeCode,
            true,
            vec![
                repo.join(".mcp.json"),
                repo.join(".claude").join("settings.json"),
            ],
        ),
        (
            Host::Codex,
            false,
            vec![
                home.join(".codex").join("config.toml"),
                home.join(".codex").join("hooks.json"),
            ],
        ),
        (
            Host::Cursor,
            false,
            vec![
                home.join(".cursor").join("mcp.json"),
                home.join(".cursor").join("hooks.json"),
            ],
        ),
        (
            Host::Opencode,
            false,
            vec![home.join(".config").join("opencode").join("opencode.json")],
        ),
        (Host::Opencode, true, vec![repo.join("opencode.json")]),
    ];

    for (host, project, expected) in cases {
        let touched = install(&sandbox.roots, host, project, false);
        assert_eq!(touched, expected, "{host:?} project={project}");
        for path in expected {
            assert!(
                path.exists(),
                "{host:?}: {} was not written",
                path.display()
            );
        }
    }
}

#[test]
fn a_settings_file_full_of_other_hooks_keeps_all_of_them() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");

    // The shape of a real file: many events, and a Stop entry that is
    // somebody else's. Losing any of it is the failure this test exists
    // for.
    let events = [
        "PreToolUse",
        "PostToolUse",
        "UserPromptSubmit",
        "SessionStart",
        "SessionEnd",
        "PreCompact",
        "PostCompact",
        "Notification",
        "SubagentStart",
        "SubagentStop",
        "TaskCompleted",
    ];
    let mut hooks = serde_json::Map::new();
    for event in events {
        hooks.insert(
            event.to_string(),
            json!([{"hooks": [{"type": "command", "command": format!("{event}.sh")}]}]),
        );
    }
    hooks.insert(
        "Stop".to_string(),
        json!([{"hooks": [{"type": "command", "command": "somebody-else.sh"}]}]),
    );
    let original = json!({
        "model": "opus",
        "permissions": {"allow": ["Bash(git:*)"], "deny": []},
        "hooks": hooks
    });
    write(
        &settings,
        &serde_json::to_string_pretty(&original).expect("serialize"),
    );

    install(&sandbox.roots, Host::ClaudeCode, false, false);

    let after = read_json(&settings);
    assert_eq!(after["model"], json!("opus"));
    assert_eq!(after["permissions"], original["permissions"]);
    for event in events {
        // SubagentStop is a gate now, so it is expected to change. Every
        // other event in the file has to come back byte for byte.
        if event == "SubagentStop" {
            continue;
        }
        assert_eq!(
            after["hooks"][event], original["hooks"][event],
            "{event} was changed"
        );
    }
    let subagent = after["hooks"]["SubagentStop"]
        .as_array()
        .expect("SubagentStop array");
    assert_eq!(subagent.len(), 2, "the other tool's entry was replaced");
    assert_eq!(
        subagent[0]["hooks"][0]["command"],
        json!("SubagentStop.sh"),
        "the other tool's entry must come first and survive"
    );
    let stop = after["hooks"]["Stop"].as_array().expect("Stop array");
    assert_eq!(stop.len(), 2, "the other tool's Stop entry was replaced");
    assert_eq!(stop[0]["hooks"][0]["command"], json!("somebody-else.sh"));
    assert_eq!(gate_commands(&settings, "Stop").len(), 1);
}

#[test]
fn cursors_existing_stop_entry_from_another_tool_survives() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let hooks = home.join(".cursor").join("hooks.json");
    write(
        &hooks,
        r#"{"version": 1, "hooks": {"stop": [{"command": "./audit.sh"}], "afterFileEdit": [{"command": "./fmt.sh"}]}}"#,
    );

    install(&sandbox.roots, Host::Cursor, false, false);

    let after = read_json(&hooks);
    assert_eq!(
        after["hooks"]["afterFileEdit"][0]["command"],
        json!("./fmt.sh")
    );
    let stop = after["hooks"]["stop"].as_array().expect("stop array");
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0]["command"], json!("./audit.sh"));
    assert_eq!(gate_commands(&hooks, "stop").len(), 1);
}

#[test]
fn codex_gets_its_own_protocol_and_not_claude_codes() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    install(&sandbox.roots, Host::Codex, false, false);

    let gate = gate_commands(&home.join(".codex").join("hooks.json"), "Stop").remove(0);
    assert!(gate.contains("writ audit --hook codex"), "{gate}");
    assert!(!gate.contains("claude-code"), "{gate}");

    let text = std::fs::read_to_string(home.join(".codex").join("config.toml")).expect("read");
    let table: toml::Table = toml::from_str(&text).expect("valid TOML");
    assert_eq!(
        table["mcp_servers"]["writ"]["command"].as_str(),
        Some("writ")
    );
}

#[test]
fn running_it_twice_changes_nothing_the_second_time() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");

    for host in [Host::ClaudeCode, Host::Codex, Host::Cursor, Host::Opencode] {
        let paths = install(&sandbox.roots, host, false, false);
        let first: Vec<String> = paths
            .iter()
            .map(|path| std::fs::read_to_string(path).expect("read"))
            .collect();

        let plan = plan(host, false, &sandbox.roots).expect("plan");
        let reports = execute(&plan, false, false).expect("second run");
        for report in &reports {
            assert!(
                matches!(report.outcome, Outcome::Present | Outcome::Skipped),
                "{host:?} wrote {} twice",
                report.path.display()
            );
        }

        let second: Vec<String> = paths
            .iter()
            .map(|path| std::fs::read_to_string(path).expect("read"))
            .collect();
        assert_eq!(first, second, "{host:?} changed on the second run");
    }

    // And no backup was taken for a run that changed nothing.
    let backups = std::fs::read_dir(home.join(".cursor"))
        .expect("read dir")
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("writ-backup"))
        .count();
    assert_eq!(backups, 0);
}

#[test]
fn a_change_takes_a_backup_and_names_it() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    write(&settings, r#"{"model": "opus"}"#);

    let plan = plan(Host::ClaudeCode, false, &sandbox.roots).expect("plan");
    let reports = execute(&plan, false, false).expect("execute");

    let gate = reports
        .iter()
        .find(|report| report.path == settings)
        .expect("gate report");
    let Outcome::Wrote {
        backup: Some(backup),
    } = &gate.outcome
    else {
        panic!("expected a backup, got {:?}", gate.outcome);
    };
    assert_eq!(
        std::fs::read_to_string(backup).expect("read backup"),
        r#"{"model": "opus"}"#
    );
    assert!(render_report(&plan, &reports, false).contains(&backup.display().to_string()));

    // A file that did not exist has nothing to back up.
    let mcp = reports
        .iter()
        .find(|report| report.path == home.join(".claude.json"))
        .expect("mcp report");
    assert!(matches!(mcp.outcome, Outcome::Wrote { backup: None }));
}

#[test]
fn print_changes_nothing_on_disk() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    write(&settings, r#"{"model": "opus"}"#);

    let plan = plan(Host::ClaudeCode, false, &sandbox.roots).expect("plan");
    let reports = execute(&plan, true, false).expect("print");

    assert_eq!(
        std::fs::read_to_string(&settings).expect("read"),
        r#"{"model": "opus"}"#
    );
    assert!(!home.join(".claude.json").exists());
    let rendered = render_report(&plan, &reports, true);
    assert!(rendered.contains("would write"), "{rendered}");
    assert!(
        rendered.contains("writ audit --hook claude-code"),
        "{rendered}"
    );
}

#[test]
fn opencode_reports_that_no_gate_was_installed() {
    let sandbox = sandbox();
    let plan = plan(Host::Opencode, false, &sandbox.roots).expect("plan");
    let reports = execute(&plan, false, false).expect("execute");
    let rendered = render_report(&plan, &reports, false);

    assert!(rendered.contains("gate: NOT INSTALLED"), "{rendered}");
    assert!(rendered.contains("no stop-equivalent"), "{rendered}");
    assert!(!rendered.contains("--hook opencode"), "{rendered}");
}

#[test]
fn every_host_names_the_instructions_file_it_reads() {
    let sandbox = sandbox();
    for (host, file) in [
        (Host::ClaudeCode, "CLAUDE.md"),
        (Host::Codex, "AGENTS.md"),
        (Host::Cursor, "AGENTS.md"),
        (Host::Opencode, "AGENTS.md"),
    ] {
        let plan = plan(host, false, &sandbox.roots).expect("plan");
        let reports = execute(&plan, true, false).expect("print");
        let rendered = render_report(&plan, &reports, true);
        assert!(rendered.contains(file), "{host:?}: {rendered}");
    }
}

/// `--print` shows the entry, not the file the entry lands in.
///
/// `~/.claude/settings.json` and `~/.claude.json` are live configuration
/// files, and the second runs to thousands of lines. Reprinting the whole
/// document to preview a four-line insertion buries the one thing the
/// reader asked to see, and puts unrelated private content on a terminal
/// that did not ask for it.
#[test]
fn print_shows_the_entry_not_the_whole_file() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    let far_away: Vec<String> = (0..200).map(|i| format!("Bash(unrelated-{i}:*)")).collect();
    write(
        &settings,
        &serde_json::to_string(&serde_json::json!({
            "permissions": { "allow": far_away },
            "model": "opus",
        }))
        .expect("fixture"),
    );

    let plan = plan(Host::ClaudeCode, false, &sandbox.roots).expect("plan");
    let reports = execute(&plan, true, false).expect("print");
    let rendered = render_report(&plan, &reports, true);

    assert!(
        rendered.contains("writ audit --hook claude-code"),
        "the entry itself has to be shown: {rendered}"
    );
    assert!(
        !rendered.contains("unrelated-0"),
        "untouched content must stay out of the preview: {rendered}"
    );
    assert!(
        rendered.contains("..."),
        "the elision has to be visible, or the preview reads as the whole file: {rendered}"
    );
}

/// The writ commands one event of one hooks file runs.
///
/// Handles both shapes: matcher groups holding handlers, which Claude
/// Code and Codex use, and Cursor's flat array of scripts.
fn gate_commands(settings: &Path, event: &str) -> Vec<String> {
    let Some(entries) = read_json(settings)["hooks"][event].as_array().cloned() else {
        return Vec::new();
    };
    entries
        .iter()
        .flat_map(|entry| match entry["hooks"].as_array() {
            Some(handlers) => handlers.clone(),
            None => vec![entry.clone()],
        })
        .filter_map(|hook| hook["command"].as_str().map(str::to_string))
        .filter(|command| command.contains("writ audit"))
        .collect()
}

/// Claude Code gates subagents as well as the turn.
///
/// A subagent writes to the same tree the parent will be judged on, so
/// leaving it ungated only defers the finding to the end of the turn —
/// by which point the agent that wrote the violation is gone and the
/// parent, which has less context on it, has to fix it.
#[test]
fn claude_code_gates_the_subagent_stop_too() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    install(&sandbox.roots, Host::ClaudeCode, false, false);
    let settings = home.join(".claude").join("settings.json");

    assert_eq!(gate_commands(&settings, "Stop").len(), 1);
    assert_eq!(gate_commands(&settings, "SubagentStop").len(), 1);
}

/// The two moments audit different ranges, and that is the point.
///
/// An agent that commits as it goes leaves a clean tree at `Stop`, so
/// the turn's gate has to reach back to the branch point or it audits
/// nothing. A subagent has not committed, so its gate wants the working
/// tree alone: the branch point would hand a read-only subagent every
/// violation its parent had already committed.
#[test]
fn the_turn_gate_reaches_the_branch_point_and_the_subagent_gate_does_not() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    install(&sandbox.roots, Host::ClaudeCode, false, false);
    let settings = home.join(".claude").join("settings.json");

    let turn = gate_commands(&settings, "Stop").remove(0);
    assert!(turn.contains("merge-base"), "{turn}");
    assert!(turn.contains("--diff"), "{turn}");
    assert!(turn.contains("--hook claude-code"), "{turn}");

    let subagent = gate_commands(&settings, "SubagentStop").remove(0);
    assert_eq!(subagent, "writ audit --hook claude-code");
}

/// An install that predates the subagent gate gains it without `--force`.
///
/// The turn's gate is already there and must be left exactly as it is,
/// including any edit the reader made to it. Refusing the whole file
/// because one of the two events is settled would leave subagents
/// ungated on every machine that installed writ before this.
#[test]
fn an_existing_turn_gate_still_gains_the_subagent_gate() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    write(
        &settings,
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"writ audit --hook claude-code --my-own-edit"}]}]}}"#,
    );

    install(&sandbox.roots, Host::ClaudeCode, false, false);

    assert_eq!(
        gate_commands(&settings, "Stop"),
        vec!["writ audit --hook claude-code --my-own-edit".to_string()],
        "the reader's own gate must survive untouched"
    );
    assert_eq!(gate_commands(&settings, "SubagentStop").len(), 1);
}

/// Only Claude Code has a subagent-stop event. Section 9.2.
#[test]
fn codex_gets_the_turn_gate_and_no_subagent_gate() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    install(&sandbox.roots, Host::Codex, false, false);
    let hooks = home.join(".codex").join("hooks.json");

    assert_eq!(gate_commands(&hooks, "Stop").len(), 1);
    assert!(gate_commands(&hooks, "SubagentStop").is_empty());
}

/// Every host's turn gate has the same empty-diff problem. Cursor and
/// Codex agents commit as they go too.
#[test]
fn every_host_turn_gate_reaches_the_branch_point() {
    for (host, file, event) in [
        (Host::ClaudeCode, vec![".claude", "settings.json"], "Stop"),
        (Host::Codex, vec![".codex", "hooks.json"], "Stop"),
        (Host::Cursor, vec![".cursor", "hooks.json"], "stop"),
    ] {
        let sandbox = sandbox();
        let home = sandbox.roots.home.clone().expect("home");
        install(&sandbox.roots, host, false, false);
        let path = file.iter().fold(home, |acc, part| acc.join(part));

        let command = gate_commands(&path, event).remove(0);
        assert!(command.contains("merge-base"), "{host:?}: {command}");
        assert!(
            command.contains(&format!("--hook {}", host.as_str())),
            "{host:?}: {command}"
        );
    }
}

/// The shipped plugin gates exactly what `writ install` gates.
///
/// P8. The plugin carries its own copy of the hook configuration, so a
/// reader who installs through `/plugin install` and a reader who runs
/// `writ install` must end up with the same gate. Nothing but a test
/// keeps those two files in step.
#[test]
fn the_claude_code_plugin_ships_the_same_gate_the_installer_writes() {
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/claude-code/hooks/hooks.json")
        .canonicalize()
        .expect("the plugin hooks file");

    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    install(&sandbox.roots, Host::ClaudeCode, false, false);
    let settings = home.join(".claude").join("settings.json");

    for event in ["Stop", "SubagentStop"] {
        assert_eq!(
            gate_commands(&plugin, event),
            gate_commands(&settings, event),
            "the plugin and `writ install` disagree about {event}"
        );
    }
}

/// The plugin and `writ install` are two delivery paths, and exactly one
/// may be live. Spec section 9.3.
///
/// Pinning their text is what made this invisible. Both surfaces emit the
/// identical command, so a machine carrying both runs two identical gates
/// per turn and nothing looks wrong: the pointer lands in the transcript
/// twice, two `audits` rows open for one moment, and `times_selected`
/// moves twice for one gate. That is the section 6 counter conflation
/// arriving through configuration instead of through code.
///
/// `is_writ_hook` already settles this *within* the file. What was
/// missing is idempotence *across* the two surfaces.
#[test]
fn the_enabled_plugin_stops_the_installer_writing_a_second_gate() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    write(&settings, r#"{"enabledPlugins":{"writ@writ":true}}"#);

    let plan = plan(Host::ClaudeCode, false, &sandbox.roots).expect("plan");
    let reports = execute(&plan, false, false).expect("execute");

    assert!(
        gate_commands(&settings, "Stop").is_empty(),
        "the plugin already registers the turn gate"
    );
    assert!(
        gate_commands(&settings, "SubagentStop").is_empty(),
        "the plugin already registers the subagent gate"
    );

    let note = reports
        .iter()
        .find(|report| matches!(report.role, Role::Gate))
        .and_then(|report| report.note.clone())
        .expect("the gate line carries a note");
    assert!(note.contains("writ@writ"), "{note}");
    assert!(note.contains("--force"), "{note}");
}

/// A reader who disabled the plugin's hooks is not writ's to overrule.
#[test]
fn force_writes_the_gate_even_with_the_plugin_enabled() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    write(&settings, r#"{"enabledPlugins":{"writ@writ":true}}"#);

    install(&sandbox.roots, Host::ClaudeCode, false, true);

    assert_eq!(gate_commands(&settings, "Stop").len(), 1);
    assert_eq!(gate_commands(&settings, "SubagentStop").len(), 1);
}

/// A plugin entry that is present but switched off is not a live gate.
#[test]
fn a_disabled_plugin_entry_does_not_stop_the_installer() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    let settings = home.join(".claude").join("settings.json");
    write(&settings, r#"{"enabledPlugins":{"writ@writ":false}}"#);

    install(&sandbox.roots, Host::ClaudeCode, false, false);

    assert_eq!(gate_commands(&settings, "Stop").len(), 1);
}
