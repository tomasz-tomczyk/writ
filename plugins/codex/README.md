# writ for Codex CLI

Codex gets both halves: MCP for discovery, and a `Stop` hook for the
gate.

```
writ install codex
writ install codex --project
```

That merges `[mcp_servers.writ]` into `config.toml` and a `Stop` entry
into `hooks.json`, in `~/.codex/` or in `<repo>/.codex/`. The binary
must be on `PATH`.

The files in this directory are the same configuration packaged as a
Codex plugin, for a reader who would rather install one.

## The gate

`writ audit --hook codex` speaks Codex's `Stop` protocol: it prints
`{"decision":"block","reason":...}` on stdout and exits `0`. For that
event, `decision: "block"` does not reject the turn. Codex builds a new
continuation prompt out of the reason, so the agent comes back with its
own rules in front of it.

Codex sends `stop_hook_active` on the second entry, and writ reads it:
one block, then the turn is allowed to end. Without that cap an agent
that reports `ignored` produces the same finding forever, because writ
sees each audit fresh and cannot tell one turn from the next.

Source: [Codex hooks, `Stop`](https://developers.openai.com/codex/hooks).

## Trust the hook once

**Codex will not run a hook you have not trusted.** It records trust
against the hook's hash, so a new or edited hook is skipped until you
review it. Run `/hooks` in the CLI, find the writ entry, and trust it.
If the audit never seems to fire, that is the first thing to check.

Project-local hooks load only when the project's `.codex/` layer is
trusted at all.

Source: [Codex hooks, review and trust](https://developers.openai.com/codex/hooks).

## Why hooks.json and not an inline table

Codex reads hooks from either `hooks.json` or an inline `[hooks]` table
in `config.toml`, and it warns when one layer holds both. writ puts the
MCP server in `config.toml` and the hook in `hooks.json`, so the two
never collide.

The TOML edit is a text edit, not a reprint. A hand-written
`config.toml` is full of comments, and a round trip through a parser
drops every one of them.

## Files

| File | What it is |
| --- | --- |
| `.codex-plugin/plugin.json` | The plugin manifest |
| `.mcp.json` | The bundled MCP server |
| `hooks/hooks.json` | The `Stop` gate |
| `AGENTS.snippet.md` | The block to paste into your `AGENTS.md` |

Codex reads `AGENTS.md`. writ never writes into it: paste the block
yourself.
