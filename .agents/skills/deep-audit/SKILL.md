---
name: deep-audit
description: "Perform a deep audit of the writ codebase with Rust, SQLite, CLI, MCP, and host-integration discovery followed by independent validation and PR-sized grouping. Use for codebase audits, deep reviews, tech-debt discovery, or asking what needs fixing."
disable-model-invocation: true
---

# Deep codebase audit

The name is deliberately `deep-audit`. Never call this skill `writ-audit`: `writ audit` is a product command that selects learning-ledger rules for a diff. This skill audits the repository developers work on.

## Prepare

Read `AGENTS.md` and the complete local design spec. Stop if the spec is absent. Read `mise.toml`, the workspace manifests, CI and release workflows, README, plugin documentation, and the repository tree. Record the current revision and dirty state. An audit is read-only unless the user later chooses fixes.

Map ownership before dispatching discovery:

- `writ-core`: domain model, store, migrations, selection, budget, glob and repository normalization; no I/O surface.
- `writ-cli`: terminal, git/process/filesystem boundaries, MCP, hooks, install, telemetry, UI and rendering.
- `plugins/`: Claude Code, Codex, Cursor, and OpenCode distribution artifacts.
- `scripts/`, `mise.toml`, `.github/`, Cargo and Nix files: quality and release machinery.

Tell every reviewer that automated formatting, Clippy warnings, ordinary test failures, typos, dependency policy, and documented-flag checks already have mechanical gates. The audit should focus on semantic defects, broken contracts, missing tests, data integrity, security/privacy boundaries, and worthwhile structural improvements.

## Discovery

When the host supports independent agents, dispatch fresh reviewers in parallel. Otherwise perform the same passes locally and keep their findings separate until validation.

### Rust and architecture

Read every Rust source and test. Focus on ownership boundaries, error propagation, state transitions, path/identity behavior, unnecessary public surface, duplicated logic with at least three meaningful callers, and behavior that exists without a regression test. Do not flag small-collection linear work merely for being linear; the spec explicitly bounds prompt size, not all selection cost.

### SQLite and data integrity

Read schema, migrations, store queries, JSONL import/export, FTS, and tests. Check foreign keys per connection, trigger-maintained timestamps, external content FTS integrity, bound values, migration compatibility, status defaults, scope semantics, counter timing, archive preservation, and exemplar location rules. Treat silent widening or lost evidence as high-risk.

### CLI and system boundaries

Review command parsing, git invocation, stdin modes, output formats, exit codes, config paths, UI, editor spawning, and telemetry. Look for hangs, partial reads, misclassified failures, argument injection, Unicode slicing, diff-parser state, HTML/CSRF issues, path re-splitting, privacy leaks, or config inconsistencies.

### MCP, hooks, and host integrations

Review CLI/MCP parity, tool schema size, prompt return paths, the two gate moments, retry signaling, host protocols, installer merges/backups/idempotence, plugin manifests, snippets, skills, and documentation. OpenCode's lack of an enforceable gate is an intentional difference, not missing parity.

### Tests, docs, build, and release

Check whether tests prove all exit codes and the three stdin cases, prompt goldens are meaningful, fixtures cannot satisfy cap assertions without running the implementation, documented flags are live, CI mirrors mise tasks, and Cargo/Nix/plugin/release versions cannot drift. Look for important workflows CI does not exercise.

Each raw finding must cite a current file and line, identify an observable harm, and propose a concrete fix. Re-read the line before reporting it.

## Group and validate

Merge duplicates and arrange raw findings into independently shippable, theme-based groups, ideally under about 100 changed lines each. Keep cross-domain changes together only when separating them would break a contract.

Give each group to a fresh skeptical validator with no discovery conclusions beyond the exact claims. Validators must read the actual code and answer:

1. Is this real at writ's local-first scale and execution model?
2. Can the claimed path actually execute?
3. Does the spec require the current behavior?
4. Is the cited line and history accurate?
5. Does an existing test or caller refute the claim?
6. Is the proposed fix the smallest correct one and worth its churn?

Verdicts are `REAL PROBLEM`, `MARGINAL`, or `NOT A REAL PROBLEM`. Only real problems enter the ready-to-implement list. Show marginal and rejected claims with short reasons so calibration remains visible.

## Results

Return:

- audited revision and surfaces;
- count of raw and validated findings;
- validated findings ordered by data-loss/security/spec risk;
- PR-sized groups with theme, files, dependencies, and estimated effort;
- rejected findings and why;
- missing or failed verification commands.

Ask whether the user wants all groups implemented, selected groups, or only the report. If implementation is approved, use separate `wt` worktrees based explicitly on `origin/main`, keep each group focused, run `writ-review`, and ship only when separately requested.

## Prevention and retrospective

Suggest an `AGENTS.md` or skill refinement only when a surviving finding or repeated false positive exposes a reusable rule. Never write prevention changes automatically. Close with a one-line smooth-run note or propose one precise canonical skill update for user approval.
