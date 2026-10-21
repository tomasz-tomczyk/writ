# Windsurf / Cascade Memories & Rules — competitive brief

Objective notes on Windsurf (formerly Codeium, now part of Cognition AI / Devin)
and its Cascade memory and rule features, compared against writ, a local-first
ledger of steering/corrections for coding agents.

Sources cited inline; unknowns are marked explicitly.

## What it is

Windsurf is an AI-native IDE built around Cascade, an agentic coding assistant
that plans, edits across files, and calls tools. Its persistent context layer has
two documented mechanisms:

- **Memories** — short, auto-generated notes Cascade creates during
  conversations, plus manually created memories via prompts like "create a
  memory of ...". They are workspace-scoped, stored locally, and retrieved when
  Cascade judges them relevant.
- **Rules** — human-authored markdown instruction files, optionally with YAML
  frontmatter that controls when and how they load. Rules live at global,
  workspace, and system/enterprise levels, and can also be inferred from
  `AGENTS.md` files.

As of 2026, Windsurf is shipping under Cognition AI as part of the Devin
ecosystem. The documentation now lives at `docs.devin.ai`, with `docs.windsurf.com`
redirecting there. The product still uses the `.codeium/` local data directory
and supports the legacy `.windsurf/rules/` path alongside the preferred
`.devin/rules/` path.

This brief focuses on the IDE-level Memories/Rules product, not Devin Cloud
agents or the broader Devin platform.

Sources:
[Windsurf memories docs](https://docs.windsurf.com/windsurf/cascade/memories),
[Windsurf AGENTS.md docs](https://docs.windsurf.com/windsurf/cascade/agents-md),
[Devin pricing](https://windsurf.com/pricing),
[Codeium → Windsurf rebrand coverage](https://zynovix.github.io/windsurf-formerly-codeium-review-2026.html).

## Goals

- Give Cascade persistent context so the user does not have to repeat stack
  choices, conventions, or workflows every session.
- Let teams share durable standards through version-controlled workspace Rules
  and `AGENTS.md` files.
- Provide global and workspace-scoped rules with explicit activation modes so
  context is loaded only when needed.
- Capture one-off, personal observations automatically via Memories without
  requiring the user to author a formal rule.
- Let enterprises enforce baseline policies through system-level rules
  deployed by IT.

## Memory / steering model

**Rules are authored, static prompts.** A workspace rule is a markdown file with
YAML frontmatter that controls when it loads:

| Mode | `trigger:` value | How it reaches Cascade |
|---|---|---|
| Always On | `always_on` | Full content in the system prompt on every message |
| Model Decision | `model_decision` | Only the `description` is always loaded; full file is read when Cascade judges it relevant |
| Glob | `glob` | Loaded when Cascade reads or edits a file matching `globs` |
| Manual | `manual` | Loaded only when `@rule-name` is typed in the Cascade input |

Rules can be stored at multiple scopes:

| Scope | Location | Notes |
|---|---|---|
| Global | `~/.codeium/windsurf/memories/global_rules.md` | Single file, always on, 6,000-character limit |
| Workspace | `.devin/rules/*.md` (preferred) or `.windsurf/rules/*.md` (fallback) | One file per rule, 12,000-character limit per file |
| `AGENTS.md` | Any project directory | Root-level = always on; subdirectory = auto-glob for that directory |
| System / Enterprise | OS-specific (e.g., `/etc/devin/rules/`) | Deployed by IT, read-only for end users |

Rules discovery walks the current workspace, subdirectories, and parent
directories up to the git root. The legacy `.windsurfrules` single file at the
workspace root is also still read.

**Memories are extracted, not authored.** Cascade can propose memories
automatically during a conversation, and users can create them explicitly with
"create a memory of ..." or "remember that ...". Memories are stored locally
under `~/.codeium/windsurf/memories/`, organized by workspace. They are not
committed to the repository, are not shared with teammates, and do not consume
Cascade credits. Cascade retrieves memories when it believes they are relevant.

The docs explicitly recommend Rules or `AGENTS.md` for durable, team-shareable
knowledge and Memories for personal, one-off, or ephemeral context.

**Important current limitation:** Memories apply to the legacy Cascade agent
only. The newer Devin Local agent, which is the default for new tabs, does not
persist memories. The official migration path is to convert relied-upon
memories to Skills via the Cascade Migration Wizard.

In all cases, Windsurf / Cascade itself is the reasoning engine; rules and
memories are context, not a separate audit layer.

Sources:
[Windsurf memories docs](https://docs.windsurf.com/windsurf/cascade/memories),
[Windsurf AGENTS.md docs](https://docs.windsurf.com/windsurf/cascade/agents-md),
[Windsurf memories guide 2026](https://baeseokjae.github.io/posts/windsurf-memories-guide-2026/).

## Local vs cloud

Windsurf is a proprietary, cloud-connected IDE/agent, not local-first software:

- **Workspace Rules and `AGENTS.md`** live as files in the repo (local disk) and
  travel with git.
- **Global rules** in `~/.codeium/windsurf/memories/global_rules.md` stay on one
  machine unless synced manually.
- **Memories** are stored locally under `~/.codeium/windsurf/memories/` and are
  not uploaded to the repo, but they are tied to the workspace and machine.
- **Cascade inference** sends file contents and surrounding context to Codeium /
  Cognition servers for model processing. Local indexing helps select context,
  but the inference itself is cloud-based.
- **Zero Data Retention (ZDR)** is available and is the default for Teams /
  Enterprise; on individual plans it is opt-in. Without ZDR, code snippets and
  trajectories may be logged and may be used for training. **Marked as unknown:**
  the exact retention details for non-ZDR individual accounts.
- **Cloud agents / Devin Cloud** run in Cognition-managed infrastructure.
- **System / Enterprise rules** are pushed through enterprise deployment tools.

writ, by contrast, stores rules in a local SQLite database under XDG paths, has
no account, no server, calls no model, and sends no code context anywhere.

Sources:
[Windsurf memories docs](https://docs.windsurf.com/windsurf/cascade/memories),
[Devin pricing](https://windsurf.com/pricing),
[Windsurf privacy overview](https://www.lowcode.agency/blog/windsurf-privacy-security),
[Windsurf ZDR summary](https://ptkd.com/journal/does-windsurf-keep-my-code-or-prompt-data),
[Cognition privacy policy](https://cognition.com/pages/privacy-policy).

## Agent integration

Windsurf / Cascade is itself the host agent; rules and memories are intrinsic
context for Cascade:

- **Cascade** — loads global rules, workspace rules, `AGENTS.md`, and relevant
  memories automatically based on frontmatter / location.
- **Customizations panel** — UI for managing memories and rules.
- **MCP servers** — Windsurf / Devin Desktop can connect to external MCP
  servers, including third-party memory tools.
- **Devin Local agent** — uses the same Rules / `AGENTS.md` system but does not
  persist Memories.
- **Devin Cloud agents** — run in the cloud and can load project hooks and
  context.

writ integrates with Windsurf / Devin Desktop via MCP and `writ install`, and
emits a host-specific `--hook` stop message where the host protocol supports it.
writ is an external steering ledger, not part of Windsurf.

Sources:
[Windsurf memories docs](https://docs.windsurf.com/windsurf/cascade/memories),
[Devin Desktop docs](https://docs.devin.ai/desktop).

## Curation / prune / audit

**Curation.** Rules are edited as `.md` files or through the Customizations
panel. There is no separate "inbox" or approval state; a saved workspace rule is
active immediately (unless it is a system/enterprise rule managed by IT).
Memories can be edited by clicking into them in the Customizations panel or by
editing the files under `~/.codeium/windsurf/memories/`.

**Prune.** There is no documented archive/retire operation equivalent to
`writ archive`. Rules are deleted by deleting the file; memories are deleted by
removing the file or using the UI. **Marked as unknown:** whether deleted
Windsurf data is retained as evidence.

**Audit.** Windsurf does not run a diff-time rule selection that asks the agent
to report findings back. It injects rules into context and relies on the model
to follow them. Cascade can review code as part of its agentic loop, but that is
a model-driven review, not a structured audit against a durable rule ledger with
"selected vs applied" metrics.

## Overlap with writ

Both tools address the same underlying problem: making coding agents more
consistent by giving them durable, reusable guidance.

- Both persist instructions across sessions.
- Both support project-scoped rules (Windsurf via workspace `.devin/rules/` or
  `.windsurf/rules/`, writ via `project:` scopes).
- Both support file-pattern scoping (Windsurf via `glob` trigger / `AGENTS.md`
  subdirectory, writ via `glob:`).
- Both can be shared with a team, though Windsurf's workspace rules and
  `AGENTS.md` travel with the repo while writ uses `writ export` / JSONL
  exchange.
- Both treat the IDE/agent as the primary enforcement surface.

## Gaps vs writ

For writ's specific use case — durable, human-curated steering/corrections
that gate code handoff — Windsurf has notable gaps:

- **No required rationale per rule.** Windsurf rules are free-form prompts;
  they do not require a separate `rationale` field that explains why the rule
  applies.
- **No explicit proposed → active gate.** A saved workspace rule is active
  immediately. There is no inbox for reviewing candidate rules before they
  enter context.
- **No blocking vs advisory distinction.** Windsurf rules are soft instructions;
  there is no equivalent of writ's `blocking` boolean that gates whether an
  unfixed finding stops a handoff.
- **No structured diff-time audit prompt.** Windsurf injects rules into the
  system prompt. It does not select rules against the current diff, cap them at
  `max_rules`/`max_chars`, and ask the agent to return findings.
- **No findings ingest / handoff gate.** writ's `--ingest` reads reported
  violations back and can block on open blocking findings. Windsurf has no
  equivalent terminal-based finding ingestion flow.
- **No evidence-preserving archive.** Deleting a rule or memory removes it;
  there is no documented archival state that keeps the rule as evidence while
  stopping selection.
- **No host-neutral neutrality.** Windsurf rules and memories are
  Windsurf/Cascade-only. writ is deliberately host-neutral (Claude Code, Codex,
  Cursor, OpenCode).
- **Memories are personal and machine-bound.** Even for the same workspace,
  teammates do not share memories, and memories may not transfer across
  machines or dev containers.
- **Memories are being deprecated for the default agent.** The Devin Local
  agent does not persist memories, so the auto-memory mechanism is confined to
  the legacy Cascade agent.
- **Vendor-controlled data.** Workspace Rules and `AGENTS.md` are local files,
  but inference, global rules sync, system/enterprise rules, Devin Cloud, and
  account data pass through Cognition/Codeium servers. writ is local-only.

## writ gaps vs Windsurf

Windsurf is stronger where writ deliberately does not play:

- **Integrated IDE/agent experience.** Windsurf is the editor, the agent, and
  the context system in one product. writ requires a separate host agent and
  CLI.
- **AI-native workflows.** Windsurf has built-in multi-file edits, plan mode,
  inline edits, cloud agents, and automation triggers. writ does not write or
  execute code.
- **Automatic memory extraction.** Cascade can create memories from
  conversations without manual authoring. writ has no transcript scanner or
  automatic rule extraction.
- **Team-wide enforcement at scale.** System/enterprise rules can be enforced
  across an organization through IT-managed deployment. writ has no hosted
  admin layer.
- **Codebase-aware retrieval.** Cascade indexes the codebase and retrieves
  relevant files automatically. writ selects only from its rule ledger; it does
  not understand the codebase beyond rule scopes.
- **MCP ecosystem.** Windsurf can plug in many MCP servers, including
  third-party memory systems. writ's integration surface is intentionally
  narrow.
- **Pricing / support tiers.** Windsurf is a commercial product with Free,
  Pro, Max, Teams, and Enterprise plans. writ is free and self-hosted.

## Pricing posture

As of September 2026, Windsurf / Devin pricing includes:

| Plan | Price | Notes |
|---|---|---|
| Free | $0 | Light agent quota, limited model availability, unlimited inline edits and Tab completions |
| Pro | $20/user/mo | Higher quotas, frontier models, Devin Cloud access |
| Max | $200/mo | Significantly higher quotas for heavy individual use |
| Teams | $80/mo base + $40/mo per full dev seat | Centralized billing, admin dashboard, priority support |
| Enterprise | Custom | SSO, SCIM, RBAC, VPC deployment, dedicated account support |

Core memory and rule functionality is included across tiers; the paid tiers add
quotas, models, team administration, and deployment options.

Sources:
[Devin pricing](https://windsurf.com/pricing),
[Codeium pricing 2026](https://comparedge.com/tools/codeium/pricing),
[Windsurf pricing review](https://aisotools.com/windsurf-pricing).

## Sources

1. Windsurf Memories & Rules docs — https://docs.windsurf.com/windsurf/cascade/memories
2. Windsurf AGENTS.md docs — https://docs.windsurf.com/windsurf/cascade/agents-md
3. Devin Desktop docs — https://docs.devin.ai/desktop
4. Devin / Windsurf pricing — https://windsurf.com/pricing
5. Windsurf Memories guide 2026 — https://baeseokjae.github.io/posts/windsurf-memories-guide-2026/
6. Codeium (Windsurf) pricing comparison — https://comparedge.com/tools/codeium/pricing
7. Windsurf pricing breakdown — https://aisotools.com/windsurf-pricing
8. Windsurf review 2026 (Codeium rebrand, Devin integration) — https://zynovix.github.io/windsurf-formerly-codeium-review-2026.html
9. Windsurf privacy & security guide — https://www.lowcode.agency/blog/windsurf-privacy-security
10. Does Windsurf keep my code or prompt data? — https://ptkd.com/journal/does-windsurf-keep-my-code-or-prompt-data
11. Cognition privacy policy — https://cognition.com/pages/privacy-policy

Reviewed 2026-09-07.

## Suggested matrix values for synthesis

| Dimension | Suggested value for windsurf |
|---|---|
| Local-first / data leaves machine | Memories are local files; workspace Rules and `AGENTS.md` are in git. Cascade inference and account/cloud features pass through Cognition/Codeium servers. Not local-first by design. |
| Durable rules with rationale | Human-authored rules in `.devin/rules/` / `.windsurf/rules/` / `AGENTS.md`; no required separate `rationale` field. Memories are short notes, not rules with rationale. |
| Human approve before active | No `proposed → active` inbox; saved workspace rules are active immediately. Memories can be auto-generated and are user-edited after creation. |
| Scope model | Global file + workspace rule files with `trigger` frontmatter (`always_on`, `model_decision`, `glob`, `manual`) + `AGENTS.md` directory scoping. No explicit repo-identity or language scopes. |
| Diff-time selection / audit | Rules are injected into system prompt based on triggers/context; no diff-time selection that asks for findings back. |
| Prune / archive | Delete rule/memory files or use UI; no documented archival equivalent to `writ archive`. |
| UI for collection | Customizations panel for managing Memories and Rules; no dedicated "collection health" view. |
| Terminal findings | No structured terminal findings/ingest flow; hooks are host-specific stop messages, not rule-violation ingestion. |
| Multi-agent host neutrality | Windsurf/Cascade-only; other agents need their own instructions or MCP memory servers. |
| Open source | No; Windsurf / Devin Desktop is a proprietary Cognition AI product. |
| Pricing posture | Freemium IDE/agent: Free, Pro ($20/mo), Max ($200/mo), Teams ($80 base + $40/seat), Enterprise custom; core memory/steering included across tiers. |
