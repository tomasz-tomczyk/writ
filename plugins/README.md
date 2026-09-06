# writ host integrations

Four hosts, two things each: an MCP registration so the agent can find
writ, and a gate so reading the learnings back is not optional.

`writ install <host>` writes both. Everything under this directory is
the same configuration in the form each host distributes it, for a
reader who would rather install a plugin than run a command.

```
writ install claude-code
writ install codex --project
writ install cursor --print
writ install opencode
```

`--print` shows the merged file and changes nothing. `--project` targets
the repository instead of your home directory. `--force` replaces a writ
entry that is already there and differs from what writ would write.

Every write merges. These files hold other tools, and none of them is
overwritten: one key or one array element goes in, and the rest of the
document stays. The old file is copied aside first and the copy's path
is printed. Running the command twice adds nothing the second time.

## What each host can do

| Host | Gate | Protocol `writ audit --hook` emits | Plugin format |
| --- | --- | --- | --- |
| Claude Code | `Stop` hook | exit 2, findings on stderr | `.claude-plugin/plugin.json` |
| Codex CLI | `Stop` hook | `{"decision":"block","reason":...}`, exit 0 | `.codex-plugin/plugin.json` |
| Cursor | `stop` hook | `{"followup_message":...}`, exit 0 | `.cursor-plugin/plugin.json` |
| OpenCode | **none** | — | `.opencode/plugins/*.ts` |

**OpenCode cannot enforce a gate.** Every hook in its plugin API returns
`Promise<void>`, and `session.idle` reaches only the fire-and-forget
`event` hook, so nothing there can block a turn or inject a prompt. It
gets MCP, an `AGENTS.md` block that asks the agent to run the audit, and
a plugin that appends the rules to the first write of a session. None of
those is a gate. `writ audit --hook opencode` is rejected rather than
accepted and quietly ignored.

## Where each host keeps its configuration

| Host | MCP, home | MCP, project | Gate |
| --- | --- | --- | --- |
| Claude Code | `~/.claude.json` | `.mcp.json` | `.claude/settings.json` |
| Codex CLI | `~/.codex/config.toml` | `.codex/config.toml` | `.codex/hooks.json` |
| Cursor | `~/.cursor/mcp.json` | `.cursor/mcp.json` | `.cursor/hooks.json` |
| OpenCode | `~/.config/opencode/opencode.json` | `opencode.json` | none |

Sources: [Claude Code MCP](https://docs.claude.com/en/docs/claude-code/mcp),
[Claude Code hooks](https://docs.claude.com/en/docs/claude-code/hooks),
[Codex MCP](https://developers.openai.com/codex/mcp),
[Codex hooks](https://developers.openai.com/codex/hooks),
[Cursor MCP](https://cursor.com/docs/context/mcp),
[Cursor hooks](https://cursor.com/docs/agent/hooks),
[OpenCode MCP](https://opencode.ai/docs/mcp-servers/),
[OpenCode plugins](https://opencode.ai/docs/plugins/).

## The instructions block

Each host reads a different file, and writ never writes into it. Those
are your words, and a tool that edits them has to be trusted with the
whole file.

| Host | File | Block to paste |
| --- | --- | --- |
| Claude Code | `CLAUDE.md` | `claude-code/CLAUDE.snippet.md` |
| Codex CLI | `AGENTS.md` | `codex/AGENTS.snippet.md` |
| Cursor | `AGENTS.md` | `cursor/AGENTS.snippet.md` |
| OpenCode | `AGENTS.md` | `opencode/AGENTS.snippet.md` |

Claude Code reads `CLAUDE.md` and never `AGENTS.md`. An `@AGENTS.md`
import line or a symlink is the documented bridge.

The block tells the agent to call `writ_record` when the user corrects
it on something that would apply again. Until a session scanner exists,
that is the only capture that happens without someone typing a command.

## The skill

`claude-code/skills/record/SKILL.md` teaches an agent how to write a
good learning: how to scope it, how to write a rationale that survives
an argument, and when to reinforce instead of writing a second row.

It ships once. Codex and Cursor both read a `skills/` directory, so a
copy would work in either, and a copy is exactly the drift invariant 8
exists to stop. Symlink it, or point your host at the file.
