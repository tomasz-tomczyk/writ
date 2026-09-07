---
name: release-audit
description: "Audit all writ changes since the latest stable release for regressions, clobbers, inconsistency, test gaps, host drift, and release risk. Use immediately before cutting a writ release."
disable-model-invocation: true
---

# Pre-release audit

Run this before tagging. It reviews the exact release window; after a tag moves, that window is no longer the same.

## Establish the release window

Read `AGENTS.md`, then inspect `mise.toml`, `.github/workflows/release.yml`, Cargo manifests, `flake.nix`, the README, and plugin documentation.

Fetch remote refs. Resolve the latest stable release from GitHub or reachable tags, and use the up-to-date `origin/main` as the upper bound. Do not rely on a stale local `main` or mutate the current checkout merely to audit it.

Gather the oldest-to-newest commit log, full diff stat, changed files, and release PR metadata. Categorize files as Rust core, Rust CLI, SQLite/migrations, tests, host integrations, docs, dependencies/scripts/CI, and packaging.

Report:

- previous stable tag and date;
- exact tag-to-`origin/main` range;
- commit and file counts by category;
- whether local main was stale;
- uncommitted work excluded from the release window.

## Release gates

### 1. Intent and clobber audit (subagents)

Dispatch these as **fresh subagents** (parallel when possible), not as inline checks in the orchestrator:

1. An agent that runs the `intent-check` skill in release-window mode and returns its CONFIRMED / REFUTED / AMBIGUOUS table.
2. A separate clobber agent with the release-range commit log and full diff. It looks for later commits that undo earlier commits in the same window, and for unadvertised removal of behavior that existed at the prior tag.

A confirmed clobber is a release blocker and should be restored in its own PR so history remains honest.

### 2. Parallel domain review (subagents)

Dispatch fresh Rust, SQLite, CLI, and MCP/hooks reviewers in parallel. Give each the release range, changed files, commit log, `AGENTS.md` invariants, and only the relevant source.

Review accumulated changes for:

- inconsistent patterns introduced by separate PRs;
- dead code or obsolete tests left by iterative work;
- behavioral changes without focused regression tests;
- status, scope, timestamp, FTS, foreign-key, archive, exemplar, identity, prompt-cap, counter, or gate-semantics drift;
- dishonest exit errors, hanging stdin paths, CLI/MCP mismatch, unsafe boundary parsing, UI security, or telemetry privacy regression;
- stale host snippets, skills, hooks, MCP configuration, manifests, READMEs, or installer output;
- dependency, Cargo lock, Nix, package metadata, or release-workflow mismatch.

Every finding needs a verified current file and line plus a concrete release impact. Avoid speculative refactors and performance claims outside writ's actual scale.

### 3. Validate findings (subagents)

Give each domain's findings to a fresh skeptical validator agent (one per domain, in parallel). It must verify the claim against code, tests, `AGENTS.md`, and the actual execution model, then return `REAL`, `PARTIAL`, or `FALSE POSITIVE` with evidence. Carry only real and supported part of partial findings forward; list dropped findings briefly.

### 4. Execute repository gates

Run:

```bash
mise run check
mise run docs
mise run lint
mise run build
```

Run `mise run coverage` when coverage risk changed materially or when preparing the final release candidate. If Nix is available, run the repository's Nix smoke test used by release CI. Treat missing optional local tooling as an explicit verification gap, not an implicit pass.

For host-related changes, run `host-parity`. For CLI flags and MCP tools, ensure both focused tests and documentation checks ran.

### 5. Release metadata dry check

Before release, verify one proposed version can be applied consistently to:

- `[workspace.package] version` in root `Cargo.toml`;
- the `writ-cli` dependency requirement on `writ-core`;
- `Cargo.lock` package versions;
- `flake.nix` package version;
- versioned plugin manifests.

Confirm `.github/workflows/release.yml` accepts `vX.Y.Z`, requires the tag commit to be on main, and publishes the current GitHub binaries, checksums, Homebrew formula, crates in dependency order, and Nix package. Do not perform the bump or tag in an audit-only invocation.

## Report and next action

Return P0 release blockers, P1 fixes recommended before tag, P2 cleanup, host parity status, command results, and a `READY TO RELEASE` or `FIX BEFORE RELEASE` verdict. Group surviving fixes into small independent workstreams and ask which to implement. Use `wt` worktrees for approved fixes and review them before offering `writ-release`.

## Retrospective

Always close with either a brief smooth-run note or a precise proposal to update this canonical workflow based on observed friction. Apply workflow changes only after user approval.
