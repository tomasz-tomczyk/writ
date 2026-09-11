# writ — agent instructions

`writ` is a local-first ledger of the steering a developer gives coding
agents. A correction lands in one session and then disappears, so writ
gives it three things it lacks: a durable place to sit, a step that
checks each new diff against the corrections that apply to it, and a
view of the whole collection so it can be pruned when it goes stale.
Nothing leaves the machine, and writ never calls a model itself — the
host agent does the reasoning.

**The spec is the source of truth**, and it is deliberately not tracked
in this repository. Locally it lives at
`docs/superpowers/specs/2026-09-06-writ-design.md`. If that path is
absent, ask for it before changing behavior — do not infer the design
from the code.

If the code and the spec disagree, the spec wins. If the spec is wrong,
stop and say so. Do not amend it silently.

The rest of this file is the part that must survive without it.

## Principles

Spec section 2, reduced to the parts that decide a question.

1. **P1 — The core knows nothing about its sources.** Every source calls
   one write command, and the schema never names a source.
2. **P2 — Nothing reaches the audit unapproved.** A write defaults to
   `proposed`. Activation is explicit.
3. **P3 — Prompt size is bounded by the cap. Selection cost is not.** A
   thousand learnings must produce the same prompt as fifty. They will
   not produce it as fast: selection is linear in the collection.
4. **P4 — Nothing is destroyed.** Pruning archives. A pruned rule is
   still evidence.
5. **P5 — The terminal is for findings. The UI is for the collection.**
   An audit never opens a browser and never waits for a human.
6. **P6 — The core loop requires no external install.** An optional
   capability may use an external tool. Its absence degrades the result,
   never fails it.
7. **P7 — Fail honestly.** Each cause gets its own message and its own
   exit code. Never mask one as another, and never hang on stdin.
8. **P8 — Docs, skills, and the CLI agree.** CI exercises every flag in
   the README and in every shipped SKILL.md.
9. **P9 — Defer speculative structure unless it is expensive to
   retrofit.** Identity and timestamps are expensive. A nullable column
   is not.

## The model

Four nouns. `store.rs` is easier to read once they are clear.

**A learning** is one rule with its reason: `title`, `rule`, and
`rationale`. Rationale is required and never empty, because the reason
is what lets a rule transfer to a case its author did not foresee. It
carries `blocking`, a boolean and not a severity scale, because "does
breaking this stop the handoff" is answerable and "how strongly do you
mean it" is not. It also carries `sides`: `added`, `removed`, or
`both` (the default), which half of the diff the rule cares about.
Status runs `proposed` → `active` → `archived`. Only an
`active` learning is ever selected.

**A scope** says where a learning applies: `global`, `project:<id>`,
`language:<lang>`, or `glob:<pattern>`. A learning holds one or more
scope rows, so one mechanism carries all four kinds. Scopes AND across
kinds and OR within a kind. Pure OR would mean a second scope widens a
rule, so a narrow rule could only ever carry one. `global` cannot be
combined with another kind, because "everywhere, and also only Elixir"
has no meaning.

**An audit** is one selection run over one diff. It records what it
considered, what it sent, and how many findings came back.

**A finding** is one violation an agent reported against one learning in
one audit. It may carry a path and a line. That is report data for that
audit alone, which is why invariant 3 keeps a location off an exemplar.

**Two counter pairs, and conflating them was a defect.**
`times_selected` and `last_selected_at` move at emit: this rule was put
in front of a reviewer. `times_applied` and `last_applied_at` move at
ingest: this rule actually caught something. One pair cannot answer
both. High `times_selected` with `times_applied` at zero is the whole
point — a rule that costs budget on every audit and has never found a
thing. Read the ratio between them, never the absolute number.

## The gate has two moments

They gate on different things, and this is the piece the spec itself got
wrong twice. Spec section 9.2.

| Moment | Command | Blocks when |
| --- | --- | --- |
| Hook entry | `writ audit --hook HOST` | **any** learning is selected, blocking or advisory |
| Ingest | `writ audit --ingest` | any **blocking** finding is `open` or `ignored` |

So `blocking` does not decide whether the agent is sent back to review.
It decides whether an unfixed violation stops the work. A Stop hook runs
`writ audit` without `--ingest`, so no finding exists yet and there is
nothing for `blocking` to gate on. Gating hook entry on it would select
advisory learnings, count them, and drop them unread.

## What the gate audits

**`--diff` defaults to the working tree against HEAD, and a gate must
not take that default.** An agent that commits as it goes leaves a clean
tree when the hook fires, so the gate sees an empty diff and passes.
That silently disabled the gate for 34 of the ledger's first 53 audits.
`considered = 0` on most `audits` rows is the symptom. Spec section 9.2,
**What the gate audits**.

The range belongs to the moment, not to the binary, so the hook command
carries it and writ's default is unchanged.

| Hook | Range | Why |
| --- | --- | --- |
| `Stop` | `merge-base` against the remote's default branch | The agent may have committed |
| `SubagentStop` | the default: working tree against HEAD | A subagent has not committed, so the tree is its own work |

Do not give the subagent hook the branch point. It would hand a
read-only subagent every violation its parent had already committed.

Only Claude Code has `SubagentStop`; Codex and Cursor have no such
event. Its payload carries `agent_type`, and using it to skip read-only
agents is tempting and wrong — a hand-maintained list of agent names
drifts, which is P8.

The diff in a prompt is **not** bounded by `max_rules` or `max_chars`,
which bound rule text. A long branch renders a large prompt every turn.
That cost is accepted: a large prompt beats a gate that audits nothing.

## The gate points, it does not paste

What `--hook` emits is a **pointer** — the audit id and the two ways to
fetch what is behind it — never the prompt. Every host renders a blocked
turn's message into the transcript verbatim, so pasting a prompt that
carries the whole diff puts 100 KB of a long branch in front of the
developer after every turn, for a document written for the agent.

The prompt is recorded on the `audits` row at emit and read back by
`writ audit --fetch ID`, or by the `writ_audit` tool's `fetch` argument,
which is the path the pointer names first and the one the transcript
collapses. Both are named because writ cannot see whether the host
serves MCP, and a pointer whose only path is a tool the host does not
serve is a gate that blocks forever.

**A fetch re-reads. It never re-selects.** Re-selecting would open a
second `audits` row and move `times_selected` again for one gate, which
is exactly the conflation the two counter pairs exist to prevent.

`audits.prompt` is `NOT NULL`. There is no audit without a prompt, so no
read path has to decide what a missing one means. Schema 3 rebuilt the
table and dropped the rows that predate the column rather than
backfilling them with `''`, which `--fetch` would have handed an agent
as a document. Their findings cascaded with them.

The cost of recording the prompt is that the diff is now in the database
as well as in the turn. On a long branch that is real growth, and P4
means it is never reclaimed by pruning. `render_pointer` and its golden
file pin the emitted text, beside `render_prompt` and its own.

**Ingest only happens if the prompt asks for it.** The hook process
exits when it has printed. A reply the agent leaves in the conversation
reaches nothing, so the prompt has to name a return path — the
`writ_audit` tool, or a pipe to `writ audit --ingest` — and not merely a
JSON shape. When it does not, `times_applied` never moves, the ranking
has no acceptance rate, `blocking` has no effect at all, and recurrence
has no data. That shipped once. `render_prompt` and its golden file are
where it is now pinned.

The retry cap is the host's own signal, never a count writ keeps. writ
sees each audit fresh and cannot tell one turn from the next, so an
agent that reports `ignored` would loop forever. Claude Code and Codex
send `stop_hook_active`, and Cursor sends `loop_count`.

Exit codes under `--hook` are the host's protocol, not the table in spec
section 5.7. `--hook claude-code` exits 2 to block, which that table
assigns to a usage error. The contradiction is deliberate and confined
to `--hook`, which exists to speak the host's language.

## Commands

The toolchain is managed by mise. Always go through it.

```
mise exec -- cargo test --workspace
mise run check      # fmt, clippy -D warnings, test
mise run docs       # every documented flag against `writ --help`
mise run lint       # cargo-deny, machete, typos, shellcheck
mise run coverage   # llvm-cov LCOV for Codecov
mise run build
```

## Worktrees

Feature work and parallel agent sessions use [Worktrunk](https://worktrunk.dev)
(`wt`), not ad-hoc `git worktree add`. A worktree is still the same project as
the main checkout (invariant 4).

```
wt switch --create <name>   # create branch + worktree and switch into it
wt switch <name>            # switch to an existing worktree
wt list                     # status of all worktrees
wt merge                    # merge this branch into the default target
wt remove                   # remove the worktree; delete the branch if merged
```

## Layout

```
crates/writ-core/   storage, selection, budget.
                    Knows nothing about a terminal, socket, or HTTP.
crates/writ-cli/    the `writ` binary. The only crate that does I/O.
plugins/claude-code/  the /record skill and the gate hooks. It carries
                    its own copy of what `writ install` writes, so a
                    test pins the two together. P8.
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
5. **Bound the prompt, and do not claim to bound the cost.** Selection
   caps at `max_rules` and `max_chars`, so a 20-fold larger collection
   sends the same 40 rules. `glob:` scopes are now evaluated in SQL via
   the `writ_glob_any` scalar function, which uses the exact crate glob
   dialect (including `**`). That means narrowing a rule with `glob:`
   narrows the set of learnings the SQL filter returns and the batched
   scope/outcome queries load. The initial select still evaluates every
   active learning's scopes, so a diff that matches a large subtree or a
   collection with many `global` rules can still read many rows.
   Selection cost is therefore not bounded by the diff alone; it is
   bounded by how many active learnings survive the filter. If you
   improve that, say so. Do not write a test that implies it is already
   true. The **diff** in a prompt is not bounded at all: see *What the
   gate audits*.
6. **Fail honestly.** Every error names its real cause and exits with
   its own code. Never hang on stdin. See spec P7.
7. **Never set `updated_at` by hand in a write.** A database trigger
   maintains it. The trigger guards on
   `WHEN new.updated_at = old.updated_at`, so an UPDATE that sets the
   column explicitly silently bypasses it. Setting it deliberately is
   reserved for import, where a learning keeps the timestamps it
   arrived with.
8. **Docs, skills, and the CLI agree.** A flag in the README or a
   SKILL.md that the binary rejects is a bug. See spec P8.

## Working style

- Commits and PR titles follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
  (`feat:`, `fix:`, `chore:`, `ci:`, …).
- TDD. Write the failing test first.
- Exit codes are a contract. Test every one.
- `writ audit --format prompt` output is asserted byte for byte.
- A test for a hang must be bounded. A test that hangs on regression
  hangs CI instead of failing it.
