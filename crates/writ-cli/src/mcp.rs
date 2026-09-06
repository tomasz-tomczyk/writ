//! `writ mcp`. Spec section 9.1.
//!
//! Two tools, `writ_record` and `writ_audit`. Not one per verb: every tool
//! definition sits in the agent's context all session, and P3 is about not
//! spending context you did not have to. The UI, health, and pruning stay
//! human-only and out of the tool list.
//!
//! Both tools are shells. Each turns its arguments into the argv the CLI
//! would have been given, hands that to the same clap parser the binary
//! uses, and calls the same function. Nothing here validates, defaults, or
//! decides. That is what makes the parity test in `tests/mcp.rs` provable
//! rather than aspirational: an argument that does not name a real flag
//! cannot reach a tool at all.
//!
//! The transport is newline-delimited JSON-RPC 2.0 on stdio, written by
//! hand. A crate would add a dependency, an async runtime, and a derive
//! layer to serve two tools over a protocol whose stdio framing is one
//! line of JSON per message.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde_json::{Value, json};
use writ_core::{
    CommandMetric, Config, CounterMetric, Paths, Result, SurfaceMetric, TelemetryBatch,
};

use crate::audit;
use crate::record;
use crate::telemetry;

/// The protocol revision this server implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// `writ mcp` takes no flags of its own. Spec section 5.
///
/// `--db` and `--config` are global, so they reach the server the same way
/// they reach every other subcommand. Per P7 a key that moves the storage
/// path is honored by every subcommand that touches the store.
#[derive(Debug, clap::Args)]
pub struct Args {}

/// How one tool argument reaches the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One string value: `--title TEXT`.
    Text,
    /// One number: `--max-rules N`.
    Number,
    /// Present or absent: `--advisory`.
    Flag,
    /// Repeatable: `--scope a --scope b`.
    TextList,
    /// A document the CLI reads on stdin. stdin is the transport here, so
    /// the tool takes it as an argument instead.
    Stdin,
}

/// One argument of one tool, and the flag it is.
#[derive(Debug, Clone, Copy)]
pub struct ToolArg {
    /// The name the agent passes.
    pub name: &'static str,
    /// The long flag it becomes, without the leading dashes.
    pub flag: &'static str,
    /// How it is spelled on the command line.
    pub kind: Kind,
    /// What the schema tells the agent.
    pub about: &'static str,
}

/// One tool.
#[derive(Debug, Clone, Copy)]
pub struct Tool {
    /// The MCP tool name.
    pub name: &'static str,
    /// The subcommand it runs.
    pub command: &'static str,
    /// What the schema tells the agent.
    pub about: &'static str,
    /// Its arguments.
    pub args: &'static [ToolArg],
}

/// Spec section 5, the `writ record` row.
///
/// `--example` is absent because it takes a *file path*, and an agent
/// holds snippet text. `--example-text` is the form for that, and it is
/// here: without it this surface could not attach an exemplar at all, and
/// an exemplar is what makes a rule teach instead of assert.
/// `--json` is absent because it reads stdin, and stdin is the transport.
const RECORD_ARGS: &[ToolArg] = &[
    ToolArg {
        name: "title",
        flag: "title",
        kind: Kind::Text,
        about: "A short name for the learning. Required unless reinforce is given",
    },
    ToolArg {
        name: "rule",
        flag: "rule",
        kind: Kind::Text,
        about: "What to do. Required unless reinforce is given",
    },
    ToolArg {
        name: "rationale",
        flag: "rationale",
        kind: Kind::Text,
        about: "Why the rule exists. Required unless reinforce is given, and never empty",
    },
    ToolArg {
        name: "scope",
        flag: "scope",
        kind: Kind::TextList,
        about: "Where it applies: global, project:ID, language:LANG or glob:PAT",
    },
    ToolArg {
        name: "advisory",
        flag: "advisory",
        kind: Kind::Flag,
        about: "Demote to report-only. Blocking is the default",
    },
    ToolArg {
        name: "example_text",
        flag: "example-text",
        kind: Kind::TextList,
        about: "A snippet the rule is about, as good:TEXT or bad:TEXT. Give the pair when you \
                can: the wrong code and the right code teach more than the rule alone",
    },
    ToolArg {
        name: "matcher",
        flag: "matcher",
        kind: Kind::Text,
        about: "A retrieval pattern. Needs matcher_kind",
    },
    ToolArg {
        name: "matcher_kind",
        flag: "matcher-kind",
        kind: Kind::Text,
        about: "The dialect of matcher: ast_grep or regex",
    },
    ToolArg {
        name: "status",
        flag: "status",
        kind: Kind::Text,
        about: "proposed or active. Default proposed, so a forgotten activate lands in the Inbox",
    },
    ToolArg {
        name: "activate",
        flag: "activate",
        kind: Kind::Flag,
        about: "Sugar for status active",
    },
    ToolArg {
        name: "reinforce",
        flag: "reinforce",
        kind: Kind::Text,
        about: "Attach to an existing learning id instead of creating one",
    },
];

/// Spec section 5, the `writ audit` row.
///
/// `--hook` is absent: it is the host's gate protocol, emitted by a Stop
/// hook, not something an agent asks for. `--format` is absent because
/// this surface always answers JSON.
const AUDIT_ARGS: &[ToolArg] = &[
    ToolArg {
        name: "diff",
        flag: "diff",
        kind: Kind::Text,
        about: "Git range. Defaults to the working tree against HEAD",
    },
    ToolArg {
        name: "dry_run",
        flag: "dry-run",
        kind: Kind::Flag,
        about: "Select and render, but write nothing: no audit row and no counters",
    },
    ToolArg {
        name: "max_rules",
        flag: "max-rules",
        kind: Kind::Number,
        about: "Override the configured cap on how many learnings one audit sends",
    },
    ToolArg {
        name: "max_chars",
        flag: "max-chars",
        kind: Kind::Number,
        about: "Override the configured cap on the characters of rule text one audit sends",
    },
    ToolArg {
        name: "findings",
        flag: "ingest",
        kind: Kind::Stdin,
        about: "The findings document to write back. The CLI reads this on stdin as --ingest",
    },
];

/// The whole tool list. Two entries, and section 9.1 says why not three.
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "writ_record",
        command: "record",
        about: "Record a learning: a piece of steering worth keeping. \
                Writes land as 'proposed' unless activate is set.",
        args: RECORD_ARGS,
    },
    Tool {
        name: "writ_audit",
        command: "audit",
        about: "Check a diff against the learnings that apply to it, or, with findings, \
                write back what the review found.",
        args: AUDIT_ARGS,
    },
];

/// The tool with this name.
pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|one| one.name == name)
}

impl Tool {
    /// The JSON Schema an agent sees.
    pub fn input_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        for arg in self.args {
            let schema = match arg.kind {
                Kind::Text => json!({ "type": "string", "description": arg.about }),
                Kind::Number => json!({ "type": "integer", "description": arg.about }),
                Kind::Flag => json!({ "type": "boolean", "description": arg.about }),
                Kind::TextList => json!({
                    "type": "array",
                    "items": { "type": "string" },
                    "description": arg.about,
                }),
                Kind::Stdin => json!({ "type": "object", "description": arg.about }),
            };
            properties.insert(arg.name.to_string(), schema);
        }
        json!({
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        })
    }

    /// The tool as `tools/list` returns it.
    pub fn describe(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.about,
            "inputSchema": self.input_schema(),
        })
    }

    /// The argv the CLI would have been given, and the stdin document.
    ///
    /// An unknown argument is refused rather than dropped: an agent that
    /// misspells `activate` must hear about it, not silently record a
    /// proposal it believes it activated. crit #446.
    pub fn to_argv(
        &self,
        arguments: &Value,
    ) -> std::result::Result<(Vec<String>, Option<String>), String> {
        let mut argv = vec!["writ".to_string()];
        let mut stdin = None;
        let Some(object) = arguments.as_object() else {
            return Err("arguments must be a JSON object".to_string());
        };
        for (name, value) in object {
            if value.is_null() {
                continue;
            }
            let Some(arg) = self.args.iter().find(|one| one.name == name) else {
                return Err(format!(
                    "{} has no argument {name}. It takes: {}",
                    self.name,
                    self.args
                        .iter()
                        .map(|one| one.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            };
            let flag = format!("--{}", arg.flag);
            match arg.kind {
                Kind::Flag => match value.as_bool() {
                    Some(true) => argv.push(flag),
                    Some(false) => {}
                    None => return Err(format!("{name} takes true or false")),
                },
                Kind::Text => {
                    let text = value
                        .as_str()
                        .ok_or_else(|| format!("{name} takes a string"))?;
                    argv.push(flag);
                    argv.push(text.to_string());
                }
                Kind::Number => {
                    let number = value
                        .as_u64()
                        .ok_or_else(|| format!("{name} takes a whole number"))?;
                    argv.push(flag);
                    argv.push(number.to_string());
                }
                Kind::TextList => {
                    let list = value
                        .as_array()
                        .ok_or_else(|| format!("{name} takes an array of strings"))?;
                    for one in list {
                        let text = one
                            .as_str()
                            .ok_or_else(|| format!("{name} takes an array of strings"))?;
                        argv.push(flag.clone());
                        argv.push(text.to_string());
                    }
                }
                Kind::Stdin => {
                    argv.push(flag);
                    stdin = Some(value.to_string());
                }
            }
        }
        Ok((argv, stdin))
    }
}

/// `record::Args` behind a parser, so the tool reuses the binary's own
/// validation instead of a second copy of it.
#[derive(Debug, Parser)]
struct RecordLine {
    #[command(flatten)]
    inner: record::Args,
}

/// The same for `audit::Args`.
#[derive(Debug, Parser)]
struct AuditLine {
    #[command(flatten)]
    inner: audit::Args,
}

/// The server, with the paths every subcommand resolved once.
pub struct Server {
    db: PathBuf,
    telemetry_db: PathBuf,
    config: Config,
}

impl Server {
    /// A server over this database and configuration.
    pub fn new(db: &Path, config: Config) -> Self {
        Self::new_with_telemetry(db, db.with_file_name("telemetry.db"), config)
    }

    /// A server with the separately resolved telemetry store used by the
    /// production command.
    pub fn new_with_telemetry(db: &Path, telemetry_db: impl Into<PathBuf>, config: Config) -> Self {
        Self {
            db: db.to_path_buf(),
            telemetry_db: telemetry_db.into(),
            config,
        }
    }

    /// Handle one request. `None` means the message was a notification and
    /// takes no reply.
    pub fn handle(&self, request: &Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        // A notification has no id, and JSON-RPC forbids replying to one.
        let id = id?;

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "writ", "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": TOOLS.iter().map(Tool::describe).collect::<Vec<_>>(),
            })),
            "tools/call" => Ok(self.call(&params)),
            other => Err((-32601, format!("unknown method {other}"))),
        };

        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message },
            }),
        })
    }

    /// Run one tool. A refusal is a tool error, not a protocol error: the
    /// agent has to read it, and a JSON-RPC error is not shown to a model.
    fn call(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let Some(tool) = tool(name) else {
            return failure(&format!(
                "unknown tool {name}. writ serves writ_record and writ_audit"
            ));
        };
        let (argv, stdin) = match tool.to_argv(&arguments) {
            Ok(both) => both,
            Err(message) => return failure(&message),
        };

        match self.dispatch(tool, &argv, stdin.as_deref()) {
            Ok(value) => success(&value),
            Err(message) => failure(&message),
        }
    }

    /// Parse the argv and call the function the CLI calls.
    fn dispatch(
        &self,
        tool: &Tool,
        argv: &[String],
        stdin: Option<&str>,
    ) -> std::result::Result<Value, String> {
        match tool.command {
            "record" => {
                let line = RecordLine::try_parse_from(argv).map_err(usage)?;
                let (written, batch) =
                    record::execute_with_telemetry(&line.inner, &self.db, &self.config)
                        .map_err(cause)?;
                self.observe(batch, CommandMetric::Record);
                Ok(serde_json::to_value(&written).expect("Recorded serializes"))
            }
            "audit" => {
                let line = AuditLine::try_parse_from(argv).map_err(usage)?;
                if let Some(findings) = stdin {
                    let (written, batch) =
                        audit::ingest_with_telemetry(findings, &self.db).map_err(cause)?;
                    self.observe(batch, CommandMetric::Audit);
                    return Ok(serde_json::to_value(&written).expect("Ingested serializes"));
                }
                let mut run = audit::select(&line.inner, &self.db, &self.config).map_err(cause)?;
                let mut report = audit::report(&run, line.inner.dry_run);
                if !run.notices.is_empty() {
                    report["notices"] = json!(run.notices);
                }
                self.observe(std::mem::take(&mut run.telemetry), CommandMetric::Audit);
                Ok(report)
            }
            other => Err(format!("no such command {other}")),
        }
    }

    fn observe(&self, mut batch: TelemetryBatch, command: CommandMetric) {
        if !self.config.telemetry.enabled {
            return;
        }
        batch.counters.extend([
            CounterMetric::Command(command),
            CounterMetric::Surface(SurfaceMetric::Mcp),
            CounterMetric::ExitCode(0),
        ]);
        telemetry::add_collection_size_best_effort(&self.db, &mut batch);
        telemetry::record_best_effort(&self.telemetry_db, &batch);
    }
}

/// A clap refusal, with its usage text stripped of the fake program name.
fn usage(error: clap::Error) -> String {
    error.render().to_string().trim_end().to_string()
}

/// A writ error, named the way the CLI names it, with its exit code so the
/// two surfaces report one cause one way. Spec section 5.7.
fn cause(error: writ_core::Error) -> String {
    format!("writ: {error} (exit {})", crate::exit_code(&error))
}

fn success(value: &Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": value.to_string() }],
        "structuredContent": value,
    })
}

fn failure(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

/// Serve on stdio until the host closes the stream.
pub fn run(_args: &Args, paths: &Paths, config: &Config) -> Result<ExitCode> {
    let server = Server::new_with_telemetry(&paths.db, &paths.telemetry_db, config.clone());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(&server, stdin.lock(), stdout.lock());
    Ok(ExitCode::SUCCESS)
}

/// The read-dispatch-write loop, over any pair of streams so a test can
/// drive it without a process.
///
/// A line writ cannot parse gets a JSON-RPC parse error and the loop
/// continues. Exiting there would take the session down over one bad
/// frame, and the host has no way to tell that apart from a crash.
pub fn serve(server: &Server, input: impl BufRead, mut output: impl Write) {
    for line in input.lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => server.handle(&request),
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": format!("parse error: {error}") },
            })),
        };
        let Some(reply) = reply else { continue };
        if writeln!(output, "{reply}").is_err() {
            return;
        }
        if output.flush().is_err() {
            return;
        }
    }
}
