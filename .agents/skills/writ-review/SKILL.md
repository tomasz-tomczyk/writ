---
name: writ-review
description: "Review a writ repository diff before landing it, with emphasis on Rust, SQLite, CLI and MCP contracts, host hooks, and writ's design invariants. Use for code review, pre-landing review, or checking changes in this repository."
disable-model-invocation: true
---

# Pre-landing review for writ

This reviews work on the writ repository. It is a developer workflow, not the product command `writ audit`, and it must not write to a user's learning ledger.

## Establish the review contract

1. Read `AGENTS.md` completely.
2. Require `docs/superpowers/specs/2026-09-06-writ-design.md`. Stop and ask for it if absent. Read the relevant sections before judging behavior. The spec wins over code; if the spec itself appears wrong, report the contradiction instead of silently changing the design.
3. Determine the intended change from the user's request, branch commits, and PR title/body when available.
4. Fetch `origin/main` when network access is available. Review the branch-only three-dot diff against `origin/main`. Do not rebase, commit, or edit during a review unless the user separately asks for fixes.
5. Include tracked, staged, unstaged, and relevant untracked work. Identify unrelated dirty files and leave them alone.

Report the branch, base, commit count, changed files, intended outcome, and whether the review is quick, standard, or deep.

## Review phases

### 1. Intent check

Run the `intent-check` skill in PR mode. Confirm that every changed file and significant deletion is plausible for the stated intent. A validated silent revert, wrong-base hunk, or unrelated change is a blocker.

If independent agents are available, keep the intent auditor and its validator fresh: neither should inherit implementation-session conclusions. If they are not available, perform both passes locally and explicitly separate discovery from skeptical validation.

### 2. Architectural boundaries

Review the complete affected call paths, not only individual hunks.

- `writ-core` is pure domain logic. It has no printing, terminal, filesystem, process, socket, HTTP, or MCP surface. I/O belongs in `writ-cli`.
- The core knows nothing about capture sources. All sources converge on the same write behavior and the schema must not grow source-specific concepts.
- The core loop has no required external install. Optional tools degrade to a useful scope-only result instead of failing the audit.
- Plugins remain integrations around the binary, not alternate implementations of product behavior.
- One workspace and one `writ` binary remain the release unit.

### 3. Data and SQLite invariants

Trace migrations, model conversion, store methods, and tests together.

- Writes default to `proposed`; activation is explicit.
- Pruning archives and preserves evidence.
- Exemplars store snippet text, never a path or line. Findings alone may carry an audit-local location.
- Repository identity comes from a normalized remote. A worktree must resolve to the same project as its main checkout; the documented no-remote fallback must remain honest.
- Scope kinds AND with each other and values OR within a kind. `global` cannot be combined with another kind.
- `updated_at` is maintained by database triggers. Ordinary writes never set it explicitly; import is the deliberate exception.
- Foreign keys are enabled on every connection, FTS external-content triggers remain complete, and SQL values are bound rather than interpolated.
- Migration changes are forward-safe for existing databases and are tested from both an empty and an older store when applicable.

### 4. Audit and gate semantics

Treat these as separate moments:

- Hook entry sends the agent back when any learning is selected, advisory or blocking, because findings do not exist yet.
- Ingest blocks only when a blocking finding is open or ignored.

Also verify:

- `times_selected` and `last_selected_at` move on emit; `times_applied` and `last_applied_at` move on ingest. Never collapse or compare them as absolute counts; their ratio is the signal.
- Prompt output is capped by both rule count and characters. Do not claim that selection work is bounded by the diff or prompt cap.
- Prompt rendering tells the host how to return findings through MCP or CLI, includes blocking/advisory state, and preserves the byte-for-byte golden contract.
- Retry state comes from each host, not a counter writ invents.
- Hook-mode exit behavior follows the host protocol. The general CLI exit-code table still applies outside hook mode.
- Required stdin commands do not silently truncate; hook input never hangs.

### 5. CLI, MCP, UI, and host integrations

- Each failure cause has its own honest message and exit code. Check usage, malformed JSON, unknown IDs, non-git directories, empty diffs, and storage failures where relevant.
- New flags have parsing tests and end-to-end behavior tests. CLI and MCP paths reach the same core behavior, and tool schemas remain intentionally small.
- All untrusted boundaries preserve data as data: remote parsing uses character boundaries, git arguments use end-of-options protection, diff parsing tracks hunk state, HTML is escaped, mutating UI routes check origin, and editor invocation does not re-split paths through a shell.
- UI mutations stay fragment-based and the footer identifies the active store.
- Host differences are represented honestly. Claude Code, Codex, and Cursor have different stop protocols; OpenCode has no enforceable stop gate.
- If integration artifacts changed, run the `host-parity` skill.

### 6. Tests and documentation

Look for a focused regression test before implementation changes. Exit-code and hang tests are contracts; hang tests must have a time bound. Prompt changes must update the golden file deliberately.

Use the repository's mise-managed commands:

```bash
mise run check
```

Also run:

- `mise run docs` when README, `AGENTS.md`, any `SKILL.md`, CLI flags, help output, prompts, or host integrations changed.
- `mise run lint` when dependencies, shell scripts, CI, or supply-chain files changed.
- `mise run build` when packaging or release behavior changed.

Do not hide an environmental failure. Record the exact command, exit status, and the evidence that distinguishes it from a product regression.

### 7. Security and privacy

Inspect the diff for secrets and network-client dependencies. Telemetry remains opt-in, local, aggregate-only, and physically separate from the learning store. No command may leak rule text, paths, repositories, identities, diffs, or finer timestamps through telemetry.

## Finding quality

Every finding must cite a real current file and line, explain the user-visible or contract-level consequence, and propose the smallest sound fix. Re-read the cited code before reporting it. Classify findings as:

- Blocker: correctness, data integrity, security/privacy, spec violation, release breakage, or a missing required test/documentation update.
- Warning: maintainability or coverage risk worth addressing before landing.
- Note: useful but non-blocking observation.

Validate all prospective blockers with a skeptical second pass. Show dropped false positives briefly so the user can challenge the calibration.

## Verdict

Return the review scope, validated findings ordered by severity, commands run, and one verdict: `SHIP IT`, `FIX THEN SHIP`, or `NEEDS REWORK`. Reviews are read-only; offer to fix findings, but do not edit merely because they were found.

## Retrospective

Always close the workflow with a short retrospective. If it ran smoothly, say so in one line. If the user corrected the process or a check produced repeated friction, propose one precise change to this canonical skill and ask whether to apply it. Integrate approved changes where they belong in this file rather than appending a session log.
