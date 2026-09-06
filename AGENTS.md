# writ — agent instructions

`writ` is a local-first ledger of the steering a developer gives coding
agents. Capture a lesson once, audit each diff against the lessons that
apply, keep the collection visible and prunable.

**The spec is the source of truth**, and it is deliberately not tracked
in this repository. Locally it lives at
`docs/superpowers/specs/2026-09-06-writ-design.md`. If that path is
absent, ask for it before changing behavior — do not infer the design
from the code.

If the code and the spec disagree, the spec wins. If the spec is wrong,
stop and say so. Do not amend it silently.

The invariants below are the part that must survive without it.

## Commands

The toolchain is managed by mise. Always go through it.

```
mise exec -- cargo test --workspace
mise run check      # fmt, clippy -D warnings, test
mise run build
```

## Layout

```
crates/writ-core/   storage, selection, budget, dedupe.
                    Knows nothing about a terminal, socket, or HTTP.
crates/writ-cli/    the `writ` binary. The only crate that does I/O.
plugins/claude-code/  the /learn skill and the Stop hook.
```

## Invariants

These are decisions, not preferences. Breaking one is a spec violation.

1. **`writ-core` has no I/O surface.** No printing, no HTTP, no MCP. If
   a core function needs to report something, it returns it.
2. **Writes default to `status = 'proposed'`.** A caller that forgets
   `--activate` must fail safe into the Inbox.
3. **Exemplars never store a path or a line number.** They hold snippet
   text. Only `findings` carry a location, and only for one audit.
4. **Repo identity comes from the normalized remote, never the path.** A
   worktree is the same project as its main checkout.
5. **Audit cost is bounded by the diff, not the collection.** Selection
   caps at `max_rules` and `max_chars`.
6. **Fail honestly.** Every error names its real cause and exits with
   its own code. Never hang on stdin. See spec P7.
7. **Docs, skills, and the CLI agree.** A flag in the README or a
   SKILL.md that the binary rejects is a bug. See spec P8.

## Working style

- TDD. Write the failing test first.
- Exit codes are a contract. Test every one.
- `writ audit --format prompt` output is asserted byte for byte.
