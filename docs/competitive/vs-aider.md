# Aider — competitive brief

Aider is an open-source, terminal-first AI pair programmer. It edits code
inside a local git repository, sends the repository context to a
user-chosen LLM, and applies changes as diffs. This note compares Aider to
writ as a steering/curated-memory tool, not as a general code editor.

Sources cited inline; unknowns are marked explicitly.

## What it is

Aider is a Python CLI (`aider-chat` on PyPI) that runs in a terminal and lets a
developer pair-program with an LLM. It can add files to a chat context (read-only
or editable), propose edits using SEARCH/REPLACE-style diff blocks, run lint/test
commands, and auto-commit results to git.

Key documented capabilities:

- Multi-file code editing in a git repo via diff/whole/udiff/editblock formats.
- Repository map: a tree-sitter-based, token-budgeted summary of key symbols
  across the repo, sent with each request.
- Chat modes: `code`, `ask`, `architect`, `help`; architect mode uses a separate
  "editor" model to translate a plan into file edits.
- Convention files: a `CONVENTIONS.md` (or any markdown/text file) loaded with
  `/read`, `--read`, or the `read:` list in `.aider.conf.yml`.
- In-chat slash commands (`/add`, `/drop`, `/map`, `/run`, `/test`, `/lint`, etc.).
- Git integration with auto-commits, `/undo`, and `(aider)` attribution.
- Connection to many hosted and local LLMs via BYO API key.

Sources: [Aider homepage](https://aider.chat/),
[repo-map docs](https://aider.chat/docs/repomap.html),
[conventions docs](https://aider.chat/docs/usage/conventions.html),
[commands docs](https://aider.chat/docs/usage/commands.html),
[chat modes docs](https://aider.chat/docs/usage/modes.html),
[git integration docs](https://aider.chat/docs/git.html),
[GitHub repository](https://github.com/Aider-AI/aider).

## Goals

Aider's primary goal is to edit code in a local git repo through conversation.
Steering is secondary: conventions and the repo map exist to make the editing
session more accurate, not to provide a durable, curated rule layer that survives
across sessions or agents.

## Memory / steering model

Aider has several mechanisms that sit near the "steering" layer, but none are a
durable, structured ledger of approved rules with required rationale:

1. **Convention files.** A plain-text/markdown file (commonly `CONVENTIONS.md`)
   is loaded read-only into the chat context. It carries style preferences,
   library choices, testing rules, etc. It is authored by the user or copied
   from the [community conventions repo](https://github.com/Aider-AI/conventions).
   There is no required rationale field, no status lifecycle, no scope model,
   and no audit selection against a diff. It is simply included as context.

2. **Repo map.** A compact, auto-generated symbol map of the whole repo is sent
   with each request. It helps the model understand dependencies and existing
   abstractions. It is generated, not authored, and it is not a rule or
   correction.

3. **Chat history.** Aider writes `.aider.chat.history.md` and
   `.aider.input.history` files in the project by default. `restore-chat-history`
   is off by default. Chat history is a transcript, not a structured memory.
   There is no documented mechanism to extract decisions or rules from past
   chats and surface them automatically in new sessions.

4. **No cross-session persistent memory.** Aider starts each session cold.
   Feature requests for persistent cross-session memory exist in the issue
   tracker (e.g., GitHub issue #5371), but this is not a shipped feature as of
   the research date.

Sources:
[conventions docs](https://aider.chat/docs/usage/conventions.html),
[repo-map docs](https://aider.chat/docs/repomap.html),
[config options](https://aider.chat/docs/config/aider_conf.html),
[chat history compression article](https://tinker-ai.com/guides/aider-chat-history-compression/),
[GitHub issue #5371](https://github.com/aider-ai/aider/issues/5371) (open feature request).

## Local vs cloud

Aider is local-first in execution: it runs in the terminal, reads the local git
repo, and writes files locally. However, it is not local-only in data flow:

- The tool itself is free, open-source, and runs locally.
- It requires a user-supplied LLM API key; prompts and code are sent to the
  chosen provider (OpenAI, Anthropic, Google, DeepSeek, OpenRouter, local
  Ollama, etc.).
- Chat history is written locally to `.aider.chat.history.md`, but the same
  content has already been sent to the model provider.
- There is no Aider-operated cloud service, no account, and no hosted sync.
- Analytics are opt-in (randomized by default) and can be disabled; a custom
  PostHog endpoint can be configured. Telemetry behavior is documented in the
  sample config.

Source: [Aider FAQ](https://aider.chat/docs/faq.html),
[sample `.aider.conf.yml`](https://aider.chat/docs/config/aider_conf.html).

## Agent integration

Aider is itself an agentic coding tool, not a neutral steering layer for another
agent. It does not expose an MCP server or host hook for external agents as a
primary interface. (Writ's model is the opposite: writ never calls a model; it
feeds rules to a host agent.) Aider can be scripted and has a Python module
entry point, but its design center is a human running aider in a terminal.

## Curation / prune / audit

- **Curation.** Convention files are plain text; users curate them by editing
  markdown. There is no Inbox/Collection/Detail UI, no approval gate, no
  `proposed`/`active`/`archived` status, and no duplicate detection.
- **Prune.** The repo map is automatically refreshed; no manual pruning is
  needed. Convention files must be edited by hand. There is no archive that
  preserves evidence without selecting it.
- **Audit.** Aider does not have a diff-time rule-audit step. The model is free
  to follow or ignore conventions; nothing reports which conventions were
  violated. The `lint-cmd`/`test-cmd` loop catches mechanical failures, not
  rule violations.

## Overlap with writ

Both tools care about durable steering:

- Aider's `CONVENTIONS.md` and writ's `learning` both encode how the agent
  should write code.
- Both are local-first and open-source.
- Both can scope context to a repo (Aider via the project-root config and
  convention file; writ via `project:` scope).

## Gaps vs writ

Relative to writ's model, Aider lacks:

1. **Required rationale.** A convention file can contain rules without reasons.
   A rationale-free rule is harder to transfer to cases the author did not
   foresee.
2. **Status lifecycle.** There is no `proposed` → `active` → `archived` flow.
   Every rule in the convention file is live once loaded.
3. **Scope model.** No equivalent to writ's `global` / `project:` /
   `language:` / `glob:` applicability scopes that AND across kinds and OR
   within a kind.
4. **Diff-time selection and bounded prompt.** The convention file is included
   in full (or not at all). There is no audit that selects only relevant rules
   for a given diff, ranks them, or caps prompt size.
5. **Findings and ingest.** Aider does not ask the model to report violations
   of conventions back into a structured finding store, and it does not gate a
   handoff on open blocking findings.
6. **Cross-session persistence.** Aider sessions start cold. Past decisions in
   chat history are not extracted into a durable memory store.
7. **Collection UI.** Convention files are edited in a text editor; there is no
   Inbox/Collection/Detail/Health UI for curating rules.
8. **Host neutrality.** Aider is the agent; it does not serve as a steering
   layer for Claude Code, Cursor, Codex, or OpenCode the way writ does.

## writ gaps vs Aider

Relative to Aider, writ lacks:

1. **Code editing.** writ does not edit files or run a coding session. It only
   retrieves steering for a host agent.
2. **Repo map.** writ has no generated codebase map; it relies on the host
   agent's own context and on `glob:` / `language:` scopes.
3. **Auto-commit and git workflow.** writ does not commit, diff, or undo code
   changes.
4. **Lint/test repair loop.** writ does not run tests or lint commands after
   edits.
5. **Broad model connectivity.** writ never calls a model at all; Aider can
   connect to many providers and local models.
6. **Terminal pair-programming UX.** writ's terminal surface is audit/findings;
   it is not an interactive coding assistant.

## Sources

1. Aider homepage and documentation: https://aider.chat/docs
2. Repository map: https://aider.chat/docs/repomap.html
3. Specifying coding conventions: https://aider.chat/docs/usage/conventions.html
4. YAML config file (`.aider.conf.yml`): https://aider.chat/docs/config/aider_conf.html
5. In-chat commands: https://aider.chat/docs/usage/commands.html
6. Chat modes: https://aider.chat/docs/usage/modes.html
7. Git integration: https://aider.chat/docs/git.html
8. FAQ: https://aider.chat/docs/faq.html
9. Aider GitHub repository: https://github.com/Aider-AI/aider
10. Community conventions repo: https://github.com/Aider-AI/conventions
11. Aider chat history compression guide (third-party): https://tinker-ai.com/guides/aider-chat-history-compression/
12. GitHub issue #5371 — persistent cross-session memory request: https://github.com/aider-ai/aider/issues/5371

## Suggested matrix values for synthesis

| Dimension | Suggested value for aider |
|---|---|
| Local-first / data leaves machine | Tool runs locally and stores history locally, but sends prompts to user's chosen LLM provider. No Aider-hosted cloud, no account. |
| Durable rules with rationale | Convention files (`CONVENTIONS.md`) encode preferences, but no required rationale field, no status lifecycle, no structured rule entity. |
| Human approve before active | No `proposed → active` gate; rules in a loaded convention file are live immediately. |
| Scope model | Scope is implicit: project-root `.aider.conf.yml`, per-directory config, or manual `/read`. No formal `project:` / `language:` / `glob:` rule scopes. |
| Diff-time selection / audit | Convention files are included whole in chat context; no diff-time rule selection, ranking, or bounded prompt. |
| Prune / archive | No archive; convention files are edited by hand. Repo map is auto-generated/refreshed. |
| UI for collection | No collection UI; conventions are plain text edited in any editor. Aider has an optional browser UI for chat, not for rule curation. |
| Terminal findings | Aider emits edits and lint/test output in the terminal, not structured findings against a rule set. |
| Multi-agent host neutrality | Aider is itself the coding agent; it does not act as a neutral steering layer for other agents. |
| Open source | Yes; Apache 2.0. |
| Pricing posture | Free, open-source CLI; user pays their own LLM API usage (BYO key). |

Date researched: 2026-09-07.
