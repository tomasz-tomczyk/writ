---
name: writ-review
description: "Review a writ repository diff before landing it, with emphasis on Rust, SQLite, CLI and MCP contracts, host hooks, and writ's `AGENTS.md` invariants. Use for code review, pre-landing review, or checking changes in this repository."
disable-model-invocation: true
---

# Pre-landing review for writ

## Establish the review contract

1. Read `AGENTS.md` completely.
2. Determine the intended change from the user's request, branch commits, and PR title/body when available.
3. Fetch `origin/main` when network access is available. Review the branch-only three-dot diff against `origin/main`. Do not rebase, commit, or edit during a review unless the user separately asks for fixes.
4. Include tracked, staged, unstaged, and relevant untracked work. Identify unrelated dirty files and leave them alone.

Report the branch, base, commit count, changed files, intended outcome, and whether the review is quick, standard, or deep.

## Review phases

Dispatch each phase that uses agents as a **fresh subagent** with no implementation-session context. Prefer parallel agents when phases do not depend on each other. Wait for returns before the verdict.

### 1. Intent check

Invoke the `intent-check` skill in PR mode (via Skill / slash command — not as a fake agent type). That skill dispatches its own independent auditor and per-finding validators.

### 2. Architectural boundaries (fresh agent)

Prompt a fresh agent with the three-dot diff, changed files, and `AGENTS.md`. It must review complete affected call paths, not only hunks:

- `writ-core` is pure domain logic. It has no printing, terminal, filesystem, process, socket, HTTP, or MCP surface. I/O belongs in `writ-cli`.
- The core knows nothing about capture sources. All sources converge on the same write behavior and the schema must not grow source-specific concepts.
- The core loop has no required external install. Optional tools degrade to a useful scope-only result instead of failing the audit.
- Plugins remain integrations around the binary, not alternate implementations of product behavior.
- One workspace and one `writ` binary remain the release unit.

### 3. Data and SQLite invariants (fresh agent)

Fresh agent. Trace migrations, model conversion, store methods, and tests together:

- Writes default to `proposed`; activation is explicit.
- Pruning archives and preserves evidence.
- Exemplars store snippet text, never a path or line. Findings alone may carry an audit-local location.
- Repository identity comes from a normalized remote. A worktree must resolve to the same project as its main checkout; the documented no-remote fallback must remain honest.
- Scope kinds AND with each other and values OR within a kind. `global` cannot be combined with another kind.
- `updated_at` is maintained by database triggers. Ordinary writes never set it explicitly; import is the deliberate exception.
- Foreign keys are enabled on every connection, FTS external-content triggers remain complete, and SQL values are bound rather than interpolated.
- Migration changes are forward-safe for existing databases and are tested from both an empty and an older store when applicable.

### 4. Audit and gate semantics (fresh agent)

Fresh agent. Treat these as separate moments:

- Hook entry sends the agent back when any learning is selected, advisory or blocking, because findings do not exist yet.
- Ingest blocks only when a blocking finding is open or ignored.

Also verify:

- `times_selected` and `last_selected_at` move on emit; `times_applied` and `last_applied_at` move on ingest. Never collapse or compare them as absolute counts; their ratio is the signal.
- Prompt output is capped by both rule count and characters. Do not claim that selection work is bounded by the diff or prompt cap.
- Prompt rendering tells the host how to return findings through MCP or CLI, includes blocking/advisory state, and preserves the byte-for-byte golden contract.
- Retry state comes from each host, not a counter writ invents.
- Hook-mode exit behavior follows the host protocol. The general CLI exit-code table still applies outside hook mode.
- Required stdin commands do not silently truncate; hook input never hangs.

### 5. CLI, MCP, UI, and host integrations (fresh agent)

Fresh agent:

- Each failure cause has its own honest message and exit code. Check usage, malformed JSON, unknown IDs, non-git directories, empty diffs, and storage failures where relevant.
- New flags have parsing tests and end-to-end behavior tests. CLI and MCP paths reach the same core behavior, and tool schemas remain intentionally small.
- All untrusted boundaries preserve data as data: remote parsing uses character boundaries, git arguments use end-of-options protection, diff parsing tracks hunk state, HTML is escaped, mutating UI routes check origin, and editor invocation does not re-split paths through a shell.
- UI mutations stay fragment-based and the footer identifies the active store.
- Host differences are represented honestly. Claude Code, Codex, and Cursor have different stop protocols; OpenCode has no enforceable stop gate.
- If integration artifacts changed, run the `host-parity` skill after this agent returns.

### 6. Tests and documentation (fresh agent)

Fresh agent. Look for a focused regression test before implementation changes. Exit-code and hang tests are contracts; hang tests must have a time bound. Prompt changes must update the golden file deliberately.

Then run (orchestrator, not a review agent):

```bash
mise run check
```

Also run:

- `mise run docs` when README, `AGENTS.md`, any `SKILL.md`, CLI flags, help output, prompts, or host integrations changed.
- `mise run lint` when dependencies, shell scripts, CI, or supply-chain files changed.
- `mise run build` when packaging or release behavior changed.

Do not hide an environmental failure. Record the exact command, exit status, and the evidence that distinguishes it from a product regression.

### 7. Security and privacy (fresh agent)

Fresh agent. Inspect the diff for secrets and network-client dependencies. Telemetry remains opt-in, local, aggregate-only, and physically separate from the learning store. No command may leak rule text, paths, repositories, identities, diffs, or finer timestamps through telemetry.

### 8. Validate findings

Review agents are over-eager. Before presenting anything, dispatch **fresh validator agents in parallel** (one per domain agent from phases 2–5 and 7). Each validator critically re-checks its domain's findings against the actual code and returns `REAL`, `PARTIAL`, or `FALSE POSITIVE` with evidence. Carry only real findings and the supported part of partial findings forward; list dropped false positives briefly.

## Finding quality

Every surviving finding must cite a real current file and line, explain the user-visible or contract-level consequence, and propose the smallest sound fix. Classify findings as:

- Blocker: correctness, data integrity, security/privacy, invariant violation, release breakage, or a missing required test/documentation update.
- Warning: maintainability or coverage risk worth addressing before landing.
- Note: useful but non-blocking observation.

## Verdict

Re-fetch `origin/main` before the summary if the review ran long. Return the review scope, validated findings ordered by severity, commands run, and one verdict: `SHIP IT`, `FIX THEN SHIP`, or `NEEDS REWORK`. Reviews are read-only; offer to fix findings, but do not edit merely because they were found.

## Retrospective

Always close the workflow with a short retrospective. If it ran smoothly, say so in one line. If the user corrected the process or a check produced repeated friction, propose one precise change to this canonical skill and ask whether to apply it. Integrate approved changes where they belong in this file rather than appending a session log.
