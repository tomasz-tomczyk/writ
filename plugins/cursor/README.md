# writ for Cursor

Cursor gets both halves: MCP for discovery, and a `stop` hook for the
gate.

```
writ install cursor
writ install cursor --project
```

That merges `mcpServers.writ` into `mcp.json` and a `stop` entry into
`hooks.json`, in `~/.cursor/` or in `<repo>/.cursor/`. The binary must
be on `PATH`.

The files in this directory are the same configuration packaged as a
Cursor plugin, for a reader who would rather install one.

## The gate differs in kind

Cursor does not block the stop. Its `stop` hook may return a
`followup_message`, and Cursor submits that string as the next user
turn. So `writ audit --hook cursor` prints
`{"followup_message":...}` on stdout and exits `0`.

That is why the retry cap reads a count here and a boolean on the other
two hosts. Cursor sends `loop_count`, which starts at `0` and counts the
follow-ups the hook has already submitted for this conversation. Cursor
also caps it: `loop_limit` defaults to `5` per script, and the shipped
configuration writes that `5` out so the cap is visible in the file
rather than implied.

Source: [Cursor hooks, `stop`](https://cursor.com/docs/agent/hooks).

## Merging into a live hooks.json

`~/.cursor/hooks.json` usually already has entries, and `stop` is a flat
array rather than the matcher groups Claude Code and Codex use. writ
appends one element and touches nothing else. It sets `version` only
when the file does not already have one, so a file on a later version
keeps it.

## Files

| File | What it is |
| --- | --- |
| `.cursor-plugin/plugin.json` | The plugin manifest |
| `mcp.json` | The bundled MCP server |
| `hooks/hooks.json` | The `stop` gate |
| `AGENTS.snippet.md` | The block to paste into your `AGENTS.md` |

Cursor discovers `mcp.json` and `hooks/hooks.json` by their names when
the manifest names no path, which is why the manifest here is only
identity and metadata.

Source: [Cursor plugins reference](https://cursor.com/docs/reference/plugins).

Cursor reads `AGENTS.md`, or `.cursor/rules/*.mdc`. writ never writes
into either: paste the block yourself.
