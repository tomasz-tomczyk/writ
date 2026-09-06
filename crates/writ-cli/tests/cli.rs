use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

    fn write_config(&self, text: &str) {
        std::fs::write(self.config(), text).unwrap();
    }

    fn write_file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut command = writ();
        command
            .arg("--db")
            .arg(self.db())
            .arg("--config")
            .arg(self.config())
            .args(args)
            // Run outside any git repository, so only the config files below
            // can answer `git config user.email`.
            .current_dir(self.dir.path())
            .env("GIT_CONFIG_GLOBAL", self.path("empty.gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.path("empty.gitconfig"))
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("EMAIL");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        Output::from(self.cmd(args).output().unwrap())
    }

    fn pipe(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = self
            .cmd(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        Output::from(child.wait_with_output().unwrap())
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
        "--status",
        "--activate",
        "--json",
        "--force",
        "--reinforce",
        "--format",
    ] {
        assert!(record.contains(flag), "record --help is missing {flag}");
    }

    let list = String::from_utf8(writ().args(["list", "--help"]).output().unwrap().stdout).unwrap();
    for flag in [
        "--status",
        "--scope",
        "--stale-days",
        "--search",
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
fn a_near_match_warns_and_the_write_still_happens() {
    // MVP warns and always writes. Spec section 7.3.
    let sandbox = Sandbox::new();
    sandbox.record(&[]);
    let output = sandbox
        .cmd(&[
            "record",
            "--title",
            "prefer sd",
            "--rule",
            "use sd",
            "--rationale",
            "sed is terse",
        ])
        .output()
        .unwrap();
    let output = Output::from(output);
    output.assert_code(0);
    assert!(output.stderr.contains("near match"), "{}", output.stderr);
    assert_eq!(sandbox.learnings().as_array().unwrap().len(), 2);
}

#[test]
fn warn_top_n_zero_turns_the_warning_off() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[dedupe]\nwarn_top_n = 0\n");
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

#[test]
fn force_is_accepted_and_changes_nothing() {
    let sandbox = Sandbox::new();
    sandbox.record(&["--force"]);
    assert_eq!(sandbox.learnings().as_array().unwrap().len(), 1);
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

/// Section 9.3 defines two Health buckets. Age is not use: a learning
/// created years ago that no audit ever selected is never-used, not stale.
#[test]
fn the_two_health_buckets_are_disjoint() {
    let sandbox = Sandbox::new();
    let line = r#"{"title":"old","rule":"r","rationale":"why",
        "created_at":"2000-01-01 00:00:00","updated_at":"2000-01-01 00:00:00"}"#
        .replace('\n', " ");
    sandbox.pipe(&["record", "--json"], &line).assert_code(0);

    let stale = sandbox.run(&["list", "--stale-days", "90", "--format", "json"]);
    stale.assert_code(0);
    let learnings: serde_json::Value = serde_json::from_str(&stale.stdout).unwrap();
    assert!(
        learnings.as_array().unwrap().is_empty(),
        "a never-used learning is not stale"
    );

    let unused = sandbox.run(&["list", "--never-used", "--format", "json"]);
    unused.assert_code(0);
    let learnings: serde_json::Value = serde_json::from_str(&unused.stdout).unwrap();
    assert_eq!(learnings.as_array().unwrap().len(), 1);
    assert_eq!(learnings[0]["title"], "old");
}

#[test]
fn asking_for_both_health_buckets_is_refused() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["list", "--never-used", "--stale-days", "90"]);
    output.assert_code(2);
    assert!(output.stderr.contains("disjoint"), "{}", output.stderr);
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

#[test]
fn a_malformed_config_file_names_itself() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[dedupe\nwarn_top_n = 3\n");
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
    sandbox.write_config("[dedupe]\nwarn_top = 3\n");
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
