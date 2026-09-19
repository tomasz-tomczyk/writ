//! `writ mcp`. Spec section 9.1.
//!
//! Three tools, `writ_record`, `writ_audit`, and `writ_edit`. Not one per
//! verb: every tool definition sits in the agent's context all session, and
//! P3 is about not spending context you did not have to. The UI, health,
//! archive, and telemetry stay human-only and out of the tool list.
//!
//! The tools are shells. Each turns its arguments into the argv the CLI
//! would have been given, hands that to the same clap parser the binary
//! uses, and calls the same function. Nothing here validates, defaults, or
//! decides. That is what makes the parity test in `tests/mcp.rs` provable
//! rather than aspirational: an argument that does not name a real flag
//! cannot reach a tool at all.
//!
//! The transport is newline-delimited JSON-RPC 2.0 on stdio, written by
//! hand. A crate would add a dependency, an async runtime, and a derive
//! layer to serve them over a protocol whose stdio framing is one line of
//! JSON per message.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde_json::{Value, json};
use writ_core::{CommandMetric, Config, Paths, Result, Store, SurfaceMetric, TelemetryBatch};

use crate::audit;
use crate::edit;
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

/// What the host may put in front of the model once, at connection.
///
/// writ's value is a loop that spans tools — a gate emits a pointer, the
/// agent fetches the document, reports findings, and later settles what it
/// left open — and no single tool description can say that, because each
/// one is read alone and only when the model is already reaching for it.
/// Without this, that knowledge ships only in the Claude Code skill and
/// the CLAUDE.md snippet, so a host that installs the MCP server and
/// nothing else never learns the loop exists.
const INSTRUCTIONS: &str = "\
writ is a ledger of the steering this developer has already given. It \
gates a handoff: when a turn ends, writ checks the diff against the rules \
that apply to it.

The loop, in order:

1. A gate blocks a turn with an audit id and nothing else. It never pastes \
   the document, because that would put the whole diff in the transcript \
   after every turn.
2. Fetch it: call writ_audit with `fetch` set to that audit id. This is a \
   read — it opens no audit and moves no counter.
3. Decide what you will do about each learning the diff breaks, then send \
   that back: call writ_audit with `findings` holding the whole document. \
   Nothing is recorded until you do, and an empty findings array is the \
   right answer when the diff breaks no rule. It still has to be sent.
4. A finding you reported as `open` — one you could not settle without the \
   developer — keeps refusing the handoff while it stands. Once the two of \
   you agree, call writ_audit with `resolve` set to that finding id and \
   `outcome` set to fixed or ignored.

Use writ_record when the developer corrects an approach, states a \
convention, or says to always or never do something: one rule, with the \
reason it exists, so it transfers to a case they did not foresee. A write \
lands as a proposal for them to review unless they asked for it to be \
active.";

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
    /// A positional value: `writ edit ID`.
    Positional,
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
        about: "Structural retrieval pattern for the rule. Omit for rules with no structural anchor. Pair with matcher_kind",
    },
    ToolArg {
        name: "matcher_kind",
        flag: "matcher-kind",
        kind: Kind::Text,
        about: "Dialect of matcher: ast_grep (preferred with language: scope) or regex (escape hatch)",
    },
    ToolArg {
        name: "sides",
        flag: "sides",
        kind: Kind::Text,
        about: "Which half of the diff the rule cares about: added, removed, or both (default both)",
    },
    // `--status` is not offered here, though the CLI keeps it. It has
    // exactly one non-default value, which `activate` already spells, and
    // two spellings of one decision in a tool schema is a coin an agent
    // flips. The default stays where invariant 2 puts it: in the column.
    ToolArg {
        name: "activate",
        flag: "activate",
        kind: Kind::Flag,
        about: "Record it active instead of proposed. Without this it lands in the \
                developer's Inbox for review, which is the safe default and usually \
                the right one: activate only what they have actually approved",
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
        name: "fetch",
        flag: "fetch",
        kind: Kind::Text,
        about: "An audit id a gate handed you. Returns the document that audit sent: \
                the diff, the learnings that apply to it, and how to report back. \
                This is what a Stop hook's pointer asks for, and it is a read: it \
                opens no audit and moves no counter",
    },
    ToolArg {
        name: "resolve",
        flag: "resolve",
        kind: Kind::Text,
        about: "A finding id to settle, once a question you reported as open has been \
                answered. Pass outcome alongside it. Use this when the developer and \
                you agreed what to do about a finding an earlier audit left open: \
                ingest writes an outcome once, so without this the finding stays open \
                and a blocking learning keeps refusing the handoff",
    },
    ToolArg {
        name: "outcome",
        flag: "outcome",
        kind: Kind::Text,
        about: "What resolve settles the finding to: fixed or ignored. Only a \
                developer rejects a finding, so rejected is not accepted here",
    },
    ToolArg {
        name: "findings",
        flag: "ingest",
        kind: Kind::Stdin,
        about: "The whole findings document to write back: the audit_id the audit \
                prompt printed, and a findings array. This is how a review closes \
                the loop, and it is what moves times_applied. The CLI reads the \
                same document on stdin as --ingest",
    },
];

/// Spec section 5, the `writ edit ID` row.
///
/// `--example` is absent for the same reason it is absent from
/// `writ_record`: an agent holds snippet text, not a file path.
/// `--format` is absent because this surface always answers JSON.
const EDIT_ARGS: &[ToolArg] = &[
    ToolArg {
        name: "id",
        flag: "id",
        kind: Kind::Positional,
        about: "The learning to edit",
    },
    ToolArg {
        name: "title",
        flag: "title",
        kind: Kind::Text,
        about: "A short name for the learning",
    },
    ToolArg {
        name: "rule",
        flag: "rule",
        kind: Kind::Text,
        about: "What to do",
    },
    ToolArg {
        name: "rationale",
        flag: "rationale",
        kind: Kind::Text,
        about: "Why the rule exists. Required, and never empty",
    },
    ToolArg {
        name: "scope",
        flag: "scope",
        kind: Kind::TextList,
        about: "Where it applies: global, project:ID, language:LANG or glob:PAT. \
                Any scope replaces the full set",
    },
    ToolArg {
        name: "advisory",
        flag: "advisory",
        kind: Kind::Flag,
        about: "Demote to report-only",
    },
    ToolArg {
        name: "blocking",
        flag: "blocking",
        kind: Kind::Flag,
        about: "Promote to blocking (the default for new learnings)",
    },
    ToolArg {
        name: "sides",
        flag: "sides",
        kind: Kind::Text,
        about: "Which half of the diff the rule cares about: added, removed, or both. \
                Omit to keep the current value",
    },
    ToolArg {
        name: "matcher",
        flag: "matcher",
        kind: Kind::Text,
        about: "Structural retrieval pattern for the rule. Pair with matcher_kind",
    },
    ToolArg {
        name: "matcher_kind",
        flag: "matcher-kind",
        kind: Kind::Text,
        about: "Dialect of matcher: ast_grep (preferred with language: scope) or regex",
    },
    ToolArg {
        name: "clear_matcher",
        flag: "clear-matcher",
        kind: Kind::Flag,
        about: "Remove any matcher and matcher-kind",
    },
    ToolArg {
        name: "example_text",
        flag: "example-text",
        kind: Kind::TextList,
        about: "A snippet the rule is about, as good:TEXT or bad:TEXT. Give the pair when you \
                can: the wrong code and the right code teach more than the rule alone. \
                Any example_text replaces the full exemplar set",
    },
    ToolArg {
        name: "activate",
        flag: "activate",
        kind: Kind::Flag,
        about: "Sugar for proposed → active after a successful edit",
    },
];

/// The whole tool list. Three entries, and section 9.1 says why not more.
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
    Tool {
        name: "writ_edit",
        command: "edit",
        about: "Edit an existing learning: title, rule, rationale, scope, blocking, \
                sides, matcher, or exemplars. Proposed learnings can be activated; \
                archived ones cannot.",
        args: EDIT_ARGS,
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
                // The one argument that is a document rather than a
                // flag value. Typing it as a bare object left the shape
                // that moves `times_applied` and stamps `ingested_at`
                // described only in rendered prompt text, and this loop
                // has shipped broken once already.
                Kind::Stdin => json!({
                    "type": "object",
                    "description": arg.about,
                    "properties": {
                        "audit_id": {
                            "type": "string",
                            "description": "The audit-id the prompt printed, verbatim",
                        },
                        "findings": {
                            "type": "array",
                            "description": "One entry per violation. An empty array is \
                                            the right answer when the diff breaks no \
                                            rule, and it still has to be sent",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "learning_id": {
                                        "type": "string",
                                        "description": "The id of the learning this violates",
                                    },
                                    "path": {
                                        "type": "string",
                                        "description": "The file it happens in, when there is one",
                                    },
                                    "line": {
                                        "type": "integer",
                                        "description": "The line it happens on, when there is one",
                                    },
                                    "detail": {
                                        "type": "string",
                                        "description": "What is wrong, in one sentence",
                                    },
                                    "outcome": {
                                        "type": "string",
                                        "enum": ["fixed", "ignored", "open"],
                                        "description": "What you have decided to do: \
                                                        `fixed` you are correcting it, \
                                                        `ignored` you are leaving it and \
                                                        say why in detail, `open` you \
                                                        cannot settle it without the \
                                                        developer. `rejected` is the \
                                                        developer's word and is refused here",
                                    },
                                },
                                "required": ["learning_id", "detail", "outcome"],
                                "additionalProperties": false,
                            },
                        },
                    },
                    "required": ["audit_id", "findings"],
                }),
                Kind::Positional => json!({ "type": "string", "description": arg.about }),
            };
            properties.insert(arg.name.to_string(), schema);
        }
        // A positional is the one argument shape the command cannot run
        // without: `writ_edit` has nothing to edit without an id. Every
        // flag is optional, including the ones a bare `writ_record`
        // needs, because `--reinforce` is a call that carries none of
        // them. Saying so in the schema turns a refusal the model has to
        // read into a call it gets right the first time.
        let required: Vec<&str> = self
            .args
            .iter()
            .filter(|arg| arg.kind == Kind::Positional)
            .map(|arg| arg.name)
            .collect();
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
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
        let mut positional = Vec::new();
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
            match arg.kind {
                Kind::Positional => {
                    let text = value
                        .as_str()
                        .ok_or_else(|| format!("{name} takes a string"))?;
                    positional.push(text.to_string());
                }
                Kind::Flag => {
                    let flag = format!("--{}", arg.flag);
                    match value.as_bool() {
                        Some(true) => argv.push(flag),
                        Some(false) => {}
                        None => return Err(format!("{name} takes true or false")),
                    }
                }
                Kind::Text => {
                    let flag = format!("--{}", arg.flag);
                    let text = value
                        .as_str()
                        .ok_or_else(|| format!("{name} takes a string"))?;
                    argv.push(flag);
                    argv.push(text.to_string());
                }
                Kind::Number => {
                    let flag = format!("--{}", arg.flag);
                    let number = value
                        .as_u64()
                        .ok_or_else(|| format!("{name} takes a whole number"))?;
                    argv.push(flag);
                    argv.push(number.to_string());
                }
                Kind::TextList => {
                    let flag = format!("--{}", arg.flag);
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
                    argv.push(format!("--{}", arg.flag));
                    stdin = Some(value.to_string());
                }
            }
        }
        argv.extend(positional);
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

/// The same for `edit::Args`.
#[derive(Debug, Parser)]
struct EditLine {
    #[command(flatten)]
    inner: edit::Args,
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
        // Only an object can be a request. A top-level array (a 2025-03-26
        // batch, which 2025-06-18 removed) or a bare scalar is neither a
        // request nor a notification, and answering nothing would hang the
        // client forever: P7 says name the cause instead.
        let Some(object) = request.as_object() else {
            return Some(invalid_request(
                "a JSON-RPC message must be an object. writ does not serve batches",
            ));
        };
        let method = object.get("method").and_then(Value::as_str);
        let id = object.get("id").cloned();

        // A notification is an object carrying a method and no id, and
        // JSON-RPC forbids replying to one. An object with neither, or an
        // explicit null id, is malformed rather than silent.
        let (Some(method), Some(id)) = (method, id) else {
            // A method with no id is a notification: no reply. Anything
            // else is malformed and gets one.
            if method.is_some() {
                return None;
            }
            return Some(invalid_request(
                "a JSON-RPC request needs a method and a non-null id",
            ));
        };
        if id.is_null() {
            return Some(invalid_request("a JSON-RPC request id must not be null"));
        }
        let params = object.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "writ", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": TOOLS.iter().map(Tool::describe).collect::<Vec<_>>(),
            })),
            "tools/call" => self.call(&params),
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

    /// Run one tool.
    ///
    /// A refusal the *tool* produced is a tool error, not a protocol error:
    /// the agent has to read it, and a JSON-RPC error is not shown to a
    /// model. Failing to *find* the tool is the other way round — the
    /// caller named something that does not exist, so it is `-32602`, and
    /// a host can tell a stale tool list from a tool that ran and failed
    /// rather than retrying a call that can never succeed.
    fn call(&self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err((-32602, "tools/call needs a tool name".to_string()));
        };
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let Some(tool) = tool(name) else {
            return Err((
                -32602,
                format!("unknown tool {name}. writ serves writ_record, writ_audit, and writ_edit"),
            ));
        };
        // From here the tool exists, so every refusal is its own and
        // reaches the model as a tool error it can act on.
        let (argv, stdin) = match tool.to_argv(&arguments) {
            Ok(both) => both,
            Err(message) => return Ok(failure(&message)),
        };

        Ok(match self.dispatch(tool, &argv, stdin.as_deref()) {
            Ok(Answer { value, text }) => success(&value, text.as_deref()),
            Err(message) => failure(&message),
        })
    }

    /// Parse the argv and call the function the CLI calls.
    fn dispatch(
        &self,
        tool: &Tool,
        argv: &[String],
        stdin: Option<&str>,
    ) -> std::result::Result<Answer, String> {
        match tool.command {
            "record" => {
                let line = RecordLine::try_parse_from(argv).map_err(usage)?;
                let (written, batch) =
                    record::execute_with_telemetry(&line.inner, &self.db, &self.config)
                        .map_err(cause)?;
                self.observe(batch, CommandMetric::Record);
                // `structuredContent` must be an object, and Claude Code
                // refuses a result whose `structuredContent` is not one. A
                // record answers a list, so the list rides under `recorded`.
                Ok(Answer::json(json!({ "recorded": written })))
            }
            "audit" => {
                let line = AuditLine::try_parse_from(argv).map_err(usage)?;
                if let Some(findings) = stdin {
                    let (written, batch) =
                        audit::ingest_with_telemetry(findings, &self.db).map_err(cause)?;
                    self.observe(batch, CommandMetric::Audit);
                    return Ok(Answer::json(
                        serde_json::to_value(&written).expect("Ingested serializes"),
                    ));
                }
                // A fetch is a read of one row, so it takes neither the
                // selection path nor a telemetry counter for one.
                if let Some(id) = &line.inner.fetch {
                    let prompt = Store::open(&self.db)
                        .and_then(|store| store.audit_prompt(id))
                        .map_err(cause)?;
                    return Ok(Answer::text(
                        json!({ "audit_id": id, "prompt": &prompt }),
                        prompt,
                    ));
                }
                // A resolve is a write of one column on one finding. It must
                // come before the selection path: falling through to it would
                // open an audit row and move `times_selected` for a call that
                // asked to settle a finding, and leave the finding open.
                if let Some(id) = &line.inner.resolve {
                    let Some(outcome) = line.inner.outcome else {
                        return Err(cause(writ_core::Error::Validation {
                            message: "--resolve needs --outcome fixed or --outcome ignored"
                                .to_string(),
                        }));
                    };
                    let batch = audit::settle(id, outcome, &self.db).map_err(cause)?;
                    self.observe(batch, CommandMetric::Audit);
                    return Ok(Answer::json(
                        json!({ "finding_id": id, "outcome": outcome.to_string() }),
                    ));
                }
                let mut run = audit::select(&line.inner, &self.db, &self.config).map_err(cause)?;
                let mut report = audit::report(&run, line.inner.dry_run);
                if !run.notices.is_empty() {
                    report["notices"] = json!(run.notices);
                }
                self.observe(std::mem::take(&mut run.telemetry), CommandMetric::Audit);
                Ok(Answer::json(report))
            }
            "edit" => {
                let line = EditLine::try_parse_from(argv).map_err(usage)?;
                let learning = edit::execute(&line.inner, &self.db).map_err(cause)?;
                self.observe(TelemetryBatch::default(), CommandMetric::Edit);
                Ok(Answer::json(
                    serde_json::to_value(&learning).expect("Learning serializes"),
                ))
            }
            other => Err(format!("no such command {other}")),
        }
    }

    fn observe(&self, mut batch: TelemetryBatch, command: CommandMetric) {
        if !self.config.telemetry.enabled {
            return;
        }
        // A tool call that got this far succeeded, and the server keeps
        // no clock, so there is no duration to record.
        telemetry::observe(
            &self.db,
            &self.telemetry_db,
            &mut batch,
            command,
            SurfaceMetric::Mcp,
            0,
            None,
        );
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

/// What one tool call produced.
///
/// `text` exists for the one answer that is a document rather than a
/// record. `writ_audit`'s fetch returns the audit prompt, and handing a
/// model a JSON-escaped 100 KB diff to unescape is a worse answer than
/// handing it the document. `structuredContent` still carries the object,
/// so a caller that wants fields keeps them.
struct Answer {
    value: Value,
    text: Option<String>,
}

impl Answer {
    /// An answer whose text is its JSON.
    fn json(value: Value) -> Self {
        Self { value, text: None }
    }

    /// An answer whose text is a document.
    fn text(value: Value, text: String) -> Self {
        Self {
            value,
            text: Some(text),
        }
    }
}

fn success(value: &Value, text: Option<&str>) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": text.map_or_else(|| value.to_string(), str::to_string),
        }],
        "structuredContent": value,
    })
}

/// A malformed frame, answered with `id: null` so the client sees a reply
/// rather than waiting on one. Invariant 6.
fn invalid_request(message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": { "code": -32600, "message": message },
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
