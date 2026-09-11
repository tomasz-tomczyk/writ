use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

fn writ() -> Command {
    Command::new(env!("CARGO_BIN_EXE_writ"))
}

/// A private database, a private config file, and a git that knows nothing.
///
/// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` pointed at an empty file are
/// what make the author tests deterministic. Without them the result
/// depends on whoever runs the suite.
struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("empty.gitconfig"), "").unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn db(&self) -> PathBuf {
        self.path("learnings.db")
    }

    fn config(&self) -> PathBuf {
        self.path("config.toml")
    }

    fn telemetry_db(&self) -> PathBuf {
        self.path("xdg-data/writ/telemetry.db")
    }

    fn write_config(&self, text: &str) {
        std::fs::write(self.config(), text).unwrap();
    }

    fn write_file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn cmd(&self, args: &[&str]) -> Command {
        // Run outside any git repository, so only the config files below
        // can answer `git config user.email`.
        self.cmd_at(self.dir.path().to_path_buf(), args)
    }

    fn cmd_at(&self, dir: PathBuf, args: &[&str]) -> Command {
        let mut command = writ();
        command
            .arg("--db")
            .arg(self.db())
            .arg("--config")
            .arg(self.config())
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", self.path("empty.gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.path("empty.gitconfig"))
            .env("XDG_DATA_HOME", self.path("xdg-data"))
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("EMAIL");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        Output::from(self.cmd(args).output().unwrap())
    }

    fn run_at(&self, dir: &Path, args: &[&str]) -> Output {
        Output::from(self.cmd_at(dir.to_path_buf(), args).output().unwrap())
    }

    /// A PATH holding exactly the named tools and nothing else.
    ///
    /// Every matcher test sets this. Inheriting the ambient PATH means the
    /// suite asserts whatever the machine happens to have installed: a
    /// laptop with `ast-grep` proves the matcher path and a clean runner
    /// silently proves the degradation path instead, with the same green
    /// tick. A tool that is asked for and missing panics here rather than
    /// letting a test pass for the wrong reason.
    fn path_with(&self, tools: &[&str]) -> PathBuf {
        let bin = self.path(&format!("bin-{}", tools.join("-")));
        std::fs::create_dir_all(&bin).unwrap();
        for tool in tools {
            let link = bin.join(tool);
            if link.exists() {
                continue;
            }
            let found = which(tool).unwrap_or_else(|| {
                panic!(
                    "{tool} is not on PATH. mise.toml declares it, so run the suite \
                     through `mise run test` or `mise exec -- cargo test`"
                )
            });
            std::os::unix::fs::symlink(found, &link).unwrap();
        }
        bin
    }

    /// A git repository inside the sandbox, with one commit.
    ///
    /// `remote` is the URL of `origin`. `None` leaves the repository
    /// without one, which is the fallback row of the section 7.1 table.
    fn repo(&self, name: &str, remote: Option<&str>) -> PathBuf {
        let root = self.path(name);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main", "."]);
        if let Some(url) = remote {
            git(&root, &["remote", "add", "origin", url]);
        }
        std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "one"]);
        root
    }

    fn pipe(&self, args: &[&str], stdin: &str) -> Output {
        run_with_stdin(self.cmd(args), stdin)
    }

    /// Record one learning and return its id.
    fn record(&self, args: &[&str]) -> String {
        let mut all = vec![
            "record",
            "--title",
            "prefer sd",
            "--rule",
            "use sd",
            "--rationale",
            "sed is terse",
        ];
        all.extend_from_slice(args);
        let output = self.run(&all);
        output.assert_code(0);
        output.stdout.split_whitespace().last().unwrap().to_string()
    }

    /// The whole collection, as JSON.
    fn learnings(&self) -> serde_json::Value {
        let output = self.run(&["list", "--format", "json"]);
        output.assert_code(0);
        serde_json::from_str(&output.stdout).unwrap()
    }
}

// --- writ telemetry: explicit consent and disclosure ------------------

#[test]
fn telemetry_defaults_to_disabled_and_show_does_not_create_a_store() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["telemetry"]);
    output.assert_code(0);
    assert!(
        output.stdout.contains("telemetry: disabled"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("**Captured:**"), "{}", output.stdout);
    assert!(
        output.stdout.contains("**Never captured:**"),
        "{}",
        output.stdout
    );
    assert!(!sandbox.telemetry_db().exists());
    assert!(!sandbox.config().exists());
}

#[test]
fn telemetry_help_lists_every_local_command_and_no_upload() {
    let sandbox = Sandbox::new();
    let help = sandbox.run(&["telemetry", "--help"]);
    help.assert_code(0);
    for command in ["on", "off", "show", "dump", "purge"] {
        assert!(
            help.stdout.contains(command),
            "missing {command}: {}",
            help.stdout
        );
    }
    assert!(!help.stdout.contains("upload"), "{}", help.stdout);

    let on = sandbox.run(&["telemetry", "on", "--help"]);
    on.assert_code(0);
    assert!(on.stdout.contains("--yes"), "{}", on.stdout);
}

#[test]
fn telemetry_on_prints_privacy_text_before_accepting_yes() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["telemetry", "on", "--yes"]);
    output.assert_code(0);
    let captured = output.stdout.find("**Captured:**").unwrap();
    let enabled = output.stdout.find("telemetry enabled").unwrap();
    assert!(captured < enabled, "{}", output.stdout);
    assert!(sandbox.telemetry_db().is_file());
    let config = std::fs::read_to_string(sandbox.config()).unwrap();
    assert!(config.contains("[telemetry]\nenabled = true"), "{config}");

    let conn = rusqlite::Connection::open(sandbox.telemetry_db()).unwrap();
    let install_id: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'install_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        uuid::Uuid::parse_str(&install_id)
            .unwrap()
            .get_version_num(),
        4
    );
}

#[test]
fn telemetry_on_accepts_piped_confirmation_and_declining_is_safe() {
    let declined = Sandbox::new();
    declined.pipe(&["telemetry", "on"], "no\n").assert_code(0);
    assert!(!declined.telemetry_db().exists());
    assert!(!declined.config().exists());

    let accepted = Sandbox::new();
    accepted.pipe(&["telemetry", "on"], "yes\n").assert_code(0);
    assert!(accepted.telemetry_db().is_file());
}

#[test]
fn telemetry_off_keeps_data_and_purge_removes_only_telemetry() {
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    sandbox.run(&["telemetry", "on", "--yes"]).assert_code(0);

    sandbox.run(&["telemetry", "off"]).assert_code(0);
    assert!(sandbox.telemetry_db().is_file());
    assert!(sandbox.db().is_file());
    assert!(
        !writ_core::Config::parse(&std::fs::read_to_string(sandbox.config()).unwrap())
            .unwrap()
            .telemetry
            .enabled
    );

    sandbox.run(&["telemetry", "purge"]).assert_code(0);
    assert!(!sandbox.telemetry_db().exists());
    assert!(sandbox.db().is_file());
    assert!(sandbox.config().is_file());
}

#[test]
fn telemetry_dump_is_versioned_json_and_show_displays_every_row() {
    let sandbox = Sandbox::new();
    sandbox.run(&["telemetry", "on", "--yes"]).assert_code(0);
    let conn = rusqlite::Connection::open(sandbox.telemetry_db()).unwrap();
    conn.execute(
        "INSERT INTO counters (day, metric, label, count) VALUES ('2026-09-06','command','audit',12)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO buckets (day, metric, bucket, count) VALUES ('2026-09-06','audit_sent','6-20',9)",
        [],
    )
    .unwrap();
    drop(conn);

    let dump = sandbox.run(&["telemetry", "dump"]);
    dump.assert_code(0);
    let json: writ_core::TelemetryDump = serde_json::from_str(&dump.stdout).unwrap();
    assert_eq!(json.writ_telemetry, 1);
    assert_eq!(json.counters.len(), 1);
    assert_eq!(json.buckets.len(), 1);
    assert_eq!(json.bucket_edges.len(), 7);
    assert_eq!(
        serde_json::from_str::<writ_core::TelemetryDump>(&dump.stdout).unwrap(),
        json
    );

    let show = sandbox.run(&["telemetry", "show"]);
    show.assert_code(0);
    for held in [
        "install_id",
        "enabled_at",
        "schema_version",
        "2026-09-06  command  audit  12",
        "2026-09-06  audit_sent  6-20  9",
    ] {
        assert!(
            show.stdout.contains(held),
            "missing {held}: {}",
            show.stdout
        );
    }
}

#[test]
fn every_existing_command_leaves_no_telemetry_store_when_disabled() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("disabled", Some("git@github.com:owner/disabled.git"));
    let id = sandbox.record(&["--activate"]);
    assert!(!sandbox.telemetry_db().exists(), "record must not collect");

    for args in [
        vec!["list", "--format", "json"],
        vec!["show", id.as_str()],
        vec!["archive", id.as_str()],
        vec!["edit", id.as_str(), "--title", "t"],
        vec!["telemetry"],
        vec!["telemetry", "show"],
        vec!["telemetry", "off"],
        vec!["telemetry", "purge"],
        vec!["telemetry", "dump"],
    ] {
        let _ = sandbox.run(&args);
        assert!(
            !sandbox.telemetry_db().exists(),
            "{} created telemetry.db while disabled",
            args.join(" ")
        );
    }

    std::fs::write(root.join("a.rs"), "fn main() { changed(); }\n").unwrap();
    sandbox
        .run_at(&root, &["audit", "--format", "json"])
        .assert_code(0);
    assert!(!sandbox.telemetry_db().exists(), "audit must not collect");

    // `ui` normally serves until stopped. A deliberately unreadable learning
    // store exercises its dispatch path without leaving a server behind.
    let ui = Sandbox::new();
    std::fs::create_dir(ui.db()).unwrap();
    ui.run(&["ui", "--no-open"]).assert_code(8);
    assert!(!ui.telemetry_db().exists(), "ui must not collect");
}

#[test]
fn hand_editing_config_true_does_not_bypass_the_on_command() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[telemetry]\nenabled = true\n");
    sandbox.record(&[]);
    assert!(
        !sandbox.telemetry_db().exists(),
        "only `writ telemetry on` may create enable metadata or collect"
    );
}

#[test]
fn a_broken_telemetry_store_cannot_change_audit_results_or_exit_codes() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[telemetry]\nenabled = true\n");
    std::fs::create_dir_all(sandbox.telemetry_db()).unwrap();
    sandbox.record(&["--activate"]);
    let root = sandbox.repo(
        "broken-telemetry",
        Some("https://github.com/owner/repo.git"),
    );
    std::fs::write(root.join("a.rs"), "fn main() { changed(); }\n").unwrap();

    let success = sandbox.run_at(&root, &["audit", "--format", "json"]);
    success.assert_code(0);
    let report: serde_json::Value = serde_json::from_str(&success.stdout).unwrap();
    assert_eq!(report["sent"], 1);

    git(&root, &["add", "a.rs"]);
    git(&root, &["commit", "-qm", "changed"]);
    let empty = sandbox.run_at(&root, &["audit"]);
    empty.assert_code(7);
    assert!(empty.stderr.contains("empty"), "{}", empty.stderr);
}

#[test]
fn enabled_cli_paths_record_all_metrics_the_current_commands_can_observe() {
    let sandbox = Sandbox::new();
    sandbox.run(&["telemetry", "on", "--yes"]).assert_code(0);
    let first = sandbox.record(&[
        "--activate",
        "--scope",
        "language:rust",
        "--matcher",
        "changed",
        "--matcher-kind",
        "regex",
    ]);
    let second = sandbox.record(&[
        "--activate",
        "--scope",
        "language:rust",
        "--matcher",
        "changed",
        "--matcher-kind",
        "regex",
    ]);
    sandbox
        .run(&["record", "--reinforce", &first])
        .assert_code(0);
    sandbox
        .pipe(
            &["record", "--json"],
            r#"{"title":"json","rule":"r","rationale":"why"}
"#,
        )
        .assert_code(0);

    sandbox
        .run(&["edit", &second, "--title", "edited"])
        .assert_code(0);

    let root = sandbox.repo("metrics", Some("https://github.com/owner/metrics.git"));
    std::fs::write(root.join("a.rs"), "fn main() { changed(); }\n").unwrap();
    std::fs::write(root.join("private.qqq"), "DIFF_CONTENT_SENTINEL\n").unwrap();
    git(&root, &["add", "a.rs", "private.qqq"]);
    let audit = sandbox.run_at(&root, &["audit", "--format", "json"]);
    audit.assert_code(0);
    let report: serde_json::Value = serde_json::from_str(&audit.stdout).unwrap();
    let audit_id = report["audit_id"].as_str().unwrap();
    let ingest = format!(
        r#"{{"audit_id":"{audit_id}","findings":[
          {{"learning_id":"{first}","outcome":"fixed"}},
          {{"learning_id":"{second}","outcome":"ignored"}}
        ]}}"#
    );
    sandbox.pipe(&["audit", "--ingest"], &ingest).assert_code(1);

    let dump = sandbox.run(&["telemetry", "dump"]);
    dump.assert_code(0);
    let dump: writ_core::TelemetryDump = serde_json::from_str(&dump.stdout).unwrap();
    let metric_labels: std::collections::BTreeSet<_> = dump
        .counters
        .iter()
        .map(|row| (row.metric.as_str(), row.label.as_str()))
        .collect();
    for expected in [
        ("command", "record"),
        ("command", "audit"),
        ("command", "edit"),
        ("surface", "cli"),
        ("exit_code", "0"),
        ("exit_code", "1"),
        ("record_source", "manual"),
        ("record_source", "json"),
        ("record_status", "active"),
        ("record_status", "proposed"),
        ("scope_kind", "language"),
        ("scope_kind", "global"),
        ("language", "rust"),
        ("language", "other"),
        ("matcher_kind", "regex"),
        ("matcher_kind", "none"),
        ("matcher_result", "hit"),
        ("finding_outcome", "fixed"),
        ("finding_outcome", "ignored"),
        ("gate_result", "block"),
    ] {
        assert!(
            metric_labels.contains(&expected),
            "missing {expected:?}: {metric_labels:?}"
        );
    }

    let bucket_metrics: std::collections::BTreeSet<_> =
        dump.buckets.iter().map(|row| row.metric.as_str()).collect();
    assert_eq!(
        bucket_metrics,
        [
            "audit_considered",
            "audit_findings",
            "audit_sent",
            "collection_size",
            "command_ms",
            "diff_files",
            "prompt_chars",
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn telemetry_dump_leaks_none_of_the_enumerated_content_sentinels() {
    let sandbox = Sandbox::new();
    sandbox.run(&["telemetry", "on", "--yes"]).assert_code(0);

    let title = "TITLE_SENTINEL_7f3a";
    let rule = "RULE_SENTINEL_2b8c";
    let rationale = "RATIONALE_SENTINEL_4d1e";
    let author = "AUTHOR_SENTINEL_9a6f@example.invalid";
    let matcher = "DIFF_CONTENT_SENTINEL_5c2d";
    let exemplar_snippet = "EXEMPLAR_SNIPPET_SENTINEL_1e7b";
    let exemplar_note = "EXEMPLAR_NOTE_SENTINEL_8d4a";
    let exemplar_language = "EXEMPLAR_LANGUAGE_SENTINEL_6f9c";
    let source_adapter = "SOURCE_ADAPTER_SENTINEL_3a5d";
    let source_ref = "SOURCE_REF_SENTINEL_0c8e";
    let created_at = "2099-01-02 CREATED_TIME_SENTINEL";
    let updated_at = "2099-01-03 UPDATED_TIME_SENTINEL";
    let activated_at = "2099-01-04 ACTIVATED_TIME_SENTINEL";
    let project_scope = "remote_sentinel.example/owner_sentinel/repo_sentinel";
    let glob_scope = "src/path_sentinel.rs";
    let json = serde_json::json!({
        "title": title,
        "rule": rule,
        "rationale": rationale,
        "scopes": [
            format!("project:{project_scope}"),
            "language:rust",
            format!("glob:{glob_scope}"),
        ],
        "matcher_kind": "regex",
        "matcher": matcher,
        "exemplars": [{
            "kind": "bad",
            "language": exemplar_language,
            "snippet": exemplar_snippet,
            "note": exemplar_note,
        }],
        "status": "active",
        "author": author,
        "source_adapter": source_adapter,
        "source_ref": source_ref,
        "created_at": created_at,
        "updated_at": updated_at,
        "activated_at": activated_at,
    });
    let recorded = sandbox.pipe(&["record", "--json", "--format", "json"], &json.to_string());
    recorded.assert_code(0);
    let rows: serde_json::Value = serde_json::from_str(&recorded.stdout).unwrap();
    let learning_id = rows[0]["id"].as_str().unwrap().to_string();

    let repo_directory = "REPO_DIRECTORY_SENTINEL_4a9e";
    let remote = "https://remote_sentinel.example/owner_sentinel/repo_sentinel.git";
    let root = sandbox.repo(repo_directory, Some(remote));
    let branch = "BRANCH_SENTINEL_6d0f";
    git(&root, &["checkout", "-qb", branch]);
    let changed_path = root.join(glob_scope);
    std::fs::create_dir_all(changed_path.parent().unwrap()).unwrap();
    std::fs::write(&changed_path, format!("fn {matcher}() {{}}\n")).unwrap();
    git(&root, &["add", glob_scope]);

    let audit = sandbox.run_at(&root, &["audit", "--format", "json"]);
    audit.assert_code(0);
    let report: serde_json::Value = serde_json::from_str(&audit.stdout).unwrap();
    let audit_id = report["audit_id"].as_str().unwrap().to_string();
    assert_eq!(report["sent"], 1, "{}", audit.stdout);

    let finding_path = "FINDING_PATH_SENTINEL_8b3c.rs";
    let finding_detail = "FINDING_DETAIL_SENTINEL_1a4d";
    let findings = serde_json::json!({
        "audit_id": audit_id,
        "findings": [{
            "learning_id": learning_id,
            "path": finding_path,
            "line": 42,
            "detail": finding_detail,
            "outcome": "fixed",
        }],
    });
    sandbox
        .pipe(&["audit", "--ingest"], &findings.to_string())
        .assert_code(0);
    let finding_id = writ_core::Store::open(&sandbox.db())
        .unwrap()
        .findings_of(&learning_id)
        .unwrap()[0]
        .id
        .clone();

    let search = "SEARCH_QUERY_SENTINEL_5e7a";
    sandbox
        .run(&["list", "--search", search, "--format", "json"])
        .assert_code(0);

    let dump = sandbox.run(&["telemetry", "dump"]);
    dump.assert_code(0);
    let sentinels = [
        title,
        rule,
        rationale,
        author,
        matcher,
        exemplar_snippet,
        exemplar_note,
        exemplar_language,
        source_adapter,
        source_ref,
        created_at,
        updated_at,
        activated_at,
        project_scope,
        glob_scope,
        repo_directory,
        remote,
        branch,
        finding_path,
        finding_detail,
        search,
        &learning_id,
        &audit_id,
        &finding_id,
    ];
    for sentinel in sentinels {
        assert!(
            !dump.stdout.contains(sentinel),
            "telemetry leaked sentinel {sentinel}: {}",
            dump.stdout
        );
    }
}

#[test]
fn matcher_telemetry_distinguishes_hit_miss_and_unevaluable() {
    let sandbox = Sandbox::new();
    sandbox.run(&["telemetry", "on", "--yes"]).assert_code(0);
    for (title, pattern) in [("hit", "changed"), ("miss", "absent"), ("bad", "(")] {
        let output = sandbox.run(&[
            "record",
            "--title",
            title,
            "--rule",
            "rule",
            "--rationale",
            "why",
            "--activate",
            "--matcher-kind",
            "regex",
            "--matcher",
            pattern,
        ]);
        output.assert_code(0);
    }
    let root = sandbox.repo("matcher-metrics", Some("https://github.com/owner/repo.git"));
    std::fs::write(root.join("a.rs"), "fn main() { changed(); }\n").unwrap();
    sandbox.run_at(&root, &["audit"]).assert_code(0);

    let dump = sandbox.run(&["telemetry", "dump"]);
    let dump: writ_core::TelemetryDump = serde_json::from_str(&dump.stdout).unwrap();
    for label in ["hit", "miss", "unevaluable"] {
        assert!(
            dump.counters.iter().any(|row| {
                row.metric == "matcher_result" && row.label == label && row.count == 1
            })
        );
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl From<std::process::Output> for Output {
    fn from(output: std::process::Output) -> Self {
        Self {
            code: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

impl Output {
    fn assert_code(&self, expected: i32) {
        assert_eq!(
            self.code, expected,
            "stdout: {}\nstderr: {}",
            self.stdout, self.stderr
        );
    }
}

fn run_with_stdin(mut command: Command, stdin: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A child that rejects flags (or otherwise exits before reading) closes
    // its end of the pipe. Writing then returns BrokenPipe; that is not a
    // test failure — the exit code and stderr still carry the answer.
    {
        let mut child_stdin = child.stdin.take().unwrap();
        match child_stdin.write_all(stdin.as_bytes()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(error) => panic!("writing stdin: {error}"),
        }
    }
    Output::from(child.wait_with_output().unwrap())
}

/// The first PATH entry holding an executable of this name.
///
/// It walks `PATH` rather than shelling out to `which`, because `which` is
/// itself a host binary and this function exists to stop the suite
/// depending on those.
fn which(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(tool);
        candidate.is_file().then_some(candidate)
    })
}

/// Run git with an identity and a configuration of its own.
///
/// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` are pointed at `/dev/null`
/// as well as the author fields. Without them the host's `~/.gitconfig`
/// reaches this git: `core.abbrev`, `diff.noprefix` and `diff.algorithm`
/// all change what `git diff` prints, and the prompt golden file asserts
/// that text byte for byte.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "writ tests")
        .env("GIT_AUTHOR_EMAIL", "tests@example.com")
        .env("GIT_COMMITTER_NAME", "writ tests")
        .env("GIT_COMMITTER_EMAIL", "tests@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write_gitconfig(path: &Path, email: &str) {
    std::fs::write(path, format!("[user]\n\temail = {email}\n")).unwrap();
}

// --- the flags that already existed -----------------------------------

#[test]
fn version_prints_the_package_version() {
    let output = writ().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), format!("writ {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_documents_the_path_overrides() {
    let output = writ().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--db"), "{stdout}");
    assert!(stdout.contains("--config"), "{stdout}");
}

#[test]
fn no_command_exits_two_and_says_so() {
    let output = writ().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("no command given"), "{stderr}");
}

#[test]
fn ui_help_lists_port_and_no_open() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["ui", "--help"]);
    output.assert_code(0);
    assert!(output.stdout.contains("--port"), "{}", output.stdout);
    assert!(output.stdout.contains("--no-open"), "{}", output.stdout);
}

/// P8: every flag section 5 lists for these two commands is a real flag.
#[test]
fn record_and_list_advertise_every_documented_flag() {
    let record =
        String::from_utf8(writ().args(["record", "--help"]).output().unwrap().stdout).unwrap();
    for flag in [
        "--title",
        "--rule",
        "--rationale",
        "--scope",
        "--advisory",
        "--example",
        "--matcher",
        "--matcher-kind",
        "--sides",
        "--status",
        "--activate",
        "--json",
        "--reinforce",
        "--format",
    ] {
        assert!(record.contains(flag), "record --help is missing {flag}");
    }

    let list = String::from_utf8(writ().args(["list", "--help"]).output().unwrap().stdout).unwrap();
    for flag in [
        "--status",
        "--scope",
        "--search",
        "--unused-days",
        "--never-applied",
        "--format",
    ] {
        assert!(list.contains(flag), "list --help is missing {flag}");
    }
}

// --- record -----------------------------------------------------------

#[test]
fn recording_writes_a_learning_and_says_what_it_wrote() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[]);
    assert_eq!(id.len(), 36, "the id is a UUID: {id}");

    let learnings = sandbox.learnings();
    assert_eq!(learnings.as_array().unwrap().len(), 1);
    assert_eq!(learnings[0]["id"], id);
    assert_eq!(learnings[0]["title"], "prefer sd");
    assert_eq!(learnings[0]["rationale"], "sed is terse");
}

#[test]
fn a_record_with_no_rationale_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["record", "--title", "t", "--rule", "r"]);
    output.assert_code(2);
    assert!(output.stderr.contains("rationale"), "{}", output.stderr);
    assert!(sandbox.learnings().as_array().unwrap().is_empty());
}

#[test]
fn an_empty_rationale_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "   ",
    ]);
    output.assert_code(2);
    assert!(output.stderr.contains("rationale"), "{}", output.stderr);
}

#[test]
fn status_defaults_to_proposed() {
    // Invariant 2: a caller that forgets --activate fails safe.
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    let learnings = sandbox.learnings();
    assert_eq!(learnings[0]["status"], "proposed");
    assert_eq!(learnings[0]["activated_at"], serde_json::Value::Null);
}

#[test]
fn activate_sets_the_status_and_the_activation_clock() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--activate"]);
    let learnings = sandbox.learnings();
    assert_eq!(learnings[0]["status"], "active");
    assert!(!learnings[0]["activated_at"].is_null());
}

#[test]
fn status_active_is_the_same_as_activate() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--status", "active"]);
    assert_eq!(sandbox.learnings()[0]["status"], "active");
}

#[test]
fn reactivating_does_not_move_activated_at() {
    // activated_at is the clock recurrence is measured against, and a
    // timestamp cannot be reconstructed. Spec section 6.
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--activate"]);
    let first = sandbox.learnings()[0]["activated_at"].clone();

    sandbox
        .run(&["record", "--reinforce", &id, "--activate"])
        .assert_code(0);

    let learnings = sandbox.learnings();
    assert_eq!(learnings.as_array().unwrap().len(), 1);
    assert_eq!(learnings[0]["activated_at"], first);
}

#[test]
fn a_write_cannot_ask_for_archived() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--status",
        "archived",
    ]);
    output.assert_code(2);
    assert!(output.stderr.contains("writ archive"), "{}", output.stderr);
}

#[test]
fn activate_and_status_together_are_refused() {
    let sandbox = Sandbox::new();
    sandbox
        .run(&[
            "record",
            "--title",
            "t",
            "--rule",
            "r",
            "--rationale",
            "why",
            "--activate",
            "--status",
            "proposed",
        ])
        .assert_code(2);
}

#[test]
fn no_scope_means_global() {
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    assert_eq!(sandbox.learnings()[0]["scopes"][0], "global");
}

#[test]
fn scopes_are_stored_as_given() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--scope", "language:rust", "--scope", "glob:**/*.rs"]);
    let scopes = sandbox.learnings()[0]["scopes"].clone();
    assert_eq!(scopes[0], "glob:**/*.rs");
    assert_eq!(scopes[1], "language:rust");
}

/// Section 7.1 step 2. Every other kind narrows a rule and `global` does
/// not, so the pair has no meaning and the write refuses it rather than
/// keeping half of it.
#[test]
fn global_with_another_scope_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--scope",
        "global",
        "--scope",
        "language:rust",
    ]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("cannot be combined"),
        "{}",
        output.stderr
    );
    assert!(sandbox.learnings().as_array().unwrap().is_empty());
}

/// A second scope narrows a rule. Under the old semantics it widened one,
/// so `project:X` plus `language:elixir` fired on markdown edits in X.
#[test]
fn a_second_scope_narrows_the_rule_rather_than_widening_it() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let narrow = sandbox.record(&[
        "--scope",
        "project:github.com/owner/repo",
        "--scope",
        "language:rust",
        "--activate",
    ]);

    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert!(
        audit_ids(&sandbox, &root, &[]).contains(&narrow),
        "a Rust file in the right repo matches both kinds"
    );

    // A markdown edit in the same repository matches the project kind and
    // fails the language kind, so the rule must not fire.
    std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("README.md"), "text\n").unwrap();
    git(&root, &["add", "-A"]);
    assert!(
        !audit_ids(&sandbox, &root, &["--diff", "HEAD"]).contains(&narrow),
        "a markdown edit fails the language kind"
    );
}

#[test]
fn an_unknown_scope_kind_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--scope",
        "team:core",
    ]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("global, project"),
        "{}",
        output.stderr
    );
}

#[test]
fn advisory_demotes_the_rule() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--advisory"]);
    assert_eq!(sandbox.learnings()[0]["blocking"], false);
}

#[test]
fn blocking_is_the_default() {
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    assert_eq!(sandbox.learnings()[0]["blocking"], true);
}

#[test]
fn an_example_copies_the_snippet_text_never_the_path() {
    // Invariant 3. The file name is deliberately searchable.
    let sandbox = Sandbox::new();
    let snippet = "fn main() {\n    let x = 1;\n}\n";
    sandbox.write_file("findme.rs", snippet);
    let id = sandbox.record(&[
        "--example",
        &format!("good:{}", sandbox.path("findme.rs").display()),
    ]);

    let dump = std::fs::read(sandbox.db()).unwrap();
    let dump = String::from_utf8_lossy(&dump);
    assert!(dump.contains("let x = 1;"), "the snippet was not stored");
    assert!(!dump.contains("findme.rs"), "a path reached the database");
    assert!(!id.is_empty());
}

/// Section 5: the snippet inline. An agent holds text, not a path, so
/// without this form MCP cannot attach an exemplar at all -- and an
/// exemplar is what makes a rule teach instead of assert.
#[test]
fn an_example_text_stores_the_snippet_it_was_given() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--example-text", "bad:sed -i '' s/a/b/ file"]);

    let shown = sandbox.run(&["show", &id, "--format", "json"]);
    shown.assert_code(0);
    let shown: serde_json::Value = serde_json::from_str(&shown.stdout).unwrap();
    let exemplars = shown["exemplars"].as_array().unwrap();
    assert_eq!(exemplars.len(), 1);
    assert_eq!(exemplars[0]["kind"], "bad");
    assert_eq!(exemplars[0]["snippet"], "sed -i '' s/a/b/ file");
}

/// Invariant 3 again, from the other direction: the inline form has no
/// path to leak, and it must not invent one.
#[test]
fn example_text_and_example_file_land_in_one_collection() {
    let sandbox = Sandbox::new();
    sandbox.write_file("findme.rs", "let x = 1;\n");
    let id = sandbox.record(&[
        "--example",
        &format!("bad:{}", sandbox.path("findme.rs").display()),
        "--example-text",
        "good:let x = 1usize;",
    ]);

    let shown = sandbox.run(&["show", &id, "--format", "json"]);
    shown.assert_code(0);
    let shown: serde_json::Value = serde_json::from_str(&shown.stdout).unwrap();
    let exemplars = shown["exemplars"].as_array().unwrap();
    assert_eq!(exemplars.len(), 2);
    let kinds: Vec<_> = exemplars
        .iter()
        .map(|one| one["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["bad", "good"]);

    let dump = std::fs::read(sandbox.db()).unwrap();
    assert!(!String::from_utf8_lossy(&dump).contains("findme.rs"));
}

#[test]
fn an_example_text_with_no_kind_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--example-text",
        "let x = 1;",
    ]);
    output.assert_code(2);
}

#[test]
fn an_empty_example_text_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--example-text",
        "good:",
    ]);
    output.assert_code(2);
}

#[test]
fn an_example_with_no_kind_is_refused() {
    let sandbox = Sandbox::new();
    let path = sandbox.write_file("snippet.rs", "let x = 1;");
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--example",
        path.to_str().unwrap(),
    ]);
    output.assert_code(2);
}

#[test]
fn an_example_file_that_is_not_there_names_the_path() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--example",
        "good:/no/such/file.rs",
    ]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("/no/such/file.rs"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_matcher_needs_its_kind() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--matcher",
        "$A == $A",
    ]);
    output.assert_code(2);
    assert!(output.stderr.contains("matcher-kind"), "{}", output.stderr);
}

#[test]
fn a_matcher_and_its_kind_are_stored() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--matcher", "$A == $A", "--matcher-kind", "ast_grep"]);
    let learnings = sandbox.learnings();
    assert_eq!(learnings[0]["matcher"], "$A == $A");
    assert_eq!(learnings[0]["matcher_kind"], "ast_grep");
}

#[test]
fn reinforcing_bumps_the_counter_rather_than_adding_a_row() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[]);
    let output = sandbox.run(&["record", "--reinforce", &id]);
    output.assert_code(0);
    assert!(output.stdout.contains("reinforced"), "{}", output.stdout);

    let learnings = sandbox.learnings();
    assert_eq!(learnings.as_array().unwrap().len(), 1);
    assert_eq!(learnings[0]["reinforced"], 1);
}

#[test]
fn reinforcing_an_unknown_id_exits_five() {
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    let output = sandbox.run(&[
        "record",
        "--reinforce",
        "01234567-89ab-7def-8000-000000000000",
    ]);
    output.assert_code(5);
    assert!(
        output.stderr.contains("no learning has id"),
        "{}",
        output.stderr
    );
}

#[test]
fn force_is_gone_and_is_now_a_usage_error() {
    // `--force` only ever bypassed the near-match block. Spec section 7.3
    // removed the block, so the flag went with it.
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--force",
    ]);
    output.assert_code(2);
    assert_eq!(sandbox.learnings().as_array().unwrap().len(), 0);
}

#[test]
fn no_write_prints_a_near_match_warning() {
    // Nothing detects duplicates any more. Spec section 7.3.
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    let output = sandbox.run(&[
        "record",
        "--title",
        "prefer sd",
        "--rule",
        "use sd",
        "--rationale",
        "sed is terse",
    ]);
    output.assert_code(0);
    assert!(!output.stderr.contains("near match"), "{}", output.stderr);
}

// --- author -----------------------------------------------------------

#[test]
fn the_configured_author_wins_over_git() {
    let sandbox = Sandbox::new();
    write_gitconfig(&sandbox.path("empty.gitconfig"), "git@example.com");
    sandbox.write_config("[identity]\nauthor = \"config@example.com\"\n");
    sandbox.record(&[]);
    assert_eq!(sandbox.learnings()[0]["author"], "config@example.com");
}

#[test]
fn git_answers_when_the_config_is_silent() {
    let sandbox = Sandbox::new();
    write_gitconfig(&sandbox.path("empty.gitconfig"), "git@example.com");
    sandbox.record(&[]);
    assert_eq!(sandbox.learnings()[0]["author"], "git@example.com");
}

#[test]
fn the_author_stays_null_when_neither_answers() {
    // Outside a git repository with no config key. Spec section 6.
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    assert_eq!(
        sandbox.learnings()[0]["author"],
        serde_json::Value::Null,
        "an invented author is worse than none"
    );
}

// --- record --json ----------------------------------------------------

const LINE: &str = r#"{"title":"imported","rule":"r","rationale":"why"}"#;

#[test]
fn a_json_stream_writes_every_line() {
    let sandbox = Sandbox::new();
    let output = sandbox.pipe(&["record", "--json"], &format!("{LINE}\n{LINE}\n"));
    output.assert_code(0);
    let learnings = sandbox.learnings();
    assert_eq!(learnings.as_array().unwrap().len(), 2);
    assert_eq!(learnings[0]["source_kind"], "import");
    assert_eq!(learnings[0]["status"], "proposed", "imports land proposed");
}

#[test]
fn a_json_stream_with_one_bad_line_writes_nothing_and_exits_four() {
    // The spec gives bad JSON its own code but does not say what a bulk
    // write does with one bad line. All or nothing is what makes a re-run
    // safe, and crit #446 refuses a silent partial.
    let sandbox = Sandbox::new();
    let output = sandbox.pipe(
        &["record", "--json"],
        &format!("{LINE}\nnot json\n{LINE}\n"),
    );
    output.assert_code(4);
    assert!(output.stderr.contains("line 2"), "{}", output.stderr);
    assert!(
        sandbox.learnings().as_array().unwrap().is_empty(),
        "a failed stream must write nothing"
    );
}

#[test]
fn a_json_line_missing_its_rationale_exits_two() {
    let sandbox = Sandbox::new();
    let bad = r#"{"title":"t","rule":"r"}"#;
    let output = sandbox.pipe(&["record", "--json"], &format!("{LINE}\n{bad}\n"));
    output.assert_code(2);
    assert!(output.stderr.contains("line 2"), "{}", output.stderr);
    assert!(sandbox.learnings().as_array().unwrap().is_empty());
}

#[test]
fn an_empty_json_stream_says_so() {
    let sandbox = Sandbox::new();
    let output = sandbox.pipe(&["record", "--json"], "");
    output.assert_code(2);
    assert!(output.stderr.contains("no learning"), "{}", output.stderr);
}

#[test]
fn a_json_import_keeps_the_timestamps_it_arrived_with() {
    // The one exception to invariant 7.
    let sandbox = Sandbox::new();
    let line = r#"{"title":"t","rule":"r","rationale":"why",
        "created_at":"2000-01-01 00:00:00","updated_at":"2000-01-02 00:00:00"}"#
        .replace('\n', " ");
    sandbox.pipe(&["record", "--json"], &line).assert_code(0);
    let learnings = sandbox.learnings();
    assert_eq!(learnings[0]["created_at"], "2000-01-01 00:00:00");
    assert_eq!(learnings[0]["updated_at"], "2000-01-02 00:00:00");
}

#[test]
fn a_json_stream_can_be_activated_wholesale() {
    let sandbox = Sandbox::new();
    sandbox
        .pipe(&["record", "--json", "--activate"], LINE)
        .assert_code(0);
    assert_eq!(sandbox.learnings()[0]["status"], "active");
}

#[test]
fn json_and_the_flag_form_cannot_be_mixed() {
    let sandbox = Sandbox::new();
    sandbox
        .pipe(&["record", "--json", "--title", "t"], LINE)
        .assert_code(2);
}

// --- list -------------------------------------------------------------

#[test]
fn listing_an_empty_collection_prints_nothing_and_succeeds() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["list"]);
    output.assert_code(0);
    assert_eq!(output.stdout, "");
}

#[test]
fn listing_filters_by_status() {
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    sandbox.record(&["--activate"]);
    let output = sandbox.run(&["list", "--status", "active", "--format", "json"]);
    output.assert_code(0);
    let learnings: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(learnings.as_array().unwrap().len(), 1);
}

#[test]
fn an_unknown_status_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["list", "--status", "pending"]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("unknown status"),
        "{}",
        output.stderr
    );
}

#[test]
fn listing_filters_by_scope() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--scope", "language:rust"]);
    sandbox.record(&[]);
    let output = sandbox.run(&["list", "--scope", "language:rust", "--format", "json"]);
    let learnings: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(learnings.as_array().unwrap().len(), 1);
}

/// Section 5 gives `writ list` two filters, one per axis. `--unused-days`
/// is reach, `--never-applied` is usefulness. A rule no audit ever reached
/// is deliberately absent from the second: it caught nothing trivially,
/// and "make it advisory" is the wrong advice for it.
#[test]
fn the_two_health_filters_measure_reach_and_usefulness() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let reached = sandbox.record(&["--activate"]);
    let misscoped = sandbox.record(&["--scope", "glob:web/**", "--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    // The glob never matches an `a.rs` change, so one rule is reached and
    // the other never is.
    assert_eq!(audit_ids(&sandbox, &root, &[]), vec![reached.clone()]);

    // Reach, with the boundary at zero so the just-selected rule counts.
    let unused = ids(
        &sandbox,
        &["list", "--unused-days", "0", "--format", "json"],
    );
    assert_eq!(unused.len(), 2, "both rules are in the reach bucket");

    // Usefulness. Only the rule an audit actually reached is asked to
    // justify itself.
    assert_eq!(
        ids(&sandbox, &["list", "--never-applied", "--format", "json"]),
        vec![reached],
        "a rule nothing ever selected is not the noisy one"
    );
    let _ = misscoped;
}

/// `times_selected` on the row is what separates a misscoped rule from a
/// dead one, which is why neither needs its own flag.
#[test]
fn the_row_says_whether_an_unused_rule_is_misscoped_or_dead() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let reached = sandbox.record(&["--activate"]);
    sandbox.record(&["--scope", "glob:web/**", "--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert_eq!(audit_ids(&sandbox, &root, &[]), vec![reached.clone()]);

    let rows = sandbox.learnings();
    let counts: Vec<i64> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|one| one["times_selected"].as_i64().unwrap())
        .collect();
    assert!(counts.contains(&0), "the misscoped rule reads 0");
    assert!(counts.contains(&1), "the reached rule reads 1");
}

/// A rule written today that no audit has reached is not unused. Without
/// the fallback to `created_at`, Health opens on what the user just wrote.
#[test]
fn a_rule_written_today_is_not_unused() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--activate"]);
    assert!(
        ids(
            &sandbox,
            &["list", "--unused-days", "1", "--format", "json"]
        )
        .is_empty(),
        "a rule no audit has reached falls back to when it was written"
    );
}

/// Both imply `--status active`. A proposed learning can never be
/// selected, so without this every pending rule sits in both buckets
/// forever and the Inbox leaks into Health.
#[test]
fn the_health_filters_skip_proposed_learnings_unless_asked() {
    let sandbox = Sandbox::new();
    let line = r#"{"title":"old proposal","rule":"r","rationale":"why",
        "created_at":"2000-01-01 00:00:00","updated_at":"2000-01-01 00:00:00"}"#
        .replace('\n', " ");
    sandbox.pipe(&["record", "--json"], &line).assert_code(0);

    assert!(
        ids(
            &sandbox,
            &["list", "--unused-days", "90", "--format", "json"]
        )
        .is_empty(),
        "a proposed learning is not a Health row"
    );

    // An explicit --status wins, because asking for it is asking for it.
    assert_eq!(
        ids(
            &sandbox,
            &[
                "list",
                "--status",
                "proposed",
                "--unused-days",
                "90",
                "--format",
                "json"
            ]
        )
        .len(),
        1
    );
}

/// Reach moves at emit. Usefulness moves at ingest. Section 6.
#[test]
fn emitting_moves_reach_and_ingesting_moves_usefulness() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    let after_emit = sandbox.learnings()[0].clone();
    assert_eq!(after_emit["times_selected"], 1);
    assert_ne!(after_emit["last_selected_at"], serde_json::Value::Null);
    assert_eq!(after_emit["times_applied"], 0);
    assert_eq!(after_emit["last_applied_at"], serde_json::Value::Null);

    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(r#"{{"audit_id":"{audit_id}","findings":[{{"learning_id":"{id}"}}]}}"#),
        )
        .assert_code(1);
    let after_ingest = sandbox.learnings()[0].clone();
    assert_eq!(
        after_ingest["times_selected"], 1,
        "ingest is not a selection"
    );
    assert_eq!(after_ingest["times_applied"], 1);
    assert_ne!(after_ingest["last_applied_at"], serde_json::Value::Null);
}

/// Spec section 5.7 code 8. A directory where the database file belongs
/// makes SQLite fail, and telling the user to check their flags would be
/// dressing one cause as another. P7.
#[test]
fn an_unopenable_database_exits_eight_and_names_the_path() {
    let sandbox = Sandbox::new();
    std::fs::create_dir(sandbox.db()).unwrap();
    let output = sandbox.run(&["list"]);
    output.assert_code(8);
    assert!(
        output.stderr.contains("learnings.db"),
        "the error must name the database: {}",
        output.stderr
    );
}

/// FTS5 `MATCH` takes a query language. Each of these characters means
/// something there, so raw input either errors or searches for the wrong
/// thing. Spec section 7.3.
#[test]
fn search_treats_fts5_operators_as_text() {
    let sandbox = Sandbox::new();
    sandbox
        .run(&[
            "record",
            "--title",
            "non empty",
            "--rule",
            "keep OR and NEAR",
            "--rationale",
            "star and quote",
        ])
        .assert_code(0);

    let hits = |query: &str| {
        let output = sandbox.run(&["list", "--search", query, "--format", "json"]);
        output.assert_code(0);
        let learnings: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
        learnings.as_array().unwrap().len()
    };

    assert_eq!(hits("non-empty"), 1, "a hyphen must not negate");
    assert_eq!(hits("\"empty\""), 1, "a quote must not open a phrase");
    assert_eq!(hits("empt*"), 0, "a star is not a prefix operator");
    assert_eq!(hits("OR"), 1, "OR is a word here");
    assert_eq!(hits("NEAR"), 1, "NEAR is a word here");
    assert_eq!(hits("a NEAR b"), 0, "NEAR is searched for, not obeyed");
    assert_eq!(hits("***"), 0, "no searchable term matches nothing");
}

#[test]
fn the_text_format_carries_the_id_status_and_title() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--activate", "--scope", "language:rust"]);
    let output = sandbox.run(&["list"]);
    output.assert_code(0);
    assert!(output.stdout.contains(&id), "{}", output.stdout);
    assert!(output.stdout.contains("active"), "{}", output.stdout);
    assert!(output.stdout.contains("language:rust"), "{}", output.stdout);
    assert!(output.stdout.contains("prefer sd"), "{}", output.stdout);
}

// --- configuration ----------------------------------------------------

#[test]
fn a_missing_config_file_is_not_an_error() {
    let sandbox = Sandbox::new();
    assert!(!sandbox.config().exists());
    sandbox.record(&[]);
}

/// One valid invocation of every subcommand in spec section 5.
///
/// The list is here rather than inline in each test so that a new
/// subcommand cannot quietly skip the config contract: adding a
/// `Command` variant without adding a row makes the count test fail.
const EVERY_SUBCOMMAND: &[&[&str]] = &[
    &[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
    ],
    &["list"],
    &["audit"],
    &["show", "01234567-89ab-7def-8000-000000000000"],
    &["archive", "01234567-89ab-7def-8000-000000000000"],
    &[
        "edit",
        "01234567-89ab-7def-8000-000000000000",
        "--title",
        "t",
    ],
    &["ui", "--no-open"],
    &["mcp"],
    // `--print` so that a valid-config variant of this list can never
    // reach the machine's own host configuration.
    &["install", "claude-code", "--print"],
    &["telemetry", "show"],
];

#[test]
fn every_subcommand_reads_the_config_file() {
    // crit #763, spec section 10: the configuration is resolved once,
    // before dispatch. A subcommand that parses it late, or not at all,
    // runs against a file the user never approved.
    let sandbox = Sandbox::new();
    sandbox.write_config("[audit\nmax_rules = 3\n");

    for args in EVERY_SUBCOMMAND {
        let output = sandbox.run(args);
        output.assert_code(2);
        assert!(
            output.stderr.contains("config.toml"),
            "`writ {}` ignored a malformed config: {}",
            args[0],
            output.stderr
        );
    }
}

#[test]
fn every_subcommand_refuses_an_unknown_config_key() {
    // The `[dedupe]` guard belongs at the level where it failed: the
    // binary, not `Config::parse`. Spec section 7.3.
    let sandbox = Sandbox::new();
    sandbox.write_config("[dedupe]\nwarn_top_n = 3\n");

    for args in EVERY_SUBCOMMAND {
        let output = sandbox.run(args);
        output.assert_code(2);
        assert!(
            output.stderr.contains("dedupe"),
            "`writ {}` accepted the removed [dedupe] block: {}",
            args[0],
            output.stderr
        );
    }
}

#[test]
fn the_subcommand_list_covers_every_subcommand() {
    // Guards the two tests above. `writ --help` is the binary's own list,
    // so a new subcommand shows up here before anyone remembers to add it.
    let help = String::from_utf8(writ().arg("--help").output().unwrap().stdout).unwrap();
    let listed: Vec<&str> = help
        .lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .skip(1)
        .take_while(|line| line.starts_with("  ") && !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .collect();

    for name in &listed {
        assert!(
            EVERY_SUBCOMMAND.iter().any(|args| args[0] == *name),
            "EVERY_SUBCOMMAND is missing `{name}`, so it skips the config contract"
        );
    }
    assert_eq!(listed.len(), EVERY_SUBCOMMAND.len(), "{listed:?}");
}

#[test]
fn a_malformed_config_file_names_itself() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[audit\nmax_rules = 3\n");
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
    ]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("config.toml"),
        "the error must name the file: {}",
        output.stderr
    );
}

#[test]
fn a_misspelled_config_key_is_refused_rather_than_ignored() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[audit]\nmax_rulez = 3\n");
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
    ]);
    output.assert_code(2);
}

// --- writ audit: scope resolution, section 7.1 step 1 -----------------

/// Identity is the normalized remote, so a repository checked out twice
/// under two names is still one project. Invariant 4.
#[test]
fn identity_is_the_remote_and_not_the_directory() {
    let sandbox = Sandbox::new();
    let first = sandbox.repo("one", Some("git@github.com:Owner/Repo.git"));
    let second = sandbox.repo("two", Some("https://user@github.com/owner/repo/"));
    std::fs::write(first.join("a.rs"), "fn main() { }\n").unwrap();
    std::fs::write(second.join("a.rs"), "fn main() { }\n").unwrap();

    for root in [&first, &second] {
        let report = audit_json(&sandbox, root, &[]);
        assert_eq!(report["repo"], "github.com/owner/repo");
        assert_eq!(report["repo_identity_is_path_fallback"], false);
    }
}

/// The worktree row of the section 7.1 table. This user works in many
/// worktrees of one repository, so a path-based identity would make
/// `project:` rules fire in the main checkout and miss everywhere else.
/// The test uses a repository with **no** remote, because that is the only
/// case where a path could creep in.
#[test]
fn a_worktree_is_the_same_project_as_its_main_checkout() {
    let sandbox = Sandbox::new();
    let main = sandbox.repo("main", None);
    let tree = sandbox.path("tree");
    git(&main, &["worktree", "add", "-q", tree.to_str().unwrap()]);
    std::fs::write(main.join("a.rs"), "fn main() { }\n").unwrap();
    std::fs::write(tree.join("a.rs"), "fn main() { }\n").unwrap();

    let from_main = audit_json(&sandbox, &main, &[]);
    let from_tree = audit_json(&sandbox, &tree, &[]);
    assert_eq!(from_main["repo"], from_tree["repo"]);
    assert_ne!(from_tree["repo"], tree.display().to_string());
}

/// The no-remote row. The audit must say so, because a `project:` scope
/// recorded against the remote identity will not match this one.
#[test]
fn a_repository_with_no_remote_falls_back_to_the_path_and_says_so() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("plain", None);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let output = sandbox.run_at(&root, &["audit", "--format", "json"]);
    output.assert_code(0);
    let report: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(report["repo_identity_is_path_fallback"], true);
    assert!(
        output.stderr.contains("no git remote"),
        "the fallback must be reported: {}",
        output.stderr
    );
}

/// The submodule row. Its own remote, so its own identity.
#[test]
fn a_submodule_is_its_own_project() {
    let sandbox = Sandbox::new();
    let child = sandbox.repo("child", Some("git@github.com:Owner/Child.git"));
    let parent = sandbox.repo("parent", Some("git@github.com:Owner/Parent.git"));
    git(
        &parent,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "--quiet",
            "add",
            child.to_str().unwrap(),
            "child",
        ],
    );
    let nested = parent.join("child");
    std::fs::write(nested.join("a.rs"), "fn main() { }\n").unwrap();
    std::fs::write(parent.join("a.rs"), "fn main() { }\n").unwrap();

    let outer = audit_json(&sandbox, &parent, &[]);
    let inner = audit_json(&sandbox, &nested, &[]);
    assert_eq!(outer["repo"], "github.com/owner/parent");
    // The submodule was added from a path on disk, so its own remote
    // names no host and it falls back. What matters is that it is not the
    // parent: a submodule is its own project.
    assert_ne!(inner["repo"], outer["repo"]);
    assert_eq!(inner["repo_identity_is_path_fallback"], true);
    assert!(
        inner["repo"].as_str().unwrap().ends_with("child"),
        "{inner}"
    );
}

/// The monorepo row. `project:` cannot separate `api/` from `web/`, so a
/// rule that needs to must use a `glob:` scope. Both halves are asserted:
/// the project scope catches everything, the glob scope catches one side.
#[test]
fn a_project_scope_cannot_separate_a_monorepo_subtree_but_a_glob_can() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("mono", Some("git@github.com:Owner/Mono.git"));
    for area in ["api", "web"] {
        std::fs::create_dir_all(root.join(area)).unwrap();
        std::fs::write(root.join(area).join("a.rs"), "fn main() {}\n").unwrap();
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two"]);

    let everywhere = sandbox.record(&["--scope", "project:github.com/owner/mono", "--activate"]);
    let api_only = sandbox.record(&["--scope", "glob:api/**", "--activate"]);

    std::fs::write(root.join("web/a.rs"), "fn main() { }\n").unwrap();
    let sent = audit_ids(&sandbox, &root, &[]);
    assert!(sent.contains(&everywhere), "the project scope must fire");
    assert!(
        !sent.contains(&api_only),
        "a glob on api/ must not fire on a web/ change"
    );

    std::fs::write(root.join("api/a.rs"), "fn main() { }\n").unwrap();
    let sent = audit_ids(&sandbox, &root, &[]);
    assert!(sent.contains(&api_only), "the glob must fire on api/");
}

// --- writ audit: selection, budget and the prompt ----------------------

#[test]
fn a_diff_that_matches_no_learning_sends_nothing_and_succeeds() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    sandbox.record(&["--scope", "language:elixir", "--activate"]);
    sandbox.record(&["--scope", "project:github.com/other/thing", "--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let output = sandbox.run_at(&root, &["audit"]);
    output.assert_code(0);
    assert!(
        output.stdout.contains("No learning applies to this diff."),
        "{}",
        output.stdout
    );
    assert_eq!(audit_json(&sandbox, &root, &[])["considered"], 0);
}

/// A proposed learning is not selected. Invariant 2 and P2: nothing
/// reaches the audit unapproved.
#[test]
fn a_proposed_learning_never_reaches_the_prompt() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let proposed = sandbox.record(&[]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert!(!audit_ids(&sandbox, &root, &[]).contains(&proposed));
}

/// Adapters parse this text, so it is asserted byte for byte. Only the two
/// UUIDv7 ids are replaced, because they are new on every run.
#[test]
fn the_prompt_is_asserted_byte_for_byte() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let good = sandbox.write_file("good.rs", "let y = 2;\n");
    let learning = sandbox.record(&[
        "--scope",
        "language:rust",
        "--example",
        &format!("good:{}", good.display()),
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();

    let output = sandbox.run_at(&root, &["audit", "--format", "prompt"]);
    output.assert_code(0);
    let audit_id = output
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("audit-id: "))
        .unwrap()
        .to_string();
    let actual = output
        .stdout
        .replace(&audit_id, "<AUDIT>")
        .replace(&learning, "<LEARNING>");
    let expected = include_str!("golden/audit_prompt.txt");
    assert_eq!(actual, expected, "the prompt is a contract");
}

/// The P3 contract, as the number section 11 asks for. A database of a
/// thousand learnings must produce the same prompt as one of fifty.
#[test]
fn a_thousand_learnings_produce_the_same_prompt_as_fifty() {
    let fifty = prompt_with_learnings(50);
    let thousand = prompt_with_learnings(1_000);
    assert_eq!(
        fifty.len(),
        thousand.len(),
        "audit cost must be bounded by the diff, not the collection"
    );
    assert_eq!(fifty.matches("\n### ").count(), 40, "the cap is max_rules");
    assert_eq!(thousand.matches("\n### ").count(), 40);
}

/// Build a collection of `count` interchangeable learnings and return the
/// prompt one audit produces from it. The titles are fixed width so the
/// only thing that could differ between two runs is how many rules the
/// budget let through.
fn prompt_with_learnings(count: usize) -> String {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let mut stream = String::new();
    for index in 0..count {
        stream.push_str(&format!(
            "{{\"title\":\"learning {index:04}\",\"rule\":\"rule {index:04}\",\
             \"rationale\":\"why {index:04}\",\"scopes\":[\"global\"],\"status\":\"active\"}}\n"
        ));
    }
    sandbox.pipe(&["record", "--json"], &stream).assert_code(0);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let output = sandbox.run_at(&root, &["audit"]);
    output.assert_code(0);
    // The ids differ between the two runs but never in length, so the
    // comparison the test makes is still a comparison of prompt size.
    output.stdout
}

#[test]
fn max_rules_and_max_chars_override_the_config() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    for _ in 0..5 {
        sandbox.record(&["--activate"]);
    }
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let capped = audit_json(&sandbox, &root, &["--max-rules", "2"]);
    assert_eq!(capped["considered"], 5);
    assert_eq!(capped["sent"], 2);

    let starved = audit_json(&sandbox, &root, &["--max-chars", "10"]);
    assert_eq!(starved["sent"], 0);
}

#[test]
fn the_config_budget_applies_when_no_flag_overrides_it() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[audit]\nmax_rules = 1\n");
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    for _ in 0..3 {
        sandbox.record(&["--activate"]);
    }
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert_eq!(audit_json(&sandbox, &root, &[])["sent"], 1);
}

/// Section 7.1 step 3: blocking first. An advisory rule with a spotless
/// record still ranks behind a blocking one.
#[test]
fn a_blocking_rule_takes_the_budget_before_an_advisory_one() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let advisory = sandbox.record(&["--advisory", "--activate"]);
    let blocking = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let sent = audit_ids(&sandbox, &root, &["--max-rules", "1"]);
    assert_eq!(sent, vec![blocking], "advisory {advisory} outranked it");
}

/// Section 7.5 weights the outcomes differently, so two rules that are
/// otherwise equal must separate on what happened to them. `ignored` is a
/// small negative and `fixed` is almost none.
#[test]
fn an_ignored_rule_ranks_below_one_the_agent_fixed() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let ignored = sandbox.record(&["--activate"]);
    let fixed = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    let findings = format!(
        r#"{{"audit_id":"{audit_id}","findings":[
             {{"learning_id":"{ignored}","outcome":"ignored"}},
             {{"learning_id":"{fixed}","outcome":"fixed"}}]}}"#
    );
    // The ignored one is blocking and not fixed, so the gate holds.
    sandbox
        .pipe(&["audit", "--ingest"], &findings)
        .assert_code(1);

    assert_eq!(
        audit_ids(&sandbox, &root, &["--max-rules", "1"]),
        vec![fixed]
    );
}

/// Section 7.1 step 5. The audited agent must not be able to write the
/// one outcome that both escapes the gate and demotes the rule.
#[test]
fn ingesting_a_rejected_outcome_exits_two() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = sandbox.pipe(
        &["audit", "--ingest"],
        &format!(
            r#"{{"audit_id":"{audit_id}","findings":[
                 {{"learning_id":"{id}","outcome":"rejected"}}]}}"#
        ),
    );
    output.assert_code(2);
    assert!(
        output.stderr.contains("Only a developer rejects"),
        "{}",
        output.stderr
    );
    // Nothing landed, so the rule was not demoted either.
    assert_eq!(sandbox.learnings()[0]["times_applied"], 0);
}

// --- writ audit: matchers, section 8.2 ---------------------------------

#[test]
fn a_matcher_that_hits_selects_the_learning() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&[
        "--scope",
        "language:rust",
        "--matcher",
        "let $A = $B;",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();

    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(output.stdout.contains(&learning), "{}", output.stdout);
    assert!(
        !output.stderr.contains("on scope alone"),
        "the matcher must have been evaluated, not skipped: {}",
        output.stderr
    );
}

#[test]
fn a_matcher_that_misses_drops_the_learning() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&[
        "--scope",
        "language:rust",
        "--matcher",
        "unsafe { $$$BODY }",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();

    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(!output.stdout.contains(&learning), "{}", output.stdout);
    // Without this the test passes on a machine with no ast-grep: an
    // unevaluable matcher keeps the rule, and the rule is then absent for
    // the wrong reason. That is the CI failure this test caused.
    assert!(
        !output.stderr.contains("on scope alone"),
        "the matcher must have been evaluated, not skipped: {}",
        output.stderr
    );
}

#[test]
fn an_ast_grep_matcher_hits_a_shape_removed_from_the_working_tree() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(
        root.join("a.ex"),
        "defmodule A do\n  @spec value() :: integer()\n  def value, do: 1\nend\n",
    )
    .unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "add spec"]);

    let learning = sandbox.record(&[
        "--scope",
        "language:elixir",
        "--matcher",
        "@spec $A",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(
        root.join("a.ex"),
        "defmodule A do\n  def value, do: 1\nend\n",
    )
    .unwrap();

    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(output.stdout.contains(&learning), "{}", output.stdout);
    assert!(
        !output.stderr.contains("on scope alone"),
        "the pre-image must have been evaluated: {}",
        output.stderr
    );
}

#[test]
fn sides_added_drops_an_ast_grep_removal_only_hit() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(
        root.join("a.ex"),
        "defmodule A do\n  @spec value() :: integer()\n  def value, do: 1\nend\n",
    )
    .unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "add spec"]);

    let learning = sandbox.record(&[
        "--scope",
        "language:elixir",
        "--matcher",
        "@spec $A",
        "--matcher-kind",
        "ast_grep",
        "--sides",
        "added",
        "--activate",
    ]);
    std::fs::write(
        root.join("a.ex"),
        "defmodule A do\n  def value, do: 1\nend\n",
    )
    .unwrap();

    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(
        !output.stdout.contains(&learning),
        "sides=added must ignore a pre-image-only hit: {}",
        output.stdout
    );
}

#[test]
fn an_ast_grep_miss_on_an_added_file_stays_a_miss_without_a_pre_image() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&[
        "--scope",
        "language:elixir",
        "--matcher",
        "@spec $A",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(
        root.join("new.ex"),
        "defmodule New do\n  def value, do: 1\nend\n",
    )
    .unwrap();
    git(&root, &["add", "new.ex"]);

    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(!output.stdout.contains(&learning), "{}", output.stdout);
    // Add-only diffs have no removed content, so the pre-image path is
    // skipped rather than emitting a fallback notice for every new file.
    assert!(
        !output.stderr.contains("cannot read pre-image HEAD:new.ex"),
        "add-only misses should not probe pre-images: {}",
        output.stderr
    );
}

/// A hit selects a learning. It does not create a finding: `times_applied`
/// only moves on ingest.
#[test]
fn a_matcher_hit_is_not_a_finding() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&[
        "--matcher",
        "let $A = $B;",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();
    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(output.stdout.contains(&learning), "{}", output.stdout);

    let after = &sandbox.learnings()[0];
    assert_eq!(after["times_applied"], 0, "a matcher hit is not a finding");
    assert_eq!(after["last_applied_at"], serde_json::Value::Null);
    // Reach did move: the rule was sent.
    assert_eq!(after["times_selected"], 1);
}

/// P6 and the section 11 CI check. Without `ast-grep` on PATH the learning
/// is kept on scope alone and the audit still exits `0`.
#[test]
fn an_absent_ast_grep_keeps_the_learning_and_exits_zero() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&[
        "--matcher",
        "unsafe { $$$BODY }",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();

    // git must still be reachable, or the audit would fail on step 1 for
    // a different reason. Only ast-grep is taken away.
    let mut command = sandbox.cmd_at(root.clone(), &["audit"]);
    command.env("PATH", sandbox.path_with(&["git"]));
    let output = Output::from(command.output().unwrap());
    output.assert_code(0);
    assert!(
        output.stdout.contains(&learning),
        "an unevaluable matcher must not drop the rule: {}",
        output.stdout
    );
    assert!(
        output.stderr.contains("on scope alone"),
        "and it must say why: {}",
        output.stderr
    );
}

/// A pattern ast-grep cannot parse is not an error to ast-grep: it prints
/// an empty result. Read as a miss, that would drop the rule silently.
#[test]
fn a_pattern_that_does_not_parse_keeps_the_learning_and_exits_zero() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&[
        "--scope",
        "language:rust",
        "--matcher",
        "((((",
        "--matcher-kind",
        "ast_grep",
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();

    let output = audit_with_ast_grep(&sandbox, &root, &[]);
    output.assert_code(0);
    assert!(output.stdout.contains(&learning), "{}", output.stdout);
    assert!(
        output.stderr.contains("does not parse"),
        "the reason must be the pattern, not a missing binary: {}",
        output.stderr
    );
}

#[test]
fn a_regex_matcher_reads_added_and_removed_lines() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(root.join("a.rs"), "fn main() { unwrap_me(); }\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two"]);

    let learning = sandbox.record(&[
        "--matcher",
        "unwrap_me",
        "--matcher-kind",
        "regex",
        "--activate",
    ]);
    // The removed shape is part of the diff, so the rule is in play.
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert!(audit_ids(&sandbox, &root, &[]).contains(&learning));

    std::fs::write(root.join("a.rs"), "fn main() { unwrap_me(); }\n").unwrap();
    git(&root, &["checkout", "-q", "--", "."]);
    std::fs::write(
        root.join("a.rs"),
        "fn main() { unwrap_me(); unwrap_me(); }\n",
    )
    .unwrap();
    assert!(audit_ids(&sandbox, &root, &[]).contains(&learning));
}

#[test]
fn sides_added_ignores_a_removal_only_regex_hit() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(root.join("a.rs"), "fn main() { unwrap_me(); }\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two"]);

    let learning = sandbox.record(&[
        "--matcher",
        "unwrap_me",
        "--matcher-kind",
        "regex",
        "--sides",
        "added",
        "--activate",
    ]);
    // Pure removal: the shape is only on the removed half.
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert!(
        !audit_ids(&sandbox, &root, &[]).contains(&learning),
        "sides=added must drop a removal-only hit"
    );
}

#[test]
fn sides_removed_ignores_an_addition_only_regex_hit() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two"]);

    let learning = sandbox.record(&[
        "--matcher",
        "unwrap_me",
        "--matcher-kind",
        "regex",
        "--sides",
        "removed",
        "--activate",
    ]);
    std::fs::write(root.join("a.rs"), "fn main() { unwrap_me(); }\n").unwrap();
    assert!(
        !audit_ids(&sandbox, &root, &[]).contains(&learning),
        "sides=removed must drop an addition-only hit"
    );
}

#[test]
fn sides_is_stored_and_editable() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--sides", "added", "--activate"]);
    assert_eq!(sandbox.learnings()[0]["sides"], "added");

    let id = sandbox.learnings()[0]["id"].as_str().unwrap().to_string();
    sandbox
        .run(&["edit", &id, "--sides", "removed"])
        .assert_code(0);
    assert_eq!(sandbox.learnings()[0]["sides"], "removed");
}

#[test]
fn sides_added_without_a_matcher_drops_a_removal_only_diff() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(root.join("a.rs"), "fn keep_me() {}\nfn main() {}\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two"]);

    let learning = sandbox.record(&["--sides", "added", "--activate"]);
    // Pure deletion: one line gone, nothing added.
    std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    assert!(
        !audit_ids(&sandbox, &root, &[]).contains(&learning),
        "a matcher-less sides=added rule must not burn budget on a pure deletion"
    );
}

#[test]
fn an_unknown_sides_value_is_a_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "record",
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--sides",
        "either",
    ]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("added, removed, or both"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_regex_that_does_not_compile_keeps_the_learning() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&["--matcher", "a(", "--matcher-kind", "regex", "--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let output = sandbox.run_at(&root, &["audit"]);
    output.assert_code(0);
    assert!(output.stdout.contains(&learning), "{}", output.stdout);
}

// --- writ audit: exit codes -------------------------------------------

/// Section 5.7 code 6. P7: a wrong working directory is not a usage error.
#[test]
fn auditing_outside_a_git_repository_exits_six() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["audit"]);
    output.assert_code(6);
    assert!(
        output.stderr.contains("not inside a git repository"),
        "{}",
        output.stderr
    );
}

/// Section 5.7 code 7. An empty diff must not read as a clean pass.
#[test]
fn an_empty_diff_exits_seven() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let output = sandbox.run_at(&root, &["audit"]);
    output.assert_code(7);
    assert!(output.stderr.contains("is empty"), "{}", output.stderr);
}

#[test]
fn a_named_range_is_diffed_instead_of_the_working_tree() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(root.join("b.ex"), "defmodule B do\nend\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two"]);

    // The working tree is clean, so only the range has anything in it.
    let report = audit_json(&sandbox, &root, &["--diff", "HEAD~1..HEAD"]);
    assert_eq!(report["diff_range"], "HEAD~1..HEAD");
    assert_eq!(report["considered"], 0);
}

/// `--dry-run` renders the same selection and writes nothing. Emit moves
/// up to `max_rules` counters, so seeing a selection had no cost-free
/// path before this flag.
#[test]
fn a_dry_run_selects_and_renders_but_writes_nothing() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let output = sandbox.run_at(&root, &["audit", "--dry-run"]);
    output.assert_code(0);
    assert!(output.stdout.contains(&id), "{}", output.stdout);

    let after = sandbox.learnings()[0].clone();
    assert_eq!(after["times_selected"], 0, "a dry run moves no counter");
    assert_eq!(after["last_selected_at"], serde_json::Value::Null);

    let report = audit_json(&sandbox, &root, &["--dry-run"]);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["sent"], 1);
}

/// The placeholder is the same width as a UUID, so a dry run previews the
/// prompt byte for byte rather than approximately. A preview that is five
/// bytes short of the real thing is not a preview.
#[test]
fn a_dry_run_prompt_is_the_same_size_as_the_real_one() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let dry = sandbox.run_at(&root, &["audit", "--dry-run"]);
    dry.assert_code(0);
    let real = sandbox.run_at(&root, &["audit"]);
    real.assert_code(0);
    assert_eq!(dry.stdout.len(), real.stdout.len());
}

/// A dry run records no audit, so its findings have nowhere to land. The
/// placeholder id makes that an honest exit `5` rather than an ingest
/// against some other audit.
#[test]
fn findings_from_a_dry_run_cannot_be_ingested() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let output = sandbox.run_at(&root, &["audit", "--dry-run", "--format", "json"]);
    output.assert_code(0);
    let report: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    let audit_id = report["audit_id"].as_str().unwrap().to_string();

    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(r#"{{"audit_id":"{audit_id}","findings":[{{"learning_id":"{id}"}}]}}"#),
        )
        .assert_code(5);
}

#[test]
fn a_dry_run_and_an_ingest_cannot_be_asked_for_together() {
    let sandbox = Sandbox::new();
    let output = sandbox.pipe(&["audit", "--dry-run", "--ingest"], "{}");
    output.assert_code(2);
}

// --- writ audit --ingest, section 7.1 steps 5 and 6 --------------------

#[test]
fn ingesting_writes_the_findings_and_moves_the_counters() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let learning = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();

    let before = sandbox.learnings()[0].clone();
    assert_eq!(before["times_applied"], 0);

    let findings = format!(
        r#"{{"audit_id":"{audit_id}","findings":[
             {{"learning_id":"{learning}","path":"a.rs","line":1,"detail":"here"}}]}}"#
    );
    let output = sandbox.pipe(&["audit", "--ingest", "--format", "json"], &findings);
    output.assert_code(1);
    let report: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(report["findings"], 1);
    assert_eq!(report["blocking"], 1);

    let after = sandbox.learnings()[0].clone();
    assert_eq!(after["times_applied"], 1);
    assert_ne!(after["last_applied_at"], serde_json::Value::Null);
    // Invariant 7: the trigger owns updated_at, and ingest does not touch
    // the learning's own text, so the row still moved through the trigger.
    assert_ne!(after["updated_at"], serde_json::Value::Null);
}

/// Section 7.1 step 6. This is the gate the Stop hook reads.
#[test]
fn a_blocking_finding_exits_one_and_an_advisory_one_exits_zero() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let blocking = sandbox.record(&["--activate"]);
    let advisory = sandbox.record(&["--advisory", "--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();

    let first = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(r#"{{"audit_id":"{first}","findings":[{{"learning_id":"{advisory}"}}]}}"#),
        )
        .assert_code(0);

    let second = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(r#"{{"audit_id":"{second}","findings":[{{"learning_id":"{blocking}"}}]}}"#),
        )
        .assert_code(1);
}

/// Section 7.1 step 6, amended. A blocking finding the agent fixed lets
/// the handoff through, or the Stop hook loop never terminates.
#[test]
fn a_blocking_finding_the_agent_fixed_exits_zero() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = sandbox.pipe(
        &["audit", "--ingest", "--format", "json"],
        &format!(
            r#"{{"audit_id":"{audit_id}","findings":[
                 {{"learning_id":"{id}","outcome":"fixed"}}]}}"#
        ),
    );
    output.assert_code(0);
    let report: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(report["blocking"], 1, "it was still a blocking finding");
    assert_eq!(report["unfixed_blocking"], 0);
}

/// An `ignored` blocking finding stops the handoff. Waving a violation
/// past by naming it is the case the gate exists for.
#[test]
fn a_blocking_finding_the_agent_ignored_exits_one() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(
                r#"{{"audit_id":"{audit_id}","findings":[
                     {{"learning_id":"{id}","outcome":"ignored"}}]}}"#
            ),
        )
        .assert_code(1);
}

#[test]
fn ingesting_no_findings_exits_zero() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(r#"{{"audit_id":"{audit_id}","findings":[]}}"#),
        )
        .assert_code(0);
}

/// Section 5.7 code 4. Malformed JSON is not a usage error.
#[test]
fn ingesting_malformed_json_exits_four() {
    let sandbox = Sandbox::new();
    let output = sandbox.pipe(&["audit", "--ingest"], "{not json");
    output.assert_code(4);
    assert!(
        output.stderr.contains("not valid JSON"),
        "{}",
        output.stderr
    );
}

#[test]
fn ingesting_an_unknown_audit_exits_five() {
    let sandbox = Sandbox::new();
    let output = sandbox.pipe(
        &["audit", "--ingest"],
        r#"{"audit_id":"nope","findings":[]}"#,
    );
    output.assert_code(5);
}

#[test]
fn ingesting_a_finding_for_an_unknown_learning_exits_five() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    let audit_id = audit_json(&sandbox, &root, &[])["audit_id"]
        .as_str()
        .unwrap()
        .to_string();
    sandbox
        .pipe(
            &["audit", "--ingest"],
            &format!(r#"{{"audit_id":"{audit_id}","findings":[{{"learning_id":"nope"}}]}}"#),
        )
        .assert_code(5);
}

#[test]
fn ingesting_nothing_at_all_says_so_rather_than_hanging() {
    let sandbox = Sandbox::new();
    sandbox.pipe(&["audit", "--ingest"], "").assert_code(2);
}

// --- writ show and writ archive ---------------------------------------

#[test]
fn show_prints_the_learning_and_its_exemplar() {
    let sandbox = Sandbox::new();
    let bad = sandbox.write_file("bad.rs", "let x = 1;\n");
    let id = sandbox.record(&[
        "--scope",
        "language:rust",
        "--example",
        &format!("bad:{}", bad.display()),
    ]);

    let output = sandbox.run(&["show", &id]);
    output.assert_code(0);
    assert!(output.stdout.contains(&id), "{}", output.stdout);
    assert!(output.stdout.contains("prefer sd"), "{}", output.stdout);
    assert!(output.stdout.contains("let x = 1;"), "{}", output.stdout);
    assert!(
        output.stdout.contains("status: proposed"),
        "{}",
        output.stdout
    );
}

#[test]
fn show_in_json_carries_the_learning_and_its_exemplars() {
    let sandbox = Sandbox::new();
    let bad = sandbox.write_file("bad.rs", "let x = 1;\n");
    let id = sandbox.record(&["--example", &format!("bad:{}", bad.display())]);

    let output = sandbox.run(&["show", &id, "--format", "json"]);
    output.assert_code(0);
    let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(value["learning"]["id"], id);
    assert_eq!(value["exemplars"][0]["snippet"], "let x = 1;\n");
    // Invariant 3: an exemplar carries no location, ever.
    assert!(value["exemplars"][0].get("path").is_none());
    assert!(value["exemplars"][0].get("line").is_none());
}

/// Section 5.7 code 5. Nothing else exercised this code before slice 3.
#[test]
fn showing_an_unknown_id_exits_five() {
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    let output = sandbox.run(&["show", "01900000-0000-7000-8000-000000000000"]);
    output.assert_code(5);
    assert!(
        output.stderr.contains("no learning has id"),
        "{}",
        output.stderr
    );
}

#[test]
fn archiving_an_unknown_id_exits_five() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["archive", "01900000-0000-7000-8000-000000000000"]);
    output.assert_code(5);
    assert!(
        output.stderr.contains("no learning has id"),
        "{}",
        output.stderr
    );
}

/// P4: pruning archives. The row stays, and it stops being selected.
#[test]
fn archiving_keeps_the_row_and_stops_it_being_selected() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    let id = sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert!(audit_ids(&sandbox, &root, &[]).contains(&id));

    let output = sandbox.run(&["archive", &id]);
    output.assert_code(0);
    assert!(output.stdout.contains("archived"), "{}", output.stdout);

    assert_eq!(sandbox.learnings()[0]["status"], "archived");
    assert!(!audit_ids(&sandbox, &root, &[]).contains(&id));
}

#[test]
fn archiving_twice_is_still_archived() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--activate"]);
    sandbox.run(&["archive", &id]).assert_code(0);
    sandbox.run(&["archive", &id]).assert_code(0);
    assert_eq!(sandbox.learnings()[0]["status"], "archived");
}

// --- writ edit --------------------------------------------------------

fn backdate_updated_at(sandbox: &Sandbox, id: &str) {
    let conn = rusqlite::Connection::open(sandbox.db()).unwrap();
    conn.execute(
        "UPDATE learnings SET updated_at = '2000-01-01 00:00:00' WHERE id = ?1",
        [id],
    )
    .unwrap();
}

#[test]
fn editing_matcher_only_updates_matcher_fields_and_updated_at() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[
        "--scope",
        "language:rust",
        "--matcher",
        "old",
        "--matcher-kind",
        "regex",
        "--example-text",
        "bad:old bad",
        "--activate",
    ]);
    backdate_updated_at(&sandbox, &id);

    let before: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();

    sandbox
        .run(&[
            "edit",
            &id,
            "--matcher",
            "$A == $A",
            "--matcher-kind",
            "ast_grep",
        ])
        .assert_code(0);

    let after: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();

    assert_eq!(after["learning"]["title"], before["learning"]["title"]);
    assert_eq!(after["learning"]["rule"], before["learning"]["rule"]);
    assert_eq!(
        after["learning"]["rationale"],
        before["learning"]["rationale"]
    );
    assert_eq!(
        after["learning"]["blocking"],
        before["learning"]["blocking"]
    );
    assert_eq!(after["learning"]["scopes"], before["learning"]["scopes"]);
    assert_eq!(
        after["exemplars"].as_array().unwrap().len(),
        before["exemplars"].as_array().unwrap().len()
    );
    assert_eq!(
        after["exemplars"][0]["snippet"],
        before["exemplars"][0]["snippet"]
    );
    assert_eq!(
        after["exemplars"][0]["kind"],
        before["exemplars"][0]["kind"]
    );
    assert_eq!(after["learning"]["matcher"], "$A == $A");
    assert_eq!(after["learning"]["matcher_kind"], "ast_grep");
    assert!(
        after["learning"]["updated_at"].as_str().unwrap()
            > before["learning"]["updated_at"].as_str().unwrap(),
        "updated_at must move: before={before}, after={after}",
    );
}

#[test]
fn omitting_matcher_leaves_it_untouched() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[
        "--matcher",
        "unwrap_me",
        "--matcher-kind",
        "regex",
        "--activate",
    ]);

    sandbox
        .run(&["edit", &id, "--title", "new title"])
        .assert_code(0);

    let after: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();
    assert_eq!(after["learning"]["matcher"], "unwrap_me");
    assert_eq!(after["learning"]["matcher_kind"], "regex");
    assert_eq!(after["learning"]["title"], "new title");
}

#[test]
fn clear_matcher_removes_matcher_and_kind() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[
        "--matcher",
        "unwrap_me",
        "--matcher-kind",
        "regex",
        "--activate",
    ]);

    sandbox
        .run(&["edit", &id, "--clear-matcher"])
        .assert_code(0);

    let after: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();
    assert!(after["learning"]["matcher"].is_null());
    assert!(after["learning"]["matcher_kind"].is_null());
}

#[test]
fn any_scope_replaces_the_full_set() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--scope", "language:rust", "--activate"]);

    sandbox
        .run(&["edit", &id, "--scope", "glob:**/*.rs"])
        .assert_code(0);

    let after: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();
    let scopes: Vec<&str> = after["learning"]["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(scopes, vec!["glob:**/*.rs"]);
}

#[test]
fn editing_an_unknown_id_exits_five() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "edit",
        "01900000-0000-7000-8000-000000000000",
        "--title",
        "t",
    ]);
    output.assert_code(5);
    assert!(
        output.stderr.contains("no learning has id"),
        "{}",
        output.stderr
    );
}

#[test]
fn an_empty_rationale_in_edit_is_refused() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[]);
    let output = sandbox.run(&["edit", &id, "--rationale", "   "]);
    output.assert_code(2);
    assert!(output.stderr.contains("rationale"), "{}", output.stderr);
}

#[test]
fn a_matcher_in_edit_needs_its_kind() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[]);
    let output = sandbox.run(&["edit", &id, "--matcher", "$A == $A"]);
    output.assert_code(2);
    assert!(output.stderr.contains("matcher-kind"), "{}", output.stderr);
}

#[test]
fn activate_on_proposed_makes_it_active() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[]);

    sandbox.run(&["edit", &id, "--activate"]).assert_code(0);

    let after: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();
    assert_eq!(after["learning"]["status"], "active");
    assert!(!after["learning"]["activated_at"].is_null());
}

#[test]
fn activate_on_archived_is_refused() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--activate"]);
    sandbox.run(&["archive", &id]).assert_code(0);

    let output = sandbox.run(&["edit", &id, "--activate"]);
    output.assert_code(2);
    assert!(output.stderr.contains("archived"), "{}", output.stderr);
}

#[test]
fn edit_with_no_mutating_flags_is_refused() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&[]);

    let output = sandbox.run(&["edit", &id]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("nothing to edit"),
        "{}",
        output.stderr
    );
}

#[test]
fn example_text_replaces_the_full_exemplar_set() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--example-text", "bad:old bad"]);

    sandbox
        .run(&["edit", &id, "--example-text", "good:let x = 1;"])
        .assert_code(0);

    let after: serde_json::Value =
        serde_json::from_str(&sandbox.run(&["show", &id, "--format", "json"]).stdout).unwrap();
    let exemplars = after["exemplars"].as_array().unwrap();
    assert_eq!(exemplars.len(), 1);
    assert_eq!(exemplars[0]["kind"], "good");
    assert_eq!(exemplars[0]["snippet"], "let x = 1;");
}

#[test]
fn blocking_and_advisory_can_be_toggled() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--advisory", "--activate"]);
    assert_eq!(sandbox.learnings()[0]["blocking"], false);

    sandbox.run(&["edit", &id, "--blocking"]).assert_code(0);
    assert_eq!(sandbox.learnings()[0]["blocking"], true);

    sandbox.run(&["edit", &id, "--advisory"]).assert_code(0);
    assert_eq!(sandbox.learnings()[0]["blocking"], false);
}

#[test]
fn global_with_another_scope_is_refused_in_edit() {
    let sandbox = Sandbox::new();
    let id = sandbox.record(&["--scope", "language:rust"]);

    let output = sandbox.run(&["edit", &id, "--scope", "global", "--scope", "language:rust"]);
    output.assert_code(2);
    assert!(
        output.stderr.contains("cannot be combined"),
        "{}",
        output.stderr
    );
}

// --- P8 and P7 ---------------------------------------------------------

/// P8: every flag section 5 lists for these four commands is a real flag.
#[test]
fn audit_show_archive_and_edit_advertise_every_documented_flag() {
    let audit =
        String::from_utf8(writ().args(["audit", "--help"]).output().unwrap().stdout).unwrap();
    for flag in [
        "--diff",
        "--format",
        "--ingest",
        "--max-rules",
        "--max-chars",
        "--dry-run",
    ] {
        assert!(
            audit.contains(flag),
            "writ audit --help lacks {flag}:\n{audit}"
        );
    }
    for command in ["show", "archive", "edit"] {
        let help =
            String::from_utf8(writ().args([command, "--help"]).output().unwrap().stdout).unwrap();
        assert!(
            help.contains("<ID>"),
            "writ {command} --help lacks ID:\n{help}"
        );
    }
    let edit = String::from_utf8(writ().args(["edit", "--help"]).output().unwrap().stdout).unwrap();
    for flag in [
        "--title",
        "--rule",
        "--rationale",
        "--scope",
        "--advisory",
        "--blocking",
        "--sides",
        "--matcher",
        "--matcher-kind",
        "--clear-matcher",
        "--example-text",
        "--activate",
        "--format",
    ] {
        assert!(
            edit.contains(flag),
            "writ edit --help lacks {flag}:\n{edit}"
        );
    }
}

/// The doubled path. A storage failure that prints its file twice reads as
/// two different failures, which is what P7 forbids.
#[test]
fn a_storage_failure_names_the_database_exactly_once() {
    let sandbox = Sandbox::new();
    std::fs::create_dir(sandbox.db()).unwrap();
    let output = sandbox.run(&["list"]);
    output.assert_code(8);
    let path = sandbox.db().display().to_string();
    assert_eq!(
        output.stderr.matches(&path).count(),
        1,
        "the database path must appear once: {}",
        output.stderr
    );
}

// --- helpers -----------------------------------------------------------

/// Run `writ audit` with `ast-grep` guaranteed present.
///
/// The matcher tests must exercise the matcher, not whatever the host has
/// installed. `path_with` panics when the binary is missing, so a clean
/// runner fails loudly here instead of quietly taking the P6 degradation
/// path and making a "the matcher missed" assertion pass for the wrong
/// reason. That is exactly how this suite broke in CI.
fn audit_with_ast_grep(sandbox: &Sandbox, root: &Path, args: &[&str]) -> Output {
    let mut all = vec!["audit"];
    all.extend_from_slice(args);
    let mut command = sandbox.cmd_at(root.to_path_buf(), &all);
    command.env("PATH", sandbox.path_with(&["git", "ast-grep"]));
    Output::from(command.output().unwrap())
}

/// Run an audit in `root` and read its JSON report.
fn audit_json(sandbox: &Sandbox, root: &Path, args: &[&str]) -> serde_json::Value {
    let mut all = vec!["audit", "--format", "json"];
    all.extend_from_slice(args);
    let output = sandbox.run_at(root, &all);
    output.assert_code(0);
    serde_json::from_str(&output.stdout).unwrap()
}

/// The ids a `writ list --format json` run returned.
fn ids(sandbox: &Sandbox, args: &[&str]) -> Vec<String> {
    let output = sandbox.run(args);
    output.assert_code(0);
    let learnings: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    learnings
        .as_array()
        .unwrap()
        .iter()
        .map(|one| one["id"].as_str().unwrap().to_string())
        .collect()
}

/// The ids of the learnings one audit sent.
fn audit_ids(sandbox: &Sandbox, root: &Path, args: &[&str]) -> Vec<String> {
    audit_json(sandbox, root, args)["learnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|one| one["id"].as_str().unwrap().to_string())
        .collect()
}

// --- writ audit --hook: the gate, spec section 9.2 ---------------------

/// A repository with one uncommitted change and one active learning that
/// applies to it, so an audit selects something.
fn gated_repo(sandbox: &Sandbox, blocking: bool) -> PathBuf {
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();
    let mut args = vec!["--activate"];
    if !blocking {
        args.push("--advisory");
    }
    sandbox.record(&args);
    root
}

/// Run `writ audit --hook HOST` in `root` with a host payload on stdin.
fn hook(sandbox: &Sandbox, root: &Path, host: &str, stdin: &str) -> Output {
    let command = sandbox.cmd_at(root.to_path_buf(), &["audit", "--hook", host]);
    run_with_stdin(command, stdin)
}

#[test]
fn the_claude_code_hook_blocks_with_exit_two_and_a_pointer_on_stderr() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "claude-code", "{}");

    output.assert_code(2);
    assert!(output.stdout.is_empty(), "stdout: {}", output.stdout);
    assert!(output.stderr.contains("writ_audit"), "{}", output.stderr);
    assert!(output.stderr.contains("audit-id: "), "{}", output.stderr);
}

/// Section 9.2. The whole point of the pointer: the host renders a Stop
/// hook's stderr into the transcript verbatim, so the diff must not be
/// there. It reaches the agent through the tool result instead, which the
/// host collapses.
#[test]
fn the_gate_never_puts_the_diff_in_the_host_protocol() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let claude = hook(&sandbox, &root, "claude-code", "{}");
    assert!(!claude.stderr.contains("```diff"), "{}", claude.stderr);
    assert!(!claude.stderr.contains("prefer sd"), "{}", claude.stderr);

    let codex = hook(&sandbox, &root, "codex", "{}");
    assert!(!codex.stdout.contains("```diff"), "{}", codex.stdout);

    let cursor = hook(&sandbox, &root, "cursor", "{}");
    assert!(!cursor.stdout.contains("```diff"), "{}", cursor.stdout);
}

/// Adapters parse this text too, so it is asserted byte for byte beside
/// the prompt it points at.
#[test]
fn the_pointer_is_asserted_byte_for_byte() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "claude-code", "{}");
    output.assert_code(2);

    let audit_id = audit_id_of(&output.stderr);
    let actual = output.stderr.replace(&audit_id, "<AUDIT>");
    assert_eq!(
        actual,
        include_str!("golden/audit_pointer.txt"),
        "the pointer is a contract"
    );
}

/// The audit-id a pointer or a prompt printed.
fn audit_id_of(text: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("audit-id: "))
        .expect("an audit-id line")
        .to_string()
}

/// `--fetch` hands back the prompt the gate recorded, diff and all. The
/// gate's stderr stays small; this is where the payload lives.
#[test]
fn fetch_returns_the_prompt_the_gate_recorded() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);
    let gate = hook(&sandbox, &root, "claude-code", "{}");
    gate.assert_code(2);
    let audit_id = audit_id_of(&gate.stderr);

    let fetched = sandbox.run_at(&root, &["audit", "--fetch", &audit_id]);

    fetched.assert_code(0);
    assert!(fetched.stdout.contains("```diff"), "{}", fetched.stdout);
    assert!(fetched.stdout.contains("prefer sd"), "{}", fetched.stdout);
    assert!(
        fetched.stdout.contains(&format!("audit-id: {audit_id}")),
        "{}",
        fetched.stdout
    );
}

/// The prompt is recorded for every audit, not only a gated one, so
/// `--fetch` answers for a plain `writ audit` too.
#[test]
fn fetch_answers_for_an_audit_run_by_hand() {
    let sandbox = Sandbox::new();
    let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
    sandbox.record(&["--activate"]);
    std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();

    let emitted = sandbox.run_at(&root, &["audit", "--format", "prompt"]);
    emitted.assert_code(0);
    let audit_id = audit_id_of(&emitted.stdout);

    let fetched = sandbox.run_at(&root, &["audit", "--fetch", &audit_id]);

    fetched.assert_code(0);
    assert_eq!(fetched.stdout, emitted.stdout, "a fetch is not a re-render");
}

/// `times_selected` moves at emit and nowhere else. A fetch is a read of
/// what emit already recorded, so fetching twice must not make a rule look
/// as though it reached three reviewers.
#[test]
fn fetch_does_not_move_times_selected() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);
    let gate = hook(&sandbox, &root, "claude-code", "{}");
    gate.assert_code(2);
    let audit_id = audit_id_of(&gate.stderr);
    let after_gate = times_selected(&sandbox);

    sandbox
        .run_at(&root, &["audit", "--fetch", &audit_id])
        .assert_code(0);
    sandbox
        .run_at(&root, &["audit", "--fetch", &audit_id])
        .assert_code(0);

    assert_eq!(times_selected(&sandbox), after_gate);
}

/// `times_selected` on the one learning `gated_repo` recorded.
fn times_selected(sandbox: &Sandbox) -> i64 {
    sandbox.learnings()[0]["times_selected"]
        .as_i64()
        .expect("a count")
}

/// P7: an id that names no audit is `not found`, not a usage error and
/// not an empty document.
#[test]
fn fetch_of_an_unknown_audit_exits_five() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = sandbox.run_at(&root, &["audit", "--fetch", "no-such-audit"]);

    output.assert_code(5);
}

/// A dry run records no `audits` row, so there is nothing to fetch. It
/// must say `not found` rather than hand back some other audit's prompt.
#[test]
fn fetch_of_a_dry_run_exits_five() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);
    let dry = sandbox.run_at(&root, &["audit", "--dry-run"]);
    dry.assert_code(0);

    let output = sandbox.run_at(&root, &["audit", "--fetch", &audit_id_of(&dry.stdout)]);

    output.assert_code(5);
}

/// A fetch reads a row. It needs no diff and no repository, so it works
/// from anywhere the database is reachable.
#[test]
fn fetch_needs_neither_a_diff_nor_a_repository() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);
    let gate = hook(&sandbox, &root, "claude-code", "{}");
    gate.assert_code(2);
    let audit_id = audit_id_of(&gate.stderr);

    let output = sandbox.run_at(sandbox.dir.path(), &["audit", "--fetch", &audit_id]);

    output.assert_code(0);
    assert!(output.stdout.contains("prefer sd"), "{}", output.stdout);
}

#[test]
fn the_codex_hook_blocks_on_stdout_with_exit_zero() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "codex", "{}");

    output.assert_code(0);
    let body: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(body["decision"], "block");
    assert!(
        body["reason"].as_str().unwrap().contains("writ_audit"),
        "{body}"
    );
}

#[test]
fn the_cursor_hook_submits_a_followup_message() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "cursor", "{}");

    output.assert_code(0);
    let body: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert!(
        body["followup_message"]
            .as_str()
            .unwrap()
            .contains("writ_audit"),
        "{body}"
    );
    assert!(body.get("decision").is_none(), "{body}");
}

/// Section 9.2: hook entry gates on **any** selected learning, blocking or
/// advisory. `blocking` decides whether an unfixed violation stops the
/// work at ingest, not whether the agent is sent back to review. Gating
/// entry on `blocking` would select an advisory learning, count it, and
/// drop it unread.
#[test]
fn an_advisory_selection_still_sends_the_agent_back() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, false);

    let claude = hook(&sandbox, &root, "claude-code", "{}");
    claude.assert_code(2);
    assert!(claude.stderr.contains("writ_audit"), "{}", claude.stderr);

    let cursor = hook(&sandbox, &root, "cursor", "{}");
    cursor.assert_code(0);
    assert!(
        cursor.stdout.contains("followup_message"),
        "{}",
        cursor.stdout
    );
}

/// Nothing selected is the only pass-through. There is nothing to hand
/// the agent, so the turn ends.
#[test]
fn an_empty_selection_passes_every_host_through() {
    for host in ["claude-code", "codex", "cursor"] {
        let sandbox = Sandbox::new();
        let root = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));
        std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();
        // Recorded, and never activated, so no audit can select it.
        sandbox.record(&[]);

        let output = hook(&sandbox, &root, host, "{}");

        output.assert_code(0);
        assert!(output.stdout.is_empty(), "{host} stdout: {}", output.stdout);
    }
}

/// The retry cap of section 9.2. Without it the `ignored` case loops
/// forever, because writ sees every audit fresh and reports the same
/// blocking learning again.
#[test]
fn stop_hook_active_stops_the_second_block() {
    for host in ["claude-code", "codex"] {
        let sandbox = Sandbox::new();
        let root = gated_repo(&sandbox, true);

        let output = hook(&sandbox, &root, host, r#"{"stop_hook_active": true}"#);

        output.assert_code(0);
        assert!(output.stdout.is_empty(), "{host} stdout: {}", output.stdout);
    }
}

#[test]
fn cursor_stops_blocking_at_the_loop_limit() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let under = hook(&sandbox, &root, "cursor", r#"{"loop_count": 4}"#);
    under.assert_code(0);
    assert!(
        under.stdout.contains("followup_message"),
        "{}",
        under.stdout
    );

    let at_cap = hook(&sandbox, &root, "cursor", r#"{"loop_count": 5}"#);
    at_cap.assert_code(0);
    assert!(at_cap.stdout.is_empty(), "{}", at_cap.stdout);
}

#[test]
fn the_cursor_loop_limit_is_overridable() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let command = sandbox.cmd_at(
        root.to_path_buf(),
        &["audit", "--hook", "cursor", "--loop-limit", "2"],
    );
    let output = run_with_stdin(command, r#"{"loop_count": 2}"#);

    output.assert_code(0);
    assert!(output.stdout.is_empty(), "{}", output.stdout);
}

/// A Stop hook fires after every turn, including turns that wrote no code.
/// Exiting 6 or 7 there would put a red error in front of the user on each
/// one. There is nothing to gate, so the gate passes and says why.
#[test]
fn a_hook_with_nothing_to_audit_passes_through_and_says_so() {
    let sandbox = Sandbox::new();
    let clean = sandbox.repo("repo", Some("git@github.com:Owner/Repo.git"));

    let empty = hook(&sandbox, &clean, "claude-code", "{}");
    empty.assert_code(0);
    assert!(empty.stdout.is_empty(), "{}", empty.stdout);
    assert!(empty.stderr.contains("is empty"), "{}", empty.stderr);

    let outside = hook(&sandbox, sandbox.dir.path(), "codex", "{}");
    outside.assert_code(0);
    assert!(outside.stdout.is_empty(), "{}", outside.stdout);
}

/// Never hang on stdin. crit #693 applied to the gate: a host that sends
/// no payload must not stall the turn forever.
#[test]
fn a_hook_with_no_payload_still_runs() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "claude-code", "");

    output.assert_code(2);
}

/// What the host left on the gate's stdin.
enum HookStdin {
    /// No stdin at all. End of file at once.
    Closed,
    /// A pipe nobody writes to and nobody closes, which is what a Stop
    /// hook inherits from a session that holds its own stdin open.
    Silent,
    /// A host payload, written and then closed.
    Payload(&'static str),
}

/// Run the gate with a stdin the test controls, under a hard time bound.
///
/// The bound is the assertion. A regression in the stdin path is a hang,
/// and a test that hangs on regression hangs CI instead of failing it.
fn hook_stdin(sandbox: &Sandbox, root: &Path, host: &str, stdin: HookStdin) -> Output {
    let mut command = sandbox.cmd_at(root.to_path_buf(), &["audit", "--hook", host]);
    command
        .stdin(match stdin {
            HookStdin::Closed => Stdio::null(),
            HookStdin::Silent | HookStdin::Payload(_) => Stdio::piped(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();

    // Held for the whole run in the silent case, so the write end stays
    // open and the child never sees end of file.
    let mut held = child.stdin.take();
    if let (HookStdin::Payload(text), Some(pipe)) = (&stdin, held.as_mut()) {
        pipe.write_all(text.as_bytes()).unwrap();
    }
    if !matches!(stdin, HookStdin::Silent) {
        held = None;
    }

    let output = wait_bounded(&mut child, Duration::from_secs(30), host);
    drop(held);
    output
}

fn wait_bounded(child: &mut Child, limit: Duration, what: &str) -> Output {
    let deadline = Instant::now() + limit;
    loop {
        if child.try_wait().unwrap().is_some() {
            let mut stdout = String::new();
            let mut stderr = String::new();
            std::io::Read::read_to_string(child.stdout.as_mut().unwrap(), &mut stdout).unwrap();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut stderr).unwrap();
            return Output {
                code: child.wait().unwrap().code().unwrap(),
                stdout,
                stderr,
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("writ audit --hook {what} did not exit within {limit:?}: it blocked on stdin");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Assert the gate blocked, in whichever protocol this host speaks.
fn assert_blocked(host: &str, output: &Output) {
    match host {
        "claude-code" => {
            output.assert_code(2);
            assert!(output.stderr.contains("writ_audit"), "{}", output.stderr);
        }
        "codex" => {
            output.assert_code(0);
            let body: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(body["decision"], "block");
        }
        "cursor" => {
            output.assert_code(0);
            let body: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert!(body["followup_message"].is_string(), "{body}");
        }
        other => panic!("no protocol for {other}"),
    }
}

/// P7: never hang on stdin, and section 11 lists the gate beside
/// `--ingest` and `record --json`.
///
/// `--hook` differs from those two in what absent stdin means. They need
/// their document and exit with usage without it. A hook legitimately runs
/// with no payload on some hosts, so the gate treats absent or empty stdin
/// as "no retry signal" and carries on. What it must never do is wait.
#[test]
fn the_gate_never_waits_on_stdin() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    for host in ["claude-code", "codex", "cursor"] {
        // No stdin at all.
        assert_blocked(host, &hook_stdin(&sandbox, &root, host, HookStdin::Closed));

        // A pipe that is open, empty, and never closed. Before the
        // deadline existed this read parked until the session ended.
        assert_blocked(host, &hook_stdin(&sandbox, &root, host, HookStdin::Silent));

        // A payload still reaches the retry signal, so the bound did not
        // buy the pass-through by throwing stdin away.
        let signal = match host {
            "cursor" => r#"{"loop_count": 5}"#,
            _ => r#"{"stop_hook_active": true}"#,
        };
        let spent = hook_stdin(&sandbox, &root, host, HookStdin::Payload(signal));
        spent.assert_code(0);
        assert!(spent.stdout.is_empty(), "{}", spent.stdout);
        assert!(spent.stderr.contains("retry cap"), "{}", spent.stderr);
    }
}

/// A payload writ cannot parse is not a reason to skip the gate.
#[test]
fn an_unreadable_hook_payload_still_blocks() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "claude-code", "not json");

    output.assert_code(2);
    assert!(output.stderr.contains("writ_audit"), "{}", output.stderr);
}

/// An unknown host is a usage error, not a silent pass.
#[test]
fn an_unknown_hook_host_exits_two() {
    let sandbox = Sandbox::new();
    let root = gated_repo(&sandbox, true);

    let output = hook(&sandbox, &root, "opencode", "{}");

    output.assert_code(2);
}

/// Both want stdin, and they read different documents.
#[test]
fn hook_and_ingest_conflict() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["audit", "--hook", "codex", "--ingest"]);
    output.assert_code(2);
}
