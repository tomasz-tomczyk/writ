# Cursor Memories / Rules — competitive brief

Objective notes on Cursor's memory and steering features compared against writ,
a local-first ledger of steering/corrections for coding agents.

## What it is

Cursor is a proprietary AI-native IDE and coding agent. Its memory/steering
surface has two layers:

- **Rules** — authored instructions injected into the agent's system prompt.
  Project Rules live in `.cursor/rules/*.mdc` files and are version-controlled;
  User Rules are global preferences in Cursor settings; Team Rules are managed
  from the Cursor dashboard on Teams/Enterprise plans. A plain `AGENTS.md` file
  works as a lighter alternative. As of late 2025, Rules are the primary,
  documented mechanism for persistent agent steering.
- **Memories** — a feature that automatically extracted facts from agent
  conversations, scoped them per project per user, and surfaced them in future
  chats. It was introduced in Cursor 1.0 (June 2025), declared GA in 1.2 (July
  2025), and removed in Cursor 2.1.x (November 2025). The official migration
  path is to export old memories into a Project Rule (`.mdc` file). A separate
  "Memories" capability still exists for Cloud Agent *automations*, where the
  agent can read/write a `MEMORIES.md` file across runs of the same automation.

This brief focuses on the IDE-level Memories/Rules product, with notes on the
automation memory where relevant.

Sources:
[Cursor rules docs](https://cursor.com/docs/rules),
[Cursor rules help](https://cursor.com/help/customization/rules),
[Cursor 1.0 changelog](https://cursor.com/changelog/1-0),
[Cursor 1.2 changelog](https://cursor.com/changelog/1-2),
[Cursor 2.1 changelog](https://cursor.com/changelog/2-1),
[Forum: Memories removed in 2.1.x](https://forum.cursor.com/t/memories-not-showing/143820),
[Cursor automations docs](https://cursor.com/docs/cloud-agent/automations).

## Goals

- Give Cursor's agent persistent instructions so the user does not have to
  repeat conventions, stack choices, or workflows every session.
- Let teams enforce or share standards through version-controlled Project Rules
  or dashboard-managed Team Rules.
- (For the now-removed Memories feature) Automatically capture facts from
  conversation and reuse them across sessions without manual authoring.
- (For Cloud Agent automations) Let long-running agents persist notes across
  runs of the same automation.

## Memory / steering model

**Rules are authored, static prompts.** A Project Rule is a markdown file with
YAML frontmatter that controls when it loads:

| Rule type | Trigger |
|---|---|
| `Always Apply` | Included in every Agent chat |
| `Apply Intelligently` | Agent reads the `description` and decides relevance |
| `Apply to Specific Files` | Loaded when a matching file is in context |
| `Apply Manually` | Only when `@`-mentioned in chat |

Frontmatter fields (`alwaysApply`, `description`, `globs`) determine the
behavior. Rules can reference files with `@filename`. Precedence is documented
as **Team Rules → Project Rules → User Rules**. Rules apply only to Agent
(Chat); they do not affect Tab completion, Inline Edit, or Bugbot PR reviews.
The docs recommend keeping each rule under 500 lines.

**Memories (removed IDE feature) were extracted, not authored.** Cursor
proposed facts in the background or on explicit "remember this" requests,
required user approval for background-generated memories, and stored them per
project on an individual level. The extraction algorithm, storage format, and
retrieval mechanics were not documented in detail. Forum reports noted that
memories did not always sync across machines, did not persist in dev
containers, and lacked import/export until the removal export command.

**Automation memories** are a different, documented mechanism: a named file
(usually `MEMORIES.md`) stored outside the automation's working filesystem that
the agent can read and write across runs.

In all cases, Cursor itself is the reasoning engine; the rules/memories are
context, not a separate audit layer.

Sources:
[Cursor rules docs](https://cursor.com/docs/rules),
[Cursor rules help](https://cursor.com/help/customization/rules),
[Cursor 1.0 changelog](https://cursor.com/changelog/1-0),
[Cursor 1.2 changelog](https://cursor.com/changelog/1-2),
[Forum: Saved Memories viability](https://forum.cursor.com/t/saved-memories-in-cursor-and-their-viability/128968),
[Cursor automations docs](https://cursor.com/docs/cloud-agent/automations).

## Local vs cloud

Cursor is a proprietary, cloud-connected IDE, not local-first software:

- **Project Rules** live as files in the repo (local disk) and travel with git.
- **User Rules in settings** are stored on the user's Cursor account and sync
  across signed-in machines.
- **User rule files** in `~/.cursor/rules` stay on one machine and do not sync.
- **Team Rules** are stored on Cursor's servers and pushed to team members.
- **Codebase indexing** sends encrypted file-path hashes and embeddings to
  Cursor's servers (Turbopuffer); code content is not stored in plaintext.
- **Cloud agents / automations** run in Cursor-managed infrastructure.

Cursor offers Privacy Mode and Enterprise controls, but the product is
fundamentally a hosted, account-based service. writ, by contrast, stores rules
in a local SQLite database under XDG paths, has no account, and calls no model.

Sources:
[Cursor rules help](https://cursor.com/help/customization/rules),
[Cursor privacy / data use docs](https://cursor.com/data-use),
[Codersera Cursor guide 2026](https://codersera.com/blog/cursor-ide-complete-guide-2026/).

## Agent integration

Cursor is the host agent; rules and memories are context for its own Agent
chat. Integration points include:

- **Built-in Agent (Chat / Composer)** — loads rules automatically based on
  frontmatter and user mentions.
- **Cursor CLI** — command-line agent that can run with the same project
  context.
- **Cloud Agents / Automations** — run in Cursor's cloud with their own
  memory file and can load project hooks.
- **Hooks** — Cursor supports `hooks.json` at project or user level for
  `sessionStart`, `stop`, `beforeShellExecution`, `afterFileEdit`, and many
  others. Third-party hooks (e.g., Claude Code stop hooks) can be loaded.
- **MCP servers** — Cursor can connect to external MCP servers, including
  third-party memory servers; this is the current path for cross-tool or
  automatic memory.

writ integrates with Cursor via MCP and `writ install`, and emits a
host-specific `--hook cursor` stop message for gating. writ does not run as
part of Cursor itself; it is an external steering ledger.

Sources:
[Cursor rules docs](https://cursor.com/docs/rules),
[Cursor hooks docs](https://cursor.com/docs/hooks),
[Cursor automations docs](https://cursor.com/docs/cloud-agent/automations).

## Curation / prune / audit

**Curation.** Rules are edited as `.mdc` files or through Cursor's Customize →
Rules UI. There is no separate "inbox" or approval state; a saved rule is
active immediately (unless it is a Team Rule saved as a draft or disabled by
enforcement settings). For the removed Memories feature, background-generated
memories required approval before saving.

**Prune.** There is no documented archive/retire operation equivalent to
`writ archive`. Rules are deleted by deleting the file; Team Rules are deleted
from the dashboard. Automation memory files can be deleted from the tool UI or
by the agent. **Marked as unknown:** whether deleted Cursor data is retained as
evidence.

**Audit.** Cursor does not run a diff-time rule selection that asks the agent
to report findings back. It injects rules into context and relies on the model
to follow them. Bugbot and Cloud Agent automations can review code, but those
are model-driven review features, not a structured audit against a durable rule
ledger. Cursor provides usage/conversation analytics for Teams/Enterprise, but
not a per-rule "selected vs applied" metric.

## Overlap with writ

Both tools address the same underlying problem: making coding agents more
consistent by giving them durable, reusable guidance.

- Both persist instructions across sessions.
- Both support project-scoped rules (Cursor via `.cursor/rules`, writ via
  `project:` scopes).
- Both support file-pattern scoping (Cursor via `globs`, writ via `glob:`).
- Both can be shared with a team, though Cursor's Project Rules travel with the
  repo while writ uses `writ export` / JSONL exchange.
- Both treat the terminal as a primary surface for enforcement (Cursor via
  hooks/CLI, writ via `writ audit`).

## Gaps vs writ

For writ's specific use case — durable, human-curated steering/corrections
that gate code handoff — Cursor has notable gaps:

- **No required rationale per rule.** Cursor rules are free-form prompts; they
  do not require a separate `rationale` field that explains why the rule
  applies.
- **No explicit proposed → active gate.** A saved Project Rule is active
  immediately. There is no inbox for reviewing candidate rules before they
  enter context.
- **No blocking vs advisory distinction.** Cursor rules are soft instructions;
  there is no equivalent of writ's `blocking` boolean that gates whether an
  unfixed finding stops a handoff.
- **No structured diff-time audit prompt.** Cursor injects rules into the
  system prompt. It does not select rules against the current diff, cap them at
  `max_rules`/`max_chars`, and ask the agent to return findings.
- **No findings ingest / handoff gate.** writ's `--ingest` reads reported
  violations back and can block on open blocking findings. Cursor has no
  equivalent terminal-based finding ingestion flow.
- **No evidence-preserving archive.** Deleting a rule removes it; there is no
  documented archival state that keeps the rule as evidence while stopping
  selection.
- **No host-neutral neutrality.** Cursor rules are Cursor-only. writ is
  deliberately host-neutral (Claude Code, Codex, Cursor, OpenCode).
- **Removed automatic memory.** The Memories feature that might have competed
  with writ's "record once, reuse everywhere" model was removed without a
  public reason, and its data was only partially recoverable.
- **Vendor-controlled data.** Project Rules are local files, but User Rules,
  Team Rules, indexing data, and Cloud Agent memory live on or pass through
  Cursor's servers. writ is local-only.

## writ gaps vs Cursor

Cursor is stronger where writ deliberately does not play:

- **Integrated IDE/agent experience.** Cursor is the editor, the agent, and the
  context system in one product. writ requires a separate host agent and CLI.
- **AI-native workflows.** Cursor has built-in plan mode, inline edits,
  background/cloud agents, bug review, and automation triggers. writ does not
  write or execute code.
- **Team-wide enforcement at scale.** Team Rules can be enforced across an
  organization from a dashboard, with SSO and audit logging on Enterprise. writ
  has no hosted admin layer.
- **Codebase-aware retrieval.** Cursor indexes the entire codebase and
  retrieves relevant files automatically. writ selects only from its rule
  ledger; it does not understand the codebase beyond rule scopes.
- **MCP ecosystem.** Cursor can plug in many MCP servers, including third-party
  memory systems. writ's integration surface is intentionally narrow.
- **Cloud agent automations with memory.** Cursor automations can persist notes
  across runs and act on schedules/events. writ has no automation runtime.

## Sources

1. Cursor rules docs — https://cursor.com/docs/rules
2. Cursor rules help — https://cursor.com/help/customization/rules
3. Cursor 1.0 changelog (Memories beta) — https://cursor.com/changelog/1-0
4. Cursor 1.2 changelog (Memories GA with approvals) — https://cursor.com/changelog/1-2
5. Cursor 2.1 changelog (no Memories mention) — https://cursor.com/changelog/2-1
6. Cursor forum: Memories removed in 2.1.x — https://forum.cursor.com/t/memories-not-showing/143820
7. Cursor forum: Saved Memories viability — https://forum.cursor.com/t/saved-memories-in-cursor-and-their-viability/128968
8. Cursor hooks docs — https://cursor.com/docs/hooks
9. Cursor automations docs — https://cursor.com/docs/cloud-agent/automations
10. past.dev: How Cursor Memory Works — https://www.past.dev/blog/cursor-memory
11. Archcore: Cursor Removed Memories — https://archcore.ai/blog/cursor-memories-removed/
12. Cursor data use / privacy — https://cursor.com/data-use

Reviewed 2026-09-07.

## Suggested matrix values for synthesis

| Dimension | Suggested value for cursor-memories-rules |
|---|---|
| Local-first / data leaves machine | Project Rules are local files in git; User/Team Rules and indexing data pass through Cursor's cloud. Not local-first by design. |
| Durable rules with rationale | Free-form rules/prompts; no required separate rationale field. |
| Human approve before active | No `proposed → active` inbox; saved Project Rules are active immediately. Removed Memories feature required approval for background memories. |
| Scope model | `globs` in `.mdc` frontmatter + always/intelligent/manual activation modes; Team Rules add glob patterns. No language- or repo-identity scopes. |
| Diff-time selection / audit | Rules are injected into system prompt based on context; no diff-time selection that asks for findings back. |
| Prune / archive | Delete file or dashboard rule; no documented archival equivalent to `writ archive`. |
| UI for collection | Customize → Rules UI for managing rules; no dedicated "collection health" view. |
| Terminal findings | No structured terminal findings/ingest flow; hooks can observe/block but do not return rule violations. |
| Multi-agent host neutrality | Cursor-only; other agents need their own instructions or MCP memory servers. |
| Open source | No; Cursor is a proprietary product (built on VS Code, which is open source). |
| Pricing posture | Freemium IDE/agent with paid Pro/Teams/Enterprise tiers; core memory/steering is part of the product, not a separate purchase. |
