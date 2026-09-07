# writ for Claude Code

This plugin gives Claude Code two things: a way to write a learning
down, and a gate that makes reading them back not optional.

- **`/record`** binds to `writ record --activate`.
- **A `Stop` hook** runs `writ audit --hook claude-code` on the diff the
  agent just wrote. Any learning that applies, blocking or advisory,
  sends the agent back with it before the turn hands over.

The binary must be on `PATH`. Install it with
`cargo install --path crates/writ-cli`, which installs a binary named
`writ`.

## Install

```
/plugin marketplace add tomasztomczyk/writ
/plugin install writ@writ
```

## Discovery, separately

The hook is the gate. Discovery is MCP, and Claude Code reads its MCP
configuration from `.mcp.json`, not from this plugin:

```json
{ "mcpServers": { "writ": { "command": "writ", "args": ["mcp"] } } }
```

That server exposes three tools, `writ_record`, `writ_audit`, and
`writ_edit`. All are shells over the same code the CLI runs.

Claude Code reads `CLAUDE.md` and not `AGENTS.md`. If your instructions
live in `AGENTS.md`, bridge it with an `@AGENTS.md` import line or a
symlink.

## The retry cap

The hook blocks once. Claude Code sets `stop_hook_active` on the payload
it sends to a `Stop` hook that already fired for this turn, and writ
reads it: on the second pass the gate lets the turn stop.

Without that cap an agent that reports `ignored` produces the same
finding on the next audit, forever, because writ sees each audit fresh
and cannot tell one turn from the next.

## What each host can enforce

An advisory learning is not a weaker gate. It rides in the same prompt.
What `blocking` decides is whether an unfixed violation stops the work
when the findings come back, not whether the agent reviews at all.

| Host | Gate | What `writ audit --hook` emits |
| --- | --- | --- |
| Claude Code | `Stop` hook | exit 2, the findings on stderr |
| Codex CLI | `Stop` hook | `{"decision":"block","reason":...}`, exit 0 |
| Cursor | `stop` hook | `{"followup_message":...}`, exit 0 |
| OpenCode | **none** | — |

**OpenCode cannot enforce a gate.** Every one of its plugin hooks
returns `Promise<void>`, and `session.idle` reaches only the
fire-and-forget `event` hook, so nothing there can block a turn or
inject a prompt. Its `AGENTS.md` can ask the agent to run the audit and
a `tool.execute.after` plugin can nag after each write, but neither
stops anything. `writ audit --hook opencode` is rejected rather than
accepted and quietly ignored, because promising a gate the host cannot
enforce is worse than a missing one.
