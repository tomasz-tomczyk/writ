# writ — baseline self-description

This is the reference row for the comparison matrix. It describes what writ does
today and where it stops, without marketing language.

Sources: `docs/competitive/README.md`, `AGENTS.md`, and
`docs/superpowers/specs/2026-09-06-writ-design.md`.

## What it is

writ is a local-first ledger of steering/corrections for coding agents. A
developer records a rule once with a required rationale, then scopes it to a
project, language, or path pattern. On each diff, `writ audit` selects the
active rules that apply, ranks them, and emits a bounded prompt for the host
agent. Findings can be read back through `writ audit --ingest` and used to gate
a handoff. A local web UI (`writ ui`) provides Inbox, Collection, Detail, and
Health screens for curating the collection. Team sharing is a JSONL file in git,
not a hosted service.

## Honest strengths

- **Local-first.** One SQLite database under XDG paths; no account, server, or
  telemetry. Data stays on the machine.
- **Human approval before activation.** New rules default to `proposed` and
  must be explicitly activated before they enter an audit.
- **Bounded prompt size.** `max_rules` and `max_chars` cap what an audit sends;
  a larger collection does not automatically produce a larger prompt.
- **Host-neutral.** MCP server and `writ install` adapters for Claude Code,
  Codex, Cursor, and OpenCode; `--hook HOST` speaks each host's gate protocol.
- **Evidence is preserved.** Archiving retires a rule without deleting it.
- **writ does not call a model.** The host agent does the reasoning; writ
  retrieves the relevant steering.

## Honest limits

- writ does not fix code itself; it only retrieves relevant steering for the
  host agent to consider.
- Selection cost is not fully bounded by diff size. Many `global` or
  broad-scope rules can still be evaluated during audit selection.
- Structural matchers are optional and currently shell out to `ast-grep`. If
  `ast-grep` is absent or a pattern fails, the rule falls back to scope-only
  selection and the audit still succeeds.
- No duplicate detection; similar rules can accumulate in the collection.
- No hosted sync, registry, signing, or trust model; team sharing is a manual
  file exchange via `writ export` / `writ record --json`.
- Gate support varies by host. Claude Code and Codex use a `Stop` hook; Cursor
  uses a `stop` hook with a follow-up message; OpenCode has no
  stop-equivalent gate, so the loop there is cooperative.
- Recurrence and usefulness reporting require enough history to be meaningful;
  the data is captured from day one, but the dashboards are deferred.
- No built-in PR adapter, transcript scanner, or linter-style enforcement.
