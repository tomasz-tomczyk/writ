# writ for OpenCode

OpenCode gets MCP. **It does not get a gate, and it cannot have one.**

```
writ install opencode
writ install opencode --project
```

That merges `mcp.writ` into `~/.config/opencode/opencode.json`, or into
`opencode.json` at the repository root. Note the shape: `command` is one
array, not a command plus args. The binary must be on `PATH`.

## Why there is no gate

Every hook in `@opencode-ai/plugin` returns `Promise<void>`, and
`session.idle` reaches only the fire-and-forget `event` hook. Nothing
there can block a turn or inject a prompt. There is no stop-equivalent
to hang a gate on.

So `writ audit --hook opencode` is rejected rather than accepted and
quietly ignored. Promising a gate the host cannot enforce is worse than
a missing one: the reader stops checking.

Sources: [OpenCode plugins](https://opencode.ai/docs/plugins/), and the
`Hooks` interface in
[`packages/plugin/src/index.ts`](https://github.com/sst/opencode/blob/dev/packages/plugin/src/index.ts).

## What the plugin does instead

`plugins/writ.ts` is a nag. `tool.execute.after` may change the text of
a tool result, so after the agent's **first** write of a session the
plugin runs the audit and appends the learnings that apply to that
result. The agent reads them in the place it is already looking.

Once per session, and only after a write. A rule list appended to every
edit is noise, and noise gets ignored, which costs more than it teaches.

It fails silent. A missing binary, a directory git does not manage, and
an empty diff all end the hook with the tool result untouched. A broken
nag must never look like a failed edit.

To use it, copy the file into `.opencode/plugins/` in your project or
into `~/.config/opencode/plugins/` for every project.

## Files

| File | What it is |
| --- | --- |
| `opencode.json` | The MCP registration, as `writ install opencode` writes it |
| `plugins/writ.ts` | The after-write nag |
| `AGENTS.snippet.md` | The block to paste into your `AGENTS.md` |

The `AGENTS.md` block is the part that matters most here, because it is
the only thing that asks for the audit at the end of a turn. It says
plainly that nothing enforces it. writ never writes into that file:
paste the block yourself.
