//! Spec section 11 item 5: CLI and MCP parity.
//!
//! Two claims, and both are checked rather than asserted in a comment.
//! Every MCP tool argument names a real CLI flag, and the same input gives
//! the same result on both surfaces.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;
use writ_cli::mcp::{Kind, TOOLS, Tool};

fn writ() -> Command {
    Command::new(env!("CARGO_BIN_EXE_writ"))
}

// --- parity claim 1: every argument is a flag ---------------------------

/// One subcommand's clap definition, built the way the binary builds it.
fn subcommand(command: &'static str) -> clap::Command {
    let base = clap::Command::new(command);
    match command {
        "record" => <writ_cli::record::Args as clap::Args>::augment_args(base),
        "audit" => <writ_cli::audit::Args as clap::Args>::augment_args(base),
        other => panic!("no such subcommand {other}"),
    }
}

/// The long flags one subcommand accepts.
fn long_flags(command: &'static str) -> Vec<String> {
    subcommand(command)
        .get_arguments()
        .filter_map(|arg| arg.get_long().map(str::to_string))
        .collect()
}

#[test]
fn every_mcp_argument_maps_to_a_cli_flag() {
    for tool in TOOLS {
        let flags = long_flags(tool.command);
        for arg in tool.args {
            assert!(
                flags.contains(&arg.flag.to_string()),
                "{}.{} claims --{}, which writ {} does not accept. Flags: {flags:?}",
                tool.name,
                arg.name,
                arg.flag,
                tool.command,
            );
        }
    }
}

/// A flag that takes a value must not be declared as a boolean, and a
/// boolean must not be declared as a value. Either way round the argv the
/// tool builds would not parse, and the mistake would only show at run
/// time in whichever host tried it first.
#[test]
fn every_mcp_argument_has_the_arity_its_flag_has() {
    for tool in TOOLS {
        let built = subcommand(tool.command);
        for arg in tool.args {
            let found = built
                .get_arguments()
                .find(|one| one.get_long() == Some(arg.flag))
                .unwrap();
            let takes_value = found.get_action().takes_values();
            // A Stdin argument is a bare flag on the command line: the
            // document it carries goes down stdin, not after the flag.
            let declared_as_value = !matches!(arg.kind, Kind::Flag | Kind::Stdin);
            assert_eq!(
                takes_value, declared_as_value,
                "{}.{} and --{} disagree about taking a value",
                tool.name, arg.name, arg.flag,
            );
        }
    }
}

/// Section 9.1: exactly two tools. A third would sit in every agent's
/// context all session for a verb a human runs.
#[test]
fn the_tool_list_is_two_tools() {
    let names: Vec<_> = TOOLS.iter().map(|one| one.name).collect();
    assert_eq!(names, vec!["writ_record", "writ_audit"]);
}

/// A schema an agent cannot fill in is not a contract.
#[test]
fn every_tool_argument_is_described_in_the_schema() {
    for tool in TOOLS {
        let schema = tool.input_schema();
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(properties.len(), tool.args.len());
        for arg in tool.args {
            let property = &properties[arg.name];
            assert!(!property["description"].as_str().unwrap().is_empty());
            assert!(property["type"].is_string(), "{}", arg.name);
        }
    }
}

// --- parity claim 2: the same input gives the same result ---------------

#[test]
fn recording_through_mcp_matches_recording_through_the_cli() {
    let arguments = json!({
        "title": "prefer sd",
        "rule": "use sd, not sed",
        "rationale": "sed's syntax differs between GNU and BSD",
        // Two narrowing kinds, not `global` plus one: section 7.1 step 2
        // ANDs across kinds, so that pair is a validation question and
        // this test is about parity, not about validation.
        "scope": ["language:rust", "glob:crates/**"],
        "advisory": true,
        "activate": true,
    });

    let mcp_home = Sandbox::new();
    let mut server = mcp_home.server();
    let through_mcp = server.call("writ_record", &arguments);
    server.close();

    let cli_home = Sandbox::new();
    let through_cli = cli_home.record_json(&argv_of("writ_record", &arguments));

    assert_eq!(anonymize(&through_mcp), anonymize(&through_cli));
    // And both actually wrote the same row.
    assert_eq!(
        anonymize(&mcp_home.learnings()),
        anonymize(&cli_home.learnings())
    );
}

#[test]
fn auditing_through_mcp_matches_auditing_through_the_cli() {
    // A dry run, so both surfaces name the same audit: a real run writes
    // an `audits` row and the two ids would differ for the right reason.
    let arguments = json!({ "dry_run": true });

    let mcp_home = Sandbox::new();
    let repo = mcp_home.repo();
    mcp_home.record_json(&[
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--activate",
    ]);
    let mut server = mcp_home.server_at(&repo);
    let through_mcp = server.call("writ_audit", &arguments);
    server.close();

    let through_cli = mcp_home.audit_json(&repo, &argv_of("writ_audit", &arguments));

    assert_eq!(through_mcp, through_cli);
    assert_eq!(through_mcp["sent"], 1);
}

#[test]
fn ingesting_findings_through_mcp_matches_the_ingest_flag() {
    let home = Sandbox::new();
    let repo = home.repo();
    let id = home.record_json(&[
        "--title",
        "t",
        "--rule",
        "r",
        "--rationale",
        "why",
        "--activate",
    ])[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let report = home.audit_json::<&str>(&repo, &[]);
    let audit_id = report["audit_id"].as_str().unwrap().to_string();

    let findings = json!({
        "audit_id": audit_id,
        "findings": [{ "learning_id": id, "outcome": "fixed", "detail": "done" }],
    });
    let mut server = home.server_at(&repo);
    let through_mcp = server.call("writ_audit", &json!({ "findings": findings }));
    server.close();

    assert_eq!(through_mcp["audit_id"], audit_id.as_str());
    assert_eq!(through_mcp["findings"], 1);
    assert_eq!(through_mcp["unfixed_blocking"], 0);
}

/// An agent holds snippet text, not a path. Without `--example-text`
/// this surface could attach no exemplar at all, and an exemplar is what
/// makes a rule teach instead of assert. Invariant 3 holds here too: the
/// snippet is stored, and there is no path to store.
#[test]
fn an_agent_can_attach_an_exemplar_pair_through_mcp() {
    let home = Sandbox::new();
    let mut server = home.server();
    let written = server.call(
        "writ_record",
        &json!({
            "title": "prefer sd",
            "rule": "use sd, not sed",
            "rationale": "sed -i takes an argument on BSD and not on GNU",
            "example_text": ["bad:sed -i '' s/a/b/ f", "good:sd a b f"],
            "activate": true,
        }),
    );
    server.close();

    let id = written[0]["id"].as_str().unwrap();
    let shown = home.show(id);
    let exemplars = shown["exemplars"].as_array().unwrap();
    assert_eq!(exemplars.len(), 2);
    assert_eq!(exemplars[0]["kind"], "bad");
    assert_eq!(exemplars[0]["snippet"], "sed -i '' s/a/b/ f");
    assert_eq!(exemplars[1]["snippet"], "sd a b f");
    assert!(exemplars[0].get("path").is_none());
}

/// A refusal must be the same refusal. An empty rationale is a validation
/// error on the CLI, so it is a tool error here and not a written row.
#[test]
fn a_refusal_reaches_the_agent_instead_of_a_silent_write() {
    let home = Sandbox::new();
    let mut server = home.server();
    let raw = server.call_raw(
        "writ_record",
        &json!({ "title": "t", "rule": "r", "rationale": "" }),
    );
    server.close();

    assert_eq!(raw["isError"], true);
    let text = raw["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("rationale"), "{text}");
    assert!(home.learnings().as_array().unwrap().is_empty());
}

/// An argument writ does not have is refused, not dropped. An agent that
/// misspells `activate` must not be told it recorded an active learning.
#[test]
fn an_unknown_argument_is_refused() {
    let home = Sandbox::new();
    let mut server = home.server();
    let raw = server.call_raw(
        "writ_record",
        &json!({ "title": "t", "rule": "r", "rationale": "w", "activated": true }),
    );
    server.close();

    assert_eq!(raw["isError"], true);
    assert!(
        raw["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("activated"),
    );
}

// --- the protocol -------------------------------------------------------

#[test]
fn the_server_initializes_and_lists_its_tools() {
    let home = Sandbox::new();
    let mut server = home.server();

    let init = server.request("initialize", &json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "writ");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let listed = server.request("tools/list", &json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "writ_record");

    let unknown = server.request("resources/list", &json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    server.close();
}

/// One bad frame must not take the session down: the host cannot tell that
/// apart from a crash.
#[test]
fn a_malformed_frame_is_answered_and_the_session_survives() {
    let home = Sandbox::new();
    let mut server = home.server();

    server.write_line("{ not json");
    let error = server.read_reply();
    assert_eq!(error["error"]["code"], -32700);

    let init = server.request("initialize", &json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "writ");
    server.close();
}

/// A notification has no id, and JSON-RPC forbids replying to one.
#[test]
fn a_notification_gets_no_reply() {
    let home = Sandbox::new();
    let mut server = home.server();

    server.write_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    let after = server.request("ping", &json!({}));
    assert!(after["result"].is_object(), "{after}");
    server.close();
}

// --- helpers ------------------------------------------------------------

/// The argv the tool builds, as `&str`, so the CLI half of a parity test
/// cannot quietly use different values from the MCP half.
fn argv_of(tool: &str, arguments: &Value) -> Vec<String> {
    let tool: &Tool = TOOLS.iter().find(|one| one.name == tool).unwrap();
    let (argv, _) = tool.to_argv(arguments).unwrap();
    argv[1..].to_vec()
}

/// Drop the fields that are new on every write, so two runs of the same
/// input compare equal when they should.
fn anonymize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(anonymize).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "id" | "learning_id" | "created_at" | "updated_at" | "activated_at"
                    )
                })
                .map(|(key, one)| (key.clone(), anonymize(one)))
                .collect(),
        ),
        other => other.clone(),
    }
}

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("empty.gitconfig"), "").unwrap();
        std::fs::write(dir.path().join("config.toml"), "").unwrap();
        Self { dir }
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("learnings.db")
    }

    fn cmd_at(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = writ();
        command
            .arg("--db")
            .arg(self.db())
            .arg("--config")
            .arg(self.dir.path().join("config.toml"))
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", self.dir.path().join("empty.gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.dir.path().join("empty.gitconfig"))
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("EMAIL");
        command
    }

    /// A git repository with one commit and one uncommitted change.
    fn repo(&self) -> PathBuf {
        let root = self.dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main", "."]);
        git(
            &root,
            &["remote", "add", "origin", "git@github.com:Owner/Repo.git"],
        );
        std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "one"]);
        std::fs::write(root.join("a.rs"), "fn main() { let x = 1; }\n").unwrap();
        root
    }

    fn record_json<S: AsRef<str>>(&self, args: &[S]) -> Value {
        let mut all = vec![
            "record".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ];
        all.extend(args.iter().map(|one| one.as_ref().to_string()));
        let borrowed: Vec<&str> = all.iter().map(String::as_str).collect();
        let output = self.cmd_at(self.dir.path(), &borrowed).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn audit_json<S: AsRef<str>>(&self, repo: &Path, args: &[S]) -> Value {
        let mut all = vec![
            "audit".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ];
        all.extend(args.iter().map(|one| one.as_ref().to_string()));
        let borrowed: Vec<&str> = all.iter().map(String::as_str).collect();
        let output = self.cmd_at(repo, &borrowed).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn show(&self, id: &str) -> Value {
        let output = self
            .cmd_at(self.dir.path(), &["show", id, "--format", "json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn learnings(&self) -> Value {
        let output = self
            .cmd_at(self.dir.path(), &["list", "--format", "json"])
            .output()
            .unwrap();
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn server(&self) -> McpServer {
        self.server_at(self.dir.path())
    }

    fn server_at(&self, dir: &Path) -> McpServer {
        let child = self
            .cmd_at(dir, &["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        McpServer::new(child)
    }
}

/// A `writ mcp` process, spoken to the way a host speaks to it.
struct McpServer {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl McpServer {
    fn new(mut child: Child) -> Self {
        let out = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            out,
            next_id: 1,
        }
    }

    fn write_line(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn read_reply(&mut self) -> Value {
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error}: {line:?}"))
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_line(&body.to_string());
        let reply = self.read_reply();
        assert_eq!(reply["id"], id);
        reply
    }

    /// The `tools/call` result, whatever it is.
    fn call_raw(&mut self, tool: &str, arguments: &Value) -> Value {
        self.request(
            "tools/call",
            &json!({ "name": tool, "arguments": arguments }),
        )["result"]
            .clone()
    }

    /// The structured result of a tool call that had to succeed.
    fn call(&mut self, tool: &str, arguments: &Value) -> Value {
        let result = self.call_raw(tool, arguments);
        assert!(
            result.get("isError").is_none(),
            "{}",
            result["content"][0]["text"]
        );
        result["structuredContent"].clone()
    }

    fn close(mut self) {
        drop(self.child.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "writ mcp exited {status}");
    }
}

fn git(dir: &Path, args: &[&str]) {
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
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
