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
use writ_cli::install::{Host, Outcome, Roots, execute, plan, render_report};

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
        assert_eq!(
            after["hooks"][event], original["hooks"][event],
            "{event} was changed"
        );
    }
    let stop = after["hooks"]["Stop"].as_array().expect("Stop array");
    assert_eq!(stop.len(), 2, "the other tool's Stop entry was replaced");
    assert_eq!(stop[0]["hooks"][0]["command"], json!("somebody-else.sh"));
    assert_eq!(
        stop[1]["hooks"][0]["command"],
        json!("writ audit --hook claude-code")
    );
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
    assert_eq!(stop[1]["command"], json!("writ audit --hook cursor"));
}

#[test]
fn codex_gets_its_own_protocol_and_not_claude_codes() {
    let sandbox = sandbox();
    let home = sandbox.roots.home.clone().expect("home");
    install(&sandbox.roots, Host::Codex, false, false);

    let hooks = read_json(&home.join(".codex").join("hooks.json"));
    assert_eq!(
        hooks["hooks"]["Stop"][0]["hooks"][0]["command"],
        json!("writ audit --hook codex")
    );

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
