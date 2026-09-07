---
name: writ-release
description: "Cut a writ release end to end: audit the release window, prepare notes, synchronize Cargo, Nix, lockfile and plugin versions, land the bump on main, tag vX.Y.Z, watch release CI, and verify GitHub, Homebrew, crates.io, and Nix outputs. Use when asked to release or tag writ."
disable-model-invocation: true
---

# Release writ

This mutates repository and external release state. Keep explicit user approval at the notes/version decision and immediately before pushing the release commit or tag. Never force-push a tag.

## Preflight

Read `AGENTS.md`, and inspect `mise.toml`, root and crate manifests, `Cargo.lock`, `flake.nix`, plugin manifests, `.github/workflows/release.yml`, and the latest repository state.

Require:

- a clean release candidate with no unexplained local changes;
- all intended feature PRs merged to `origin/main`;
- no existing tag or GitHub release for the proposed version;
- authenticated access needed to push and inspect GitHub;
- a completed `release-audit` with no unresolved blockers.

Do not release from a stale local main. Fetch `origin/main`, switch to the canonical main checkout, and fast-forward it. If local main has unique commits or cannot fast-forward, stop and resolve that history explicitly.

## Notes and version

Run `release-notes`. Let the user review and edit the generated draft, and get confirmation of `X.Y.Z` before changing version files.

The release version has multiple coupled representations. Update them together:

1. `[workspace.package] version` in root `Cargo.toml`.
2. The published `writ-core` version requirement in `crates/writ-cli/Cargo.toml`.
3. Workspace package entries in `Cargo.lock`, regenerated through Cargo rather than edited by hand.
4. The package version in `flake.nix`.
5. Every version-bearing plugin manifest under `plugins/`.

Do not bump Rust toolchain versions or unrelated dependencies as part of the release. Run `host-parity` if plugin manifests or integration artifacts expose drift.

Commit only the release-version changes with a Conventional Commit subject such as `chore: release vX.Y.Z`. Release notes may remain an operator draft unless the repository convention at release time explicitly tracks them.

## Validate before publishing

Run:

```bash
mise run check
mise run docs
mise run lint
mise run build
```

Run the Nix smoke test used by release CI when Nix is available. Use Cargo metadata to confirm both crates report `X.Y.Z`, the CLI's dependency requirement matches the core crate, and the lockfile is current.

Re-read `.github/workflows/release.yml` rather than relying on this prose. At the current contract, a `vX.Y.Z` tag must point to a commit on main and equal the `writ-cli` workspace version. The workflow validates provenance, builds and checks Nix, creates platform binaries and checksums, publishes the GitHub release, updates Homebrew, and publishes `writ-core` before `writ-cli` to crates.io.

Show the exact release commit, tag, notes file, and validation results. Ask for approval before pushing main and the tag.

## Publish

Push the release commit to main through the repository's allowed path. If direct push is rejected by rules, create a focused release-bump PR, watch all checks, squash merge it, and refresh local main. Do not bypass repository protection without explicit authority.

After verifying the release commit is reachable from `origin/main`, create the annotated or lightweight tag style already used by the repository and push only `vX.Y.Z`. Never move or replace an existing remote tag.

## Watch and verify

Watch the release workflow through completion. If it fails, diagnose the specific job and preserve the tag while deciding the correct recovery; do not delete, retag, or blindly retry.

When GitHub release creation completes, apply the user-approved notes if the workflow's generated notes need replacement. Then verify:

- the GitHub release exists at `vX.Y.Z` with every expected binary and `checksums.txt`;
- the Homebrew formula references the tag and correct artifact checksums;
- `writ-core` and `writ-cli` version `X.Y.Z` are visible on crates.io;
- the Nix job passed and the flake reports the new package version.

Account for crates.io index propagation without treating it as a code failure; the workflow already retries the dependent CLI publication. Report pending external propagation honestly rather than calling the release complete early.

## Completion and retrospective

Return the version, release commit and tag, workflow URL/status, GitHub release URL, and the verified state of GitHub artifacts, Homebrew, crates.io, and Nix. List anything still propagating.

If the release exposed friction or a stale assumption, propose one precise edit to this canonical workflow and apply it only after user approval. Otherwise close with a one-line smooth-run note.
