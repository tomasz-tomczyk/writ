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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
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
fn a_regex_matcher_reads_the_added_lines_only() {
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
    // The shape leaves the tree, so the rule about it is not in play.
    std::fs::write(root.join("a.rs"), "fn main() { }\n").unwrap();
    assert!(!audit_ids(&sandbox, &root, &[]).contains(&learning));

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

// --- P8 and P7 ---------------------------------------------------------

/// P8: every flag section 5 lists for these three commands is a real flag.
#[test]
fn audit_show_and_archive_advertise_every_documented_flag() {
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
    for command in ["show", "archive"] {
        let help =
            String::from_utf8(writ().args([command, "--help"]).output().unwrap().stdout).unwrap();
        assert!(
            help.contains("<ID>"),
            "writ {command} --help lacks ID:\n{help}"
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
