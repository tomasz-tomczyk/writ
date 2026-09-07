# writ developer workflow skills

These are repository-developer workflows for working **on writ**. They review,
ship, fix, audit, and release this Rust repository. They are not writ product
features, do not replace the `writ audit` learning-ledger command, and do not
belong under `plugins/`, which contains integrations shipped to writ users.

Canonical skill bodies live only under `.agents/skills/`. Cursor loads that
path natively and exposes each skill as a slash command (for example
`/writ-ship`). Do not maintain parallel `.cursor/skills`, `.cursor/commands`,
`.claude/*`, or `.opencode/*` copies of these workflows.

## Imported and adapted

| writ skill | crit-meta source | Fitness for writ |
| --- | --- | --- |
| `writ-ship` | `crit-ship` | One Rust repo; Worktrunk, writ review, mise gates, one PR, full CI, squash merge |
| `writ-review` | `crit-review` | writ invariants plus Rust, SQLite, CLI, MCP, hooks, security, docs, and tests |
| `deep-audit` | `crit-audit` | Full-codebase discovery and independent validation; renamed to avoid the CLI collision |
| `release-audit` | `release-audit` | Latest-tag window, clobber check, accumulated regression review, and release gates |
| `writ-release` | `crit-release` | Workspace/Cargo/Nix/plugin versions, `vX.Y.Z` on main, GitHub/Homebrew/crates/Nix verification |
| `release-notes` | `release-notes` | writ-only changelog, PR and issue attribution, no crit-meta contributor database |
| `intent-check` | `crit-intent-check` | PR and release-window scope audit with independent per-finding validation |
| `writ-fix` | `crit-fix` | GitHub-only issue flow with TDD, Worktrunk, writ review, and optional ship |

## Added for writ

| writ skill | Why it exists |
| --- | --- |
| `host-parity` | Keeps plugin manifests, snippets, the canonical record skill, hooks, MCP configuration, installer behavior, docs, and host-specific limitations aligned across Claude Code, Codex, Cursor, and OpenCode |

## Deliberately skipped

| crit-meta workflow | Reason |
| --- | --- |
| `crit-parity` | It compares crit's Go and Phoenix frontends; writ has one Rust implementation. Host parity is the relevant replacement. |
| `crit-copy`, `crit-twitter`, `crit-hn`, `crit-intel`, `crit-release-tweet` | Marketing and community workflows are outside repository development. |
| `sentry-fix` | writ has no crit-web Sentry workflow. |
| `linear`, `crit-refine` | The writ fix workflow is intentionally GitHub-only. |

## Naming boundary

There is intentionally no developer skill named `writ-audit`. That spelling
belongs to the product command and its learning-ledger semantics. Use
`deep-audit` for a whole-codebase audit and `release-audit` for a release window.
