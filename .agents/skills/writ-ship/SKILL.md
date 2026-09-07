---
name: writ-ship
description: "Ship current writ repository work through assessment, rebase, writ-specific review, mise checks, PR creation, CI watch, squash merge, and safe worktree cleanup. Use when asked to ship, create a PR, send changes, or merge writ work."
disable-model-invocation: true
---

# Ship writ repository work

This ships changes to the writ codebase. It does not replace or invoke the product's learning-ledger audit except when a test explicitly exercises that CLI behavior.

Run unattended through routine reversible steps. Pause for a material choice, destructive cleanup, unresolved conflict, PR approval, or failed gate.

## Assess

Read `AGENTS.md` and require the local design spec. Fetch `origin/main` without pulling, then report:

```bash
git branch --show-current
git status --short
git log --oneline origin/main..HEAD
git log --oneline HEAD..origin/main
git diff --stat origin/main...HEAD
git stash list
```

Include staged, unstaged, untracked, unpushed, and stashed work. Leave obviously unrelated scratch files alone; include docs and tests when they belong to the change.

Feature and fix work belongs in a Worktrunk worktree. If the current checkout is `main` or the canonical checkout has uncommitted feature work, stop and offer to move the work into a branch created explicitly from `origin/main`:

```bash
wt switch --create --base origin/main <branch-name>
```

Use the installed `wt` syntax shown by `wt switch --help` if it differs. Do not create ad-hoc git worktrees for normal development.

Scan every ahead commit for unrelated subjects and the three-dot diff for unrelated hunks. A suspicious inherited commit list usually means the worktree was based on another feature branch. Recreate from `origin/main` or ask the user whether those commits truly belong; do not ship them by assumption.

If relevant work is uncommitted inside the correct feature worktree, commit it with a Conventional Commit subject. If there is unrelated or ambiguous dirty work, ask whether to commit only the scoped files, preserve it while shipping already committed work, or stop.

## Rebase

Rebase the feature branch onto the freshly fetched `origin/main` before review. Use force-with-lease only for the feature branch after a successful rebase; never force-push main or a tag.

Resolve simple conflicts by understanding both sides. For a conflict with two plausible behaviors, explain the semantic choice and ask. Re-run affected tests after resolution.

## Review and fix

Invoke `writ-review`. It includes the independent `intent-check`, design invariants, Rust/SQLite/CLI/MCP/hooks review, security/privacy, tests, and docs. Do not substitute the product command `writ audit` for this developer review.

Fix every blocker, commit the fixes, and repeat affected review phases until the verdict is `SHIP IT`. If the caller already completed `writ-review` against the same final diff, verify the reviewed commit still matches HEAD before reusing it.

When plugins or host behavior changed, run `host-parity` before proceeding.

## Local gates

Always run:

```bash
mise run check
```

Also run:

- `mise run docs` when docs, skills, CLI flags/help, MCP schemas, prompts, install behavior, or plugins changed;
- `mise run lint` when dependencies, scripts, CI, manifests, or supply-chain configuration changed;
- `mise run build` for packaging or release-sensitive changes.

Review the diff for new branching behavior that lacks focused tests. Do not delegate unknown local failures to CI: distinguish regression, flaky external service, and missing prerequisite with evidence.

## Draft the pull request

Choose a Conventional Commit title under about 70 characters. Draft a body with:

- summary of what changed and why;
- design/spec implications;
- review and parity results;
- exact test commands and outcomes;
- a real closing issue reference when applicable.

Audit every bare `#N` token so internal numbering does not accidentally link an unrelated GitHub issue.

Show the title and body and ask the user to create-and-merge, adjust, create but leave open, or stop. This approval is required because pushing, opening a PR, and merging change external state.

## Push, watch, and merge

Push the feature branch and create or update one PR in `tomasz-tomczyk/writ`. Never push directly to main.

Watch the full check set. A queued or running check is not green. After the watcher finishes, query the complete check rollup and require every check to be successful, skipped, or neutral. Investigate failures; do not merge merely because the fast checks passed.

When green and authorized, squash merge. Verify the PR state and merge commit after the command returns; empty command output is not proof either way.

## Cleanup

Fetch and prune from the canonical checkout after merge. For a Worktrunk worktree, inspect its status and then use `wt remove`; let Worktrunk remove the worktree and merged branch in its supported order. Never force cleanup when the worktree contains genuine uncommitted work. Ask before discarding anything not already identified as unrelated scratch material.

Report PR URL, merge state, final checks, branch/worktree cleanup, and anything left intentionally.

## Rules

- One repository, one PR, squash merge.
- Conventional Commit subjects for commits and PR titles.
- Never direct-push or force-push main; never force-push tags.
- Re-run review if HEAD changes materially after review.
- Use the repository's mise tasks, not ambient Cargo tooling.
- Preserve user work and report every destructive cleanup.

## Retrospective

Always report whether the ship workflow had friction. If the user corrected the process or a repeated problem appeared, propose one concrete edit to this canonical skill and apply it only after approval.
