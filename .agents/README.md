# writ developer workflow skills

Repository workflows for working on this codebase: review, ship, fix, audit, and release. They live only under `.agents/skills/`. Cursor loads that path and exposes each skill as a slash command (for example `/writ-ship`).

| Skill | Use when |
| --- | --- |
| `writ-ship` | Ship branch work: review, mise gates, PR, CI, squash merge |
| `writ-review` | Pre-landing review of a writ diff |
| `deep-audit` | Whole-codebase audit with independent validation |
| `release-audit` | Audit everything since the last stable tag before releasing |
| `writ-release` | Cut a versioned release |
| `release-notes` | Draft notes for the next version |
| `intent-check` | Check a PR or release window against stated intent |
| `writ-fix` | Resolve a GitHub issue end to end |
| `host-parity` | Check Claude Code / Codex / Cursor / OpenCode plugin drift |

Follow `AGENTS.md` for product rules and invariants. Do not duplicate these skills under host-specific trees.
