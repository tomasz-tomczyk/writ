# writ developer workflow skills

Repository workflows for working on this codebase. They live only under `.agents/skills/`. Cursor loads that path and exposes each skill as a slash command (for example `/writ-ship`). Follow `AGENTS.md` for product rules and invariants.

## Composition

Leaf skills do one job. Higher-level skills call them:

```
intent-check          (leaf)
host-parity           (leaf)
release-notes         (leaf)

writ-review
  ├─ intent-check
  └─ host-parity          (when plugins/host artifacts changed)

writ-ship
  ├─ writ-review
  │    ├─ intent-check
  │    └─ host-parity?
  └─ host-parity?         (again if host surfaces changed after fixes)

writ-fix
  ├─ host-parity?         (when host integration behavior changed)
  ├─ writ-review
  └─ writ-ship            (only if the user asked to finish through merge)

release-audit
  ├─ intent-check         (release-window mode, via subagent)
  └─ host-parity?         (when host-related files changed)

writ-release
  ├─ release-audit        (must be clean first)
  ├─ release-notes
  └─ host-parity?         (if versioned plugin manifests drift)

deep-audit                (standalone discovery → validate → optional fixes)
  └─ writ-review          (per fix group, when implementing)
```

`?` means conditional on the diff.

## Skills

| Skill | Role |
| --- | --- |
| `intent-check` | Diff vs stated intent (PR or release window) |
| `host-parity` | Claude Code / Codex / Cursor / OpenCode plugin drift |
| `writ-review` | Pre-landing review; spawns domain subagents + validators |
| `writ-ship` | Review → mise gates → PR → CI → squash merge |
| `writ-fix` | GitHub issue → worktree → fix → review → optional ship |
| `deep-audit` | Whole-codebase audit with independent validation |
| `release-audit` | Everything since last stable tag before cutting a release |
| `release-notes` | Draft `vX.Y.Z-release-notes.md` (gitignored) |
| `writ-release` | Version bump, tag, and verify publish outputs |
