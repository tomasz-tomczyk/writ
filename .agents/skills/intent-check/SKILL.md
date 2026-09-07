---
name: intent-check
description: "Audit a writ PR or release window for hunks that do not match stated intent, including wrong-base changes and silent cross-PR reverts. Use during review, shipping, or release preparation."
disable-model-invocation: true
---

# Intent check

## Modes

- PR mode compares one branch or PR with `origin/main` and its stated intent.
- Release-window mode inspects every commit between the latest stable release tag and the current `origin/main`.

Infer the mode from the caller. A feature branch implies PR mode; a pre-release check on main implies release-window mode. Ask only when the range materially changes the result.

## Gather evidence

Read `AGENTS.md`. Fetch remote refs when available, but do not rewrite the branch.

For PR mode, gather:

```bash
git log --format='%H%n%s%n%b%n---' origin/main..HEAD
git diff --stat origin/main...HEAD
git diff origin/main...HEAD
gh pr view --json number,title,body 2>/dev/null
```

Use the PR title/body as intent when available, then commit messages, then the user's description.

For release-window mode, resolve the latest stable release and gather the oldest-to-newest commit log plus full diff through `origin/main`. Inspect both same-window overlap and deletions of behavior that existed at the prior tag.

Report mode, range, stated intent, commit count, and file count before judging.

## Independent audit

Use one fresh agent when the host supports delegation. Give it only the stated intent, commit list, changed-file list, diff, and repository access. Ask it to look exclusively for scope mismatch:

- files or whole commits unrelated to the stated outcome;
- deletions of recently added behavior without an advertised removal;
- later commits in a release window removing earlier-window additions;
- changes that restore an older implementation through a bad rebase or merge resolution;
- tests, docs, or host artifacts removed even though the feature remains.

For suspicious deletions, use `git log -S` or `git log -G`, inspect the introducing commit, compare with the prior tag, and search HEAD for a moved or renamed equivalent. Do not turn ordinary code-quality concerns into intent findings.

If delegation is unavailable, perform this discovery pass locally without using implementation-session rationale to excuse suspicious hunks.

## Validate every flag

One auditor never blocks a ship. Give each raw finding to a fresh skeptical validator, in parallel when possible. The validator must check:

1. Does equivalent behavior still exist at HEAD under another name or path?
2. Does the relevant commit or PR explicitly advertise removal, replacement, migration, or cleanup?
3. Is the deletion paired with an equivalent addition?
4. Was the removed symbol or artifact already dead?
5. Is the touched area plausibly required by the stated feature?
6. Does git history support the alleged source and timing?

Verdicts are `CONFIRMED`, `REFUTED`, or `AMBIGUOUS`. Confirmed findings block. Refuted findings are shown once for transparency. Ambiguous findings never auto-block; they carry a concrete manual check.

## Report

Return the mode, range, stated intent, raw and validated counts, overall `CLEAN`, `SUSPICIOUS`, or `BLOCKER` verdict, and evidence for each surviving finding. In standalone use, ask whether the user wants confirmed hunks reverted, split into another PR, or explained as intentional. When called by another skill, return the table and let the caller decide.

## Retrospective

If the check was smooth, say so briefly. If history shape, a false-positive pattern, or user correction exposed a workflow gap, propose a precise update to this canonical file and apply it only with the user's approval.
