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
