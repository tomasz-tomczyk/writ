# Claude Code / Claude Projects — competitive brief

Objective notes on Anthropic's Claude Code terminal agent and Claude Projects
web workspaces, focused on their persistent-instruction and memory features,
compared against writ, a local-first ledger of steering/corrections for coding
agents.

## What it is

Claude Code is Anthropic's terminal-first agentic coding tool. It runs Claude as
a coding agent that searches a codebase, edits files, runs tests, and manages
git. It is generally available and ships as a native installer or package,
with VS Code / JetBrains extensions and an SDK.

Claude Projects (in claude.ai) is a separate web/workspace feature for Pro,
Max, Team, and Enterprise plans. A Project groups conversations, uploaded
knowledge-base files (up to 20 files per project), custom system-prompt
instructions, and project-specific memory.

This brief focuses on the persistent steering/memory layer that both share:
`CLAUDE.md` style instruction files, auto memory, and project memory.

Sources:
[Claude Code memory docs](https://code.claude.com/docs/en/memory),
[Claude features 2026 summary](https://suprmind.ai/hub/claude/features/),
[Claude Projects setup guide](https://tygartmedia.com/claude-projects/).

## Goals

- Give Claude persistent project instructions so the user does not have to
  repeat conventions, build commands, or architecture every session.
- Let Claude learn and reuse patterns across sessions by writing its own notes
  (auto memory).
- Provide scoped rules (global, user, project, directory, path-specific) that
  load when relevant.
- Support team sharing through committed project files and organization-managed
  policy files.
- For Claude Projects, create a persistent workspace with knowledge files and
  isolated memory for non-terminal use.

## Memory / steering model

Claude Code's steering layer has four documented persistence mechanisms:

| Layer | Authored by | What it carries | How it loads |
|---|---|---|---|
| `CLAUDE.md` files | Human | Standing instructions, conventions, build commands, architecture | Loaded at session start, re-read after `/compact` |
| Auto memory (`MEMORY.md`) | Claude | Patterns learned from corrections, preferences, debugging notes | First 200 lines / 25 KB loaded at session start |
| Memory Tool (`memory_20250818`) | Programmatic agent | Long-running structured memory for API-based custom agents | Read/write on demand via tool calls |
| Subagent memory | Subagent | Per-subagent persistent knowledge store (v2.1.33+) | Loaded for that subagent |

**`CLAUDE.md` files** are the primary explicit steering layer. Claude Code
loads them from multiple locations, concatenated in order:

1. Managed policy `CLAUDE.md` (organization-wide, cannot be overridden)
2. `~/.claude/CLAUDE.md` (user global)
3. Project root `CLAUDE.md` or `.claude/CLAUDE.md`
4. `CLAUDE.local.md` alongside any of the above (personal, gitignored)
5. Subdirectory `CLAUDE.md` files lazily when Claude reads files in those
   directories

Files can import each other with `@filename.md` frontmatter; a common pattern
is `@AGENTS.md` so that tool-agnostic instructions and Claude-specific
instructions live in one place. `/init` can generate a first draft from
existing rule files (Cursor rules, Copilot instructions, AGENTS.md, etc.), and
`/import` can copy supported agent configurations into Claude Code.

**`.claude/rules/`** scales CLAUDE.md with path-scoped rule files. Rules can
load always, intelligently based on a description, only for matching files
(`paths:` or `globs:` frontmatter), or manually when `@`-mentioned.

**Auto memory** is implicit. Claude decides what is worth saving across
sessions (build commands, fixes, preferences, architectural insights) and
writes it under `~/.claude/projects/<project-hash>/memory/`, typically in
`MEMORY.md`. Only the first 200 lines or 25 KB are auto-loaded, so the docs
recommend periodic curation. Users can inspect and edit these files with the
`/memory` command.

**Claude Projects** use the same conceptual split: a persistent system prompt
(Project Instructions) plus uploaded knowledge-base files. Conversation
history is grouped within the Project, and memory is isolated per Project.

In all cases, Claude itself is the reasoning engine; the instruction files and
memory are context, not a separate audit layer.

Sources:
[Claude Code memory docs](https://code.claude.com/docs/en/memory),
[Orchestrator.dev memory best practices 2026](https://orchestrator.dev/blog/2026-04-06--claude-code-agent-memory-2026/),
[Skills Playground memory guide](https://skillsplayground.com/guides/claude-code-memory/),
[The Prompt Shelf memory guide](https://thepromptshelf.dev/blog/claude-code-memory-auto-memory-system-2026/).

## Local vs cloud

Claude Code and Claude Projects are proprietary, cloud-connected services, not
local-first software:

- **Project `CLAUDE.md` files** live on the local disk and travel with git.
- **User/global `CLAUDE.md`** in `~/.claude/` stays on one machine unless synced
  manually.
- **Managed policy `CLAUDE.md`** and organization settings are pushed from
  Anthropic/enterprise dashboards.
- **Auto memory** is stored locally under `~/.claude/projects/<project-hash>/`,
  but project identity is derived from the Anthropic-side project/workspace.
- **Codebase indexing / Cloud agents / Claude Projects** run on or pass through
  Anthropic-managed infrastructure. The web product is inherently cloud-hosted.
- **Telemetry and data use** are governed by Anthropic's terms; Enterprise
  offers admin controls, but the product is account-based.

writ, by contrast, stores rules in a local SQLite database under XDG paths, has
no account, no server, and calls no model.

Sources:
[Claude Code memory docs](https://code.claude.com/docs/en/memory),
[Anthropic platform system prompts docs](https://platform.claude.com/docs/en/release-notes/system-prompts),
[Claude Projects guide](https://tygartmedia.com/claude-projects/).

## Agent integration

Claude Code is itself the host agent. Its instruction/memory layers are
intrinsic to the product:

- **Claude Code terminal agent** — loads `CLAUDE.md`, auto memory, and scoped
  rules automatically.
- **`/memory`, `/init`, `/import`, `/compact`** — built-in commands for memory
  management and configuration bootstrapping.
- **Subagents** — can carry their own persistent memory since v2.1.33.
- **Multi-agent runs** — a lead agent can dispatch parallel background agents
  and merge results, watched via `claude agents`.
- **Claude Code SDK / API agents** — can use the Memory Tool for structured
  cross-session persistence.
- **VS Code / JetBrains extensions** — inline edits with the same project
  context.
- **MCP servers** — Claude Code can connect to external MCP servers, including
  third-party memory tools.

writ integrates with Claude Code via MCP and `writ install`, and emits a
host-specific `--hook claude-code` stop message. writ is an external steering
ledger, not part of Claude Code.

Sources:
[Claude Code memory docs](https://code.claude.com/docs/en/memory),
[Claude features summary](https://suprmind.ai/hub/claude/features/),
[Orchestrator.dev memory best practices](https://orchestrator.dev/blog/2026-04-06--claude-code-agent-memory-2026/).

## Curation / prune / audit

**Curation.** `CLAUDE.md` and `.claude/rules/*.md` files are edited directly or
generated with `/init`. Auto memory files are inspected and edited with `/memory`
or a text editor. There is no separate "inbox" or approval state for new rules;
a committed `CLAUDE.md` is active immediately. Auto memory is curated by the
user after Claude writes it.

**Prune.** There is no documented archive/retire operation equivalent to
`writ archive`. Rules are deleted by deleting the file; auto memory notes are
deleted by editing `MEMORY.md`. **Marked as unknown:** whether deleted Anthropic
memory data is retained as evidence.

**Audit.** Claude Code does not run a diff-time rule selection that asks the
agent to report findings back. It injects instructions into context and relies
on the model to follow them. The `/review` or PR review features are
model-driven reviews, not a structured audit against a durable rule ledger.
There is no per-rule "selected vs applied" metric analogous to writ's
`times_selected` / `times_applied` counters.

## Overlap with writ

Both tools address the same underlying problem: making coding agents more
consistent by giving them durable, reusable guidance.

- Both persist instructions across sessions.
- Both support project-scoped rules (Claude via project `CLAUDE.md`, writ via
  `project:` scopes).
- Both support file-pattern scoping (Claude via `.claude/rules/` `paths:`/
  `globs:`, writ via `glob:`).
- Both can be shared with a team, though Claude's project files travel with the
  repo and writ uses `writ export` / JSONL exchange.
- Both treat written rationale/conventions as a source of truth the agent should
  consult.

## Gaps vs writ

For writ's specific use case — durable, human-curated steering/corrections
that gate code handoff — Claude Code / Projects have notable gaps:

- **No required rationale per rule.** `CLAUDE.md` entries are free-form
  instructions; they do not require a separate `rationale` field that explains
  why the rule applies.
- **No explicit proposed → active gate.** A saved `CLAUDE.md` or rule file is
  active immediately. There is no inbox for reviewing candidate rules before
  they enter context.
- **No blocking vs advisory distinction.** Instructions are soft prompts; there
  is no equivalent of writ's `blocking` boolean that gates whether an unfixed
  finding stops a handoff.
- **No structured diff-time audit prompt.** Claude injects rules into the
  system prompt. It does not select rules against the current diff, cap them at
  `max_rules`/`max_chars`, and ask the agent to return findings.
- **No findings ingest / handoff gate.** writ's `--ingest` reads reported
  violations back and can block on open blocking findings. Claude Code has no
  equivalent terminal-based finding ingestion flow.
- **No evidence-preserving archive.** Deleting a rule or memory note removes it;
  there is no documented archival state that keeps the rule as evidence while
  stopping selection.
- **No host-neutral neutrality.** Claude's instruction layer is Claude-only.
  writ is deliberately host-neutral (Claude Code, Codex, Cursor, OpenCode).
- **Model-dependent curation of auto memory.** Auto memory is written by Claude
  itself, then curated by the user. writ rules are written and approved by the
  human before activation.
- **Vendor-controlled data.** Project files are local, but managed policy,
  Claude Projects content, cloud agents, and account data live on or pass
  through Anthropic's servers. writ is local-only.

## writ gaps vs Claude Code / Projects

Claude Code / Projects are stronger where writ deliberately does not play:

- **Integrated agent experience.** Claude Code is the terminal agent; Claude
  Projects is the web workspace. writ requires a separate host agent and CLI.
- **AI-native workflows.** Plan mode, inline edits, multi-agent runs,
  background/cloud agents, PR review, and automation triggers are built in.
  writ does not write or execute code.
- **Automatic memory extraction.** Auto memory captures patterns without manual
  authoring. writ has no transcript scanner or automatic rule extraction.
- **Cross-session factual memory.** Claude's memory layers remember preferences,
  account state, and project facts across arbitrary sessions. writ only knows
  the rules in its ledger.
- **Team-wide policy enforcement at scale.** Organization-managed `CLAUDE.md`
  and enterprise dashboards can push standards with SSO and admin controls.
  writ has no hosted admin layer.
- **Codebase-aware retrieval.** Claude indexes and retrieves relevant files
  automatically. writ selects only from its rule ledger.
- **Rich web workspace.** Claude Projects provides a chat UI, knowledge files,
  and isolated memory for non-developer workflows. writ's UI is focused on the
  rule collection.

## Sources

1. Claude Code memory docs — https://code.claude.com/docs/en/memory
2. Claude Code docs index (llms.txt) — https://code.claude.com/docs/llms.txt
3. Claude features 2026 summary — https://suprmind.ai/hub/claude/features/
4. Claude Code & Agent Memory: Best Practices for 2026 — https://orchestrator.dev/blog/2026-04-06--claude-code-agent-memory-2026/
5. Skills Playground: Claude Code Memory — https://skillsplayground.com/guides/claude-code-memory/
6. The Prompt Shelf: Claude Code Memory System (2026) — https://thepromptshelf.dev/blog/claude-code-memory-auto-memory-system-2026/
7. The Prompt Shelf: Memory Persistence Guide — https://thepromptshelf.dev/blog/claude-code-memory-persistence-guide-2026/
8. Anthropic system prompts docs — https://platform.claude.com/docs/en/release-notes/system-prompts
9. Claude Projects setup guide — https://tygartmedia.com/claude-projects/
10. Claude Code system prompts repository (unofficial) — https://github.com/Piebald-AI/claude-code-system-prompts

Reviewed 2026-09-07.

## Suggested matrix values for synthesis

| Dimension | Suggested value for claude-code-memory |
|---|---|
| Local-first / data leaves machine | Project `CLAUDE.md` files are local and in git; auto memory is local under `~/.claude/`. Cloud agents, Claude Projects, managed policy, and account data pass through Anthropic servers. Not local-first by design. |
| Durable rules with rationale | Human-authored instructions in `CLAUDE.md` / `.claude/rules/`; no required separate `rationale` field. Auto memory stores learned notes, not rules with rationale. |
| Human approve before active | No `proposed → active` inbox; committed `CLAUDE.md` rules are active immediately. Auto memory is written by Claude and then user-curated. |
| Scope model | Hierarchical load order (managed → user → project → directory → local) plus `.claude/rules/` with `paths:`/`globs:` frontmatter. No repo-identity or language scopes. |
| Diff-time selection / audit | Instructions are injected into context; no diff-time selection that asks for findings back. |
| Prune / archive | Edit/delete files or memory notes; no documented archival equivalent to `writ archive`. |
| UI for collection | `/memory` command and file editor for auto memory; no dedicated "collection health" UI. Claude Projects has a web workspace for instructions/knowledge files. |
| Terminal findings | No structured terminal findings/ingest flow; hooks are host-specific stop messages, not rule-violation ingestion. |
| Multi-agent host neutrality | Claude-specific; other agents need their own instructions or a shared `AGENTS.md` import. |
| Open source | No; Claude Code and Claude Projects are proprietary Anthropic products. |
| Pricing posture | Freemium chat (Free) / paid Pro/Max/Team/Enterprise; Claude Code requires paid plan; core memory/steering is part of the product, not a separate purchase. |
