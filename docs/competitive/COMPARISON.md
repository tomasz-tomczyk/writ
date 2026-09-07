# Competitive comparison matrix

Objective, per-tool notes live in `vs-<slug>.md`. Competitor cells are left as
**TBD** until they are filled from their own documentation or primary sources.

| Dimension | writ | mem0 | cursor-memories-rules | claude-code-memory | aider | continue-dev | windsurf | zep-letta | langmem |
|---|---|---|---|---|---|---|---|---|---|
| Local-first / data leaves machine | Local SQLite database under XDG paths; no server, no account, no telemetry. Data stays on the machine. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Durable rules with rationale | Each learning is a `title` + `rule` + required `rationale`; the rationale is how a rule transfers to cases its author did not foresee. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Human approve before active | Writes default to `proposed`; explicit activation is required before a rule is selected for audit. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Scope model | `global`, `project:<id>`, `language:<lang>`, `glob:<pattern>`; scopes AND across kinds and OR within a kind. `global` cannot be combined with another kind. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Diff-time selection / audit | `writ audit` selects active learnings that match the diff, ranks blocking rules first then by acceptance rate and recency, caps at `max_rules` and `max_chars`, and emits a prompt that asks for findings back. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Prune / archive | `writ archive` retires a rule; archived rows remain in the database as evidence and stop being selected. `writ list` offers reach/usefulness filters for pruning. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| UI for collection | `writ ui` provides Inbox, Collection, Detail, and Health screens for browsing, approving, editing, and pruning the collection. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Terminal findings | Audit findings are emitted and read in the terminal/CLI; the UI owns collection management, not the audit flow. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Multi-agent host neutrality | MCP server plus `writ install` for Claude Code, Codex, Cursor, and OpenCode; host-specific gate protocols emitted by `--hook`. writ never calls a model itself. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Open source | Yes; source available under the project license. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Pricing posture | Free, self-hosted local tool; no paid tiers or accounts planned for the core tool. | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

## Suggested matrix values from per-tool research

The following are proposed values for the `mem0` column, pending synthesis.
They should be reconciled with the full matrix format before the matrix is
marked final.

| Dimension | Suggested value for mem0 |
|---|---|
| Local-first / data leaves machine | Library and OpenMemory MCP can run locally; self-hosted keeps data on own infrastructure; managed cloud stores data with mem0. Not local-only by design. |
| Durable rules with rationale | Stores LLM-extracted facts/preferences, not human-authored rules with required rationale. |
| Human approve before active | No explicit `proposed → active` approval gate; memories enter retrieval once added. |
| Scope model | Multi-tenant scoping by `user_id`, `run_id`, `agent_id`, `org_id`; not rule-level applicability scopes like project/language/glob. |
| Diff-time selection / audit | Retrieval by semantic + BM25 + entity similarity to the current query; not a diff-time rule selection or review prompt. |
| Prune / archive | ADD-only extraction with memory-decay soft re-ranking; documented archival equivalent to `writ archive` not confirmed. |
| UI for collection | Dashboard for browsing and managing memories; local dashboard for OpenMemory MCP. |
| Terminal findings | Not a terminal findings tool; it is a retrieval layer feeding the agent context. |
| Multi-agent host neutrality | Broad SDK/framework integrations plus MCP server for Claude Code, Cursor, Codex, Windsurf, OpenCode. |
| Open source | Yes; Apache 2.0 open-source library/server plus optional managed cloud. |
| Pricing posture | Freemium managed cloud (free Hobby → $249+/mo Pro/Enterprise) plus free self-hosted open source. |

## Suggested matrix values from per-tool research (langmem)

The following are proposed values for the `langmem` column, pending synthesis.
They should be reconciled with the full matrix format before the matrix is
marked final.

| Dimension | Suggested value for langmem |
|---|---|
| Local-first / data leaves machine | Core SDK runs in-process; storage can be local `InMemoryStore` or self-hosted Postgres/Redis/MongoDB. Not local-only by design; managed service interest form exists. |
| Durable rules with rationale | Stores extracted facts, profiles, episodes, and prompt rules; no required human-authored rationale per memory. |
| Human approve before active | No explicit `proposed → active` approval gate; memories enter retrieval once written to the store. |
| Scope model | Hierarchical namespaces (`namespace` + `key`) plus metadata filters; not rule-level applicability scopes like project/language/glob. |
| Diff-time selection / audit | Retrieval by semantic similarity / metadata filtering to the current query; not a diff-time rule selection or review prompt. |
| Prune / archive | Update/delete/consolidate operations are configurable; evidence-preserving archive equivalent to `writ archive` not confirmed. |
| UI for collection | No dedicated curation UI in the open-source SDK; LangGraph Platform provides deployment tooling. |
| Terminal findings | Not a terminal findings tool; it is a retrieval and prompt-optimization layer. |
| Multi-agent host neutrality | LangGraph-native; core API is storage-agnostic but documented examples are LangGraph/LangChain. Python SDK only in the main repo. |
| Open source | Yes; MIT license. |
| Pricing posture | Free open-source SDK; managed service is an interest form, not a launched paid tier. |
