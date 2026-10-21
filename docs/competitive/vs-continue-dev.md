# Continue.dev — competitive brief

Objective notes on Continue.dev, an open-source, model-agnostic AI coding
assistant, focused on its rules, context providers, and agent mode, compared
against writ, a local-first ledger of steering/corrections for coding agents.

Sources cited inline; unknowns are marked explicitly.

## What it is

Continue.dev is an open-source coding assistant that provides chat,
autocomplete, inline edit, and agent mode inside VS Code, JetBrains IDEs, and a
terminal CLI. It is bring-your-own-model: a user can connect it to hosted APIs
(OpenAI, Anthropic, Mistral, etc.) or to fully local models via Ollama, LM
Studio, or llama.cpp. The project is Apache 2.0 licensed.

Important current status: Continue was acquired by Cursor (Anysphere) in
mid-June 2026. The team shipped a final 2.0.0 release for the VS Code
extension, CLI, and JetBrains plugin, then made the `continuedev/continue`
GitHub repository read-only. Continue Hub cloud data is deleted after July 15,
2026, and recurring billing has been disabled.

This brief documents the tool as shipped in its final release, not as a future
product.

Sources:
[Continue GitHub repository](https://github.com/continuedev/continue),
[Continue acquisition notice](https://continue.dev/),
[Continue config reference](https://docs.continue.dev/reference),
[Continue rules docs](https://docs.continue.dev/customize/deep-dives/rules),
[Continue context providers docs](https://docs.continue.dev/customize/deep-dives/custom-providers).

## Goals

- Give developers an open, configurable AI coding assistant inside familiar
  editors, with control over which models are used for chat, autocomplete,
  edit, and agent tasks.
- Let teams standardize models, rules, prompts, context sources, and MCP tools
  through shared configuration blocks.
- Provide project-specific and global steering through `.continue/rules/` and
  `config.yaml` so agent behavior can be tuned per workspace.
- Support both cloud and local inference, including zero-cloud setups with
  local models.

## Memory / steering model

Continue's steering layer is file-based and lives under `.continue/`:

| Mechanism | Authored by | What it carries | How it loads |
|---|---|---|---|
| `.continue/config.yaml` | Human | Models, context providers, rules, prompts, MCP servers, docs | Loaded at session start; global `~/.continue/config.yaml` merges with workspace `.continue/config.yaml` |
| `.continue/rules/*.md` | Human (or agent via tool) | Natural-language instructions with optional `globs`, `regex`, `description`, `alwaysApply` | Auto-detected and concatenated into the system message in lexicographical order |
| `.continue/prompts/*.md` | Human | Slash-command prompt templates | Invoked by `/` commands |
| Context providers | Human-configured | File, diff, terminal, repo-map, HTTP, MCP, etc. | User `@`-mentions or pre-configured `context:` list |
| Continue Hub blocks | Team/organization | Shared models, rules, prompts, MCP servers, docs | Pulled from Hub (sunsetting; cloud data deleted after 2026-07-15) |

**Rules** are markdown files with YAML frontmatter. They support:

- `name`: display title.
- `globs`: glob patterns that decide whether the rule is included when matching
  files are in context.
- `regex`: regex patterns evaluated against file content.
- `description`: a description the agent may use to decide whether to pull the
  rule into context when `alwaysApply` is `false`.
- `alwaysApply`: `true` (always include), `false` (include only when globs match
  or the agent chooses it from the description), or undefined (include if no
  globs exist or globs match).

Rules are concatenated into the system message for Agent, Chat, and Edit
requests. They are not sent to autocomplete. Files are loaded in
lexicographical order, so numbering prefixes such as `01-general.md`,
`02-frontend.md` are a common convention.

**Configuration composition.** Continue uses a YAML block/composition model.
`config.yaml` can reference shared blocks with `uses:` (e.g.,
`uses: sanity/sanity-opinionated` from Hub, or `file://...` for local files),
which are unrolled at load time. Global `~/.continue/` and workspace
`.continue/` configurations merge, with workspace values overriding global
ones. `config.json` is deprecated.

**Agent files.** Continue converts workspace-root agent files such as
`AGENT.md` or `CLAUDE.md` into rules, so tool-agnostic instruction files can
feed into Continue without duplication.

There is no separate structured "learning" entity with a required rationale
field or status lifecycle. A rule is active as soon as its file exists in
`.continue/rules/` or is referenced by `config.yaml`.

Sources:
[Continue rules deep dive](https://docs.continue.dev/customize/deep-dives/rules),
[Continue config reference](https://docs.continue.dev/reference),
[Continue context providers docs](https://docs.continue.dev/customize/deep-dives/custom-providers),
[Continue codebase awareness guide](https://docs.continue.dev/guides/codebase-documentation-awareness),
[DeepWiki YAML blocks and composition](https://deepwiki.com/continuedev/continue/5.2-yaml-blocks-and-composition).

## Local vs cloud

Continue is designed to be model-agnostic and can run entirely locally, but it
is not local-only by design:

- **Core extension/CLI** is open source and runs locally. Configuration is
  stored in `~/.continue/` and workspace `.continue/` directories.
- **Models** can be local (Ollama, LM Studio, llama.cpp) or cloud-hosted. The
  user brings their own API keys; Continue does not require a vendor account to
  use the open-source core.
- **Telemetry and authentication** were removed in the final 2.0.0 release
  according to the repository README.
- **Continue Hub** was the commercial team layer for sharing blocks, assistants,
  and governance. It is now sunsetting; cloud data is deleted after July 15,
  2026, and recurring billing has been disabled.

writ, by contrast, stores rules in a local SQLite database under XDG paths, has
no account, no server, and calls no model.

Sources:
[Continue GitHub repository](https://github.com/continuedev/continue),
[Continue acquisition notice](https://continue.dev/),
[Continue Review 2026](https://aiagentsquare.com/agents/continue-dev),
[Bodega One: Cursor acquired Continue.dev](https://www.bodegaone.ai/blog/cursor-acquires-continue-dev).

## Agent integration

Continue is itself the host agent inside the IDE or CLI. It does not rely on a
separate agent to reason over a rule ledger:

- **Agent mode** combines a configured model, rules, and MCP tools to plan and
  execute edits.
- **Chat and Edit modes** also load rules into the system message; rules do not
  apply to autocomplete.
- **Context providers** are invoked via `@` mentions or pre-configured in
  `config.yaml`. Built-in providers include file, code, git diff, current file,
  terminal, open files, clipboard, tree, problems, debugger, repo map,
  operating system, HTTP, and MCP.
- **MCP servers** are configured under `mcpServers:` and exposed as context/tool
  sources.
- **Plan mode** lets the agent propose a plan before editing.

writ integrates with Continue only indirectly: writ can feed rules into any
host agent (including one the user runs alongside Continue), but Continue
itself is not a writ host. There is no documented `writ audit --hook continue`
integration.

Sources:
[Continue agent quick start](https://docs.continue.dev/ide-extensions/agent/quick-start),
[Continue plan mode guide](https://docs.continue.dev/guides/plan-mode-guide),
[Continue MCP docs](https://docs.continue.dev/customize/deep-dives/mcp),
[Continue config reference](https://docs.continue.dev/reference).

## Curation / prune / audit

**Curation.** Rules are edited directly as markdown files in `.continue/rules/`
or as entries in `config.yaml`. The agent can create a rule via the
`create_rule_block` tool if enabled. There is no separate inbox, approval
state, or `proposed → active` lifecycle; a rule file is active as soon as it is
present.

**Prune.** Rules are retired by deleting the file or removing the `uses:`
reference. **Marked as unknown:** whether Continue retains deleted rules as
evidence.

**Audit.** Continue does not run a diff-time rule selection that asks the agent
to report findings back. Rules are concatenated into the system message and
relied upon to shape behavior. There is no per-rule `times_selected` /
`times_applied` counter, no findings ingest flow, and no blocking/advisory
distinction. Model-driven review commands exist, but they are not a structured
audit against a durable, scoped rule ledger.

## Overlap with writ

Both tools address the problem of making coding agents more consistent by
giving them durable, reusable guidance:

- Both support project-scoped rules (Continue via `.continue/rules/`, writ via
  `project:` scopes).
- Both support file-pattern scoping (Continue via `globs:` frontmatter and
  `regex:`, writ via `glob:` scopes).
- Both can be shared with a team, though Continue Hub is sunsetting and writ
  uses `writ export` / JSONL exchange.
- Both allow local-only operation if the user chooses local models and avoids
  cloud services.
- Both treat written conventions as a source of truth the agent should consult.

## Gaps vs writ

For writ's specific use case — durable, human-curated steering/corrections
that gate code handoff — Continue has notable gaps:

- **No required rationale per rule.** Continue rules are free-form markdown
  with optional description; they do not require a separate `rationale` field.
- **No explicit proposed → active gate.** A rule file is active immediately.
  There is no inbox for reviewing candidate rules before they enter context.
- **No blocking vs advisory distinction.** Rules are soft system-message
  instructions; there is no equivalent of writ's `blocking` boolean that gates
  whether an unfixed finding stops a handoff.
- **No structured diff-time audit prompt.** Continue concatenates rules into the
  system message. It does not select rules against the current diff, cap them
  at `max_rules`/`max_chars`, and ask the agent to return findings.
- **No findings ingest / handoff gate.** writ's `--ingest` reads reported
  violations back and can block on open blocking findings. Continue has no
  equivalent terminal-based finding ingestion flow.
- **No evidence-preserving archive.** Deleting a rule removes it; there is no
  documented archival state that keeps the rule as evidence while stopping
  selection.
- **No host-neutral neutrality.** Continue's instruction layer is
  Continue-specific. writ is deliberately host-neutral (Claude Code, Codex,
  Cursor, OpenCode).
- **Steering is secondary to editing.** Continue is primarily a code editor/
  agent, not a curated steering ledger. Rules exist to improve the agent's
  output, not to provide a cross-host, approval-gated rule system.
- **End-of-life status.** The project is no longer actively maintained, the
  repository is read-only, and the team layer is shutting down. Long-term
  stewardship, security fixes, and provider-API updates are unavailable.

## writ gaps vs Continue

Continue was stronger where writ deliberately does not play:

- **Integrated IDE assistant.** Continue provided chat, autocomplete, inline
  edit, and agent mode inside VS Code and JetBrains. writ is a CLI/UI steering
  layer, not an editor extension.
- **Broad model provider support.** Continue connected to many hosted and local
  providers with role assignment per model. writ never calls a model.
- **Rich context providers.** Continue's `@` context providers (file, diff,
  terminal, repo-map, HTTP, MCP, etc.) retrieve codebase and runtime context.
  writ selects only from its rule ledger.
- **Autonomous editing.** Continue's agent mode planned and executed file
  edits. writ does not write or execute code.
- **Team sharing via Hub (historical).** Continue Hub allowed organizations to
  publish shared blocks and assistants. writ has no hosted admin layer or
  registry.
- **First-class local inference.** Continue supported local models out of the
  box, giving users a zero-cloud option. writ is local-first by storage design
  but does not manage model inference.

## Sources

1. Continue GitHub repository — https://github.com/continuedev/continue
2. Continue.dev homepage / acquisition notice — https://continue.dev/
3. Continue config.yaml reference — https://docs.continue.dev/reference
4. Continue rules deep dive — https://docs.continue.dev/customize/deep-dives/rules
5. Continue context providers — https://docs.continue.dev/customize/deep-dives/custom-providers
6. Continue codebase and documentation awareness — https://docs.continue.dev/guides/codebase-documentation-awareness
7. Continue configuring models, rules, and tools — https://docs.continue.dev/guides/configuring-models-rules-tools
8. Continue agent quick start — https://docs.continue.dev/ide-extensions/agent/quick-start
9. Continue plan mode guide — https://docs.continue.dev/guides/plan-mode-guide
10. Continue MCP deep dive — https://docs.continue.dev/customize/deep-dives/mcp
11. Continue deprecated context providers — https://docs.continue.dev/reference/deprecated-context-providers
12. DeepWiki: YAML blocks and composition — https://deepwiki.com/continuedev/continue/5.2-yaml-blocks-and-composition
13. AI Agent Square: Continue Review 2026 — https://aiagentsquare.com/agents/continue-dev
14. Bodega One: Cursor acquired Continue.dev — https://www.bodegaone.ai/blog/cursor-acquires-continue-dev
15. Stridenote: Continue.dev and Ollama local setup — https://stridenote.net/continue-dev-local-autocomplete-vscode/

Reviewed 2026-09-07.

## Suggested matrix values for synthesis

| Dimension | Suggested value for continue-dev |
|---|---|
| Local-first / data leaves machine | Open-source core runs locally; BYO keys or local models (Ollama, LM Studio) keep inference on machine. Continue Hub was cloud-hosted and is sunsetting (data deleted after 2026-07-15). Not local-only by design. |
| Durable rules with rationale | Human-authored rules in `.continue/rules/*.md` with optional description; no required separate `rationale` field. Rules are active immediately. |
| Human approve before active | No `proposed → active` inbox; a rule file is active as soon as it exists. Agent can auto-create rules via `create_rule_block` tool. |
| Scope model | `globs:` and `regex:` frontmatter plus `alwaysApply` behavior; hierarchical global/workspace config merge. No repo-identity or language scopes. |
| Diff-time selection / audit | Rules are concatenated into the system message; no diff-time selection that asks for findings back. |
| Prune / archive | Delete or edit rule files; no documented archival equivalent to `writ archive`. |
| UI for collection | File editor and IDE toolbar; no dedicated "collection health" UI. Continue Hub had a sharing UI, now sunsetting. |
| Terminal findings | No structured terminal findings/ingest flow. |
| Multi-agent host neutrality | Continue-specific; rules feed Continue's own agent, not other hosts. |
| Open source | Yes; Apache 2.0, though the upstream repository is now read-only and no longer maintained. |
| Pricing posture | Core IDE extension/CLI free and open source; former Continue Hub paid tiers are no longer purchasable. User pays own model-provider costs. |
