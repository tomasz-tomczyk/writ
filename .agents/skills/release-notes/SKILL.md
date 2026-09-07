---
name: release-notes
description: "Generate draft release notes for the next writ version from commits and merged GitHub PRs since the latest stable tag. Use when preparing writ release notes or choosing the next version."
---

# Generate writ release notes

Work in the writ repository and read `AGENTS.md` plus the local design spec for product terminology. Release notes describe what users receive; they do not reinterpret the design.

## Gather the release window

Fetch `origin/main`. Resolve the latest stable GitHub release tag, then collect:

- commits from that tag through `origin/main`, oldest to newest;
- merged PR number, exact title, author, body, labels, and linked issues for each commit;
- issue authors for issues closed or explicitly credited by those PRs;
- the aggregate diff and user-visible documentation changes.

Exclude drafts, prereleases, merges outside the window, and uncommitted local work. If commit-to-PR mapping is ambiguous, verify with GitHub rather than inventing attribution.

Report the previous tag, commit count, PR count, and any commits without PR metadata.

## Propose the next version

Use semantic versioning and the actual impact:

- incompatible user-facing behavior requires the appropriate breaking bump;
- a meaningful backward-compatible capability normally bumps minor;
- fixes, docs, and internal improvements normally bump patch.

Before 1.0, explain how the repository's existing version pattern affects this choice. Show the proposed version and rationale and ask the user to confirm it before finalizing the draft.

## Write the notes

Write `vX.Y.Z-release-notes.md` at the repository root. Use this structure:

1. `## What's Changed`
2. A concise narrative of the most important user-visible outcomes.
3. Optional short feature callouts only for changes that merit explanation.
4. A grouped change list ordered by user impact; internal refactors, tests, CI, and dependency work come last.
5. A new-contributors section when applicable.
6. A full changelog link comparing the previous and proposed tags.

Keep each change entry faithful to its merged PR or commit. Preserve exact commit subjects when presenting the changelog list; add PR link and author attribution without rewriting history. Put community contributions first in a group and thank external PR authors. When a maintainer PR was driven by an external issue, credit the issue reporter only after verifying the link and identity.

Group by user outcome, not file or commit prefix. Do not create a one-item section unless it is a genuine release headline. Do not guess social handles or maintain a crit-meta contributor database; writ has no such repository-local system.

## Verify

Cross-check every listed item against the release-window commit and PR data. Confirm the changelog URL, proposed tag, contributor names, and linked issue references. Return the draft path, version recommendation, contributor credits, and any metadata gaps. The notes are a draft for user review; do not tag or publish from this skill.

## Retrospective

If gathering or attribution exposed repeatable friction, propose one precise update to this canonical skill and apply it only with user approval. Otherwise state briefly that the workflow was smooth.
