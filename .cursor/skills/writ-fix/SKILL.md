---
name: writ-fix
description: "Resolve a GitHub issue for writ end to end: fetch issue context, create a Worktrunk worktree, reproduce with a failing test, implement, review, and optionally ship. Use for writ GitHub issue URLs or numbers; not Linear or Sentry."
disable-model-invocation: true
---

# Fix a writ GitHub issue

This workflow accepts only a GitHub issue in `tomasz-tomczyk/writ`: a full URL,
`writ#N`, or an unambiguous issue number. It does not query Linear or Sentry.

Run routine local steps autonomously. Stop for unclear requirements, a design
contradiction, destructive recovery, external PR/merge approval, or a genuine
blocker.

## Read the issue and contract

Fetch the issue title, body, labels, assignees, and comments with `gh`. Summarize
the requested behavior, acceptance criteria, likely area, and open ambiguity.

Read `AGENTS.md` completely. Require
`docs/superpowers/specs/2026-09-06-writ-design.md` and read the relevant
sections before changing behavior. If the requested behavior contradicts the
approved design, stop and explain the conflict; do not silently amend the spec
or implement around it.

## Create the worktree

Choose `fix/<issue>-<slug>` for a bug, `feat/<issue>-<slug>` for a feature, or
`refactor/<issue>-<slug>` for cleanup. From the canonical repository:

```bash
git fetch origin main
wt switch --create --base origin/main <branch-name>
```

Confirm the real worktree path with `wt list` and verify the branch has zero
commits ahead of `origin/main` before editing. Use Worktrunk, not an ad-hoc git
worktree. Keep every change inside that worktree.

## Reproduce first

Follow TDD. Find the narrowest relevant existing test surface and add a failing
regression test before implementation. For CLI paths, assert observable output
and the exact exit code. Bound any hang regression test. For prompt changes,
use the byte-for-byte golden contract. For SQLite behavior, include realistic
existing-store state when relevant. For MCP or hooks, test parity with the CLI
and the host protocol.

Run the focused test and prove it fails for the expected reason. If automated
reproduction is genuinely unsuitable, document a repeatable manual
reproduction. If the issue cannot be understood well enough to reproduce, ask
the user rather than guessing.

## Implement narrowly

Trace the full call path and make the smallest design-compliant fix. Select the
relevant expert lens:

- Rust/domain architecture for `writ-core` and shared decisions;
- SQLite for migrations, triggers, FTS, scopes, counters, and persistence;
- CLI/system boundaries for git, stdin, output, exit codes, config, UI, and
  telemetry;
- MCP/hooks for schemas, prompts, retry signals, install behavior, and host
  protocols.

When independent agents are available and domains can work without editing the
same files, use fresh agents in parallel. Give them the verified worktree path,
issue text, failing test, relevant spec clauses, and explicit ownership. The
main workflow remains responsible for integrating and validating their work.

Preserve writ's invariants: core has no I/O, writes default proposed, archives
preserve evidence, exemplar locations are never durable, repo identity follows
the normalized remote, prompt output is capped without false selection-cost
claims, failures and exit codes are honest, database triggers own ordinary
`updated_at`, docs/skills/CLI agree, hook entry and ingest gate differently, and
selected/applied counters move at different moments.

Make the regression test pass, then run nearby tests. Commit coherent changes
with Conventional Commit subjects.

## Verify

Run:

```bash
mise run check
```

Add `mise run docs` for docs, skills, CLI/MCP/help, prompts, install, or plugin
changes. Add `mise run lint` for dependencies, scripts, CI, or manifests. Run
`host-parity` when host integration behavior changed.

Then invoke `writ-review`. Fix validated blockers and re-run affected checks
until the reviewed HEAD is clean.

## Ship when authorized

If the user asked for the issue to be completed through merge, invoke
`writ-ship` after review and ensure the PR body contains `Closes #N`. If the
user asked for manual review or no merge, create the PR only after showing the
draft and leave it open. If the original request was only to implement a fix,
do not infer permission to push or open a PR.

After merge, use Worktrunk cleanup from the canonical checkout. If the PR is
left open or the work is unshipped, retain the branch and worktree and report
their paths.

## Final report

Return the issue, root cause, regression test, implementation summary, review
verdict, checks, PR state, and branch/worktree state.

## Retrospective

Always close with a brief workflow retrospective. If anything caused friction,
propose one specific canonical-skill improvement and apply it only after the
user agrees.
