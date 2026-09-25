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
| git `pre-commit` | `--cached`: the index against HEAD | It is the change being committed |
| `Stop` | `--since-answer`: from the last answered commit, else the branch point | The agent may have committed |
| `SubagentStop` | the default: working tree against HEAD | A subagent has not committed, so the tree is its own work |

Do not give the subagent hook the branch point. It would hand a
read-only subagent every violation its parent had already committed.

Only Claude Code has `SubagentStop`; Codex and Cursor have no such
event. Its payload carries `agent_type`, and using it to skip read-only
agents is tempting and wrong — a hand-maintained list of agent names
drifts, which is P8.

The prompt does not carry the diff: see *The prompt points at the
diff*. The diff the agent is asked to read is still unbounded. A long
branch is a long read, paid only when the agent reads it.

## The commit gate

**Audit the change while it is small.** Stop audits after the agent has
committed, which is why it needed a range at all, and a branch-point
range drags in the whole branch and, on a stacked branch, the parent's
work too. The commit gate audits each commit as it is made.

- **It lives in git config, not in `.git/hooks`.** `hook.writ.command`
  and `hook.writ.event = pre-commit`, set by `writ install` in the global
  config, or in the repository's own config for a project setup.
  That needs git 2.54, which is the first git with hooks in config. It
  runs alongside a repository's own hook file, Husky's `core.hooksPath`
  included, and one repository can opt out with `hook.writ.enabled
  false`. Never write into `.git/hooks`: that file is the repository's.
- **It gates agents only.** `--hook git` passes silently unless
  `CLAUDECODE`, `CURSOR_AGENT` or `GEMINI_CLI` is set. Codex documents
  no such variable, so a Codex commit passes here and is audited at the
  next Stop instead.
- **Exit 1 refuses the commit**, with the pointer on stderr, which is
  where an agent reads a failed commit. It reads no stdin and keeps no
  retry cap: the agent retries by committing again, and a retry of an
  answered change is covered.
- **It records the index tree** (`audits.tree`, schema 8). The commit
  it lets through has that tree, which is how Stop knows the commit was
  reviewed.

## An audit records what it reviewed

A working-tree audit reads a **snapshot**: `git add -A` into a copy of
the index, then `git write-tree`. The real index is never touched. The
diff is `git diff BASE TREE`, which for tracked files is the same text as
`git diff BASE` and also carries new files git does not ignore — before
this, a file the agent created and never staged was invisible to Stop.
The tree goes on the `audits` row, and the prompt's commands name it, so
they print exactly what was audited. A staged audit's tree is the index.

Because both gates now record the tree they reviewed, **"Changed since
your last answer" works at every gate**, and exactly: it diffs the last
answer's tree against this one. The last answer is the newest answered
audit whose head is in HEAD's history and whose start is at or before
this audit's start. The second condition keeps a subagent's audit, which
started at its own head, from vouching for commits before it.

One answer counts at both gates: `git diff --cached` after `git add -A`
is byte for byte `git diff HEAD TREE`, so the slice digests match.

## The Stop gate starts at the last answer

`--since-answer` walks HEAD's first-parent history for the newest commit
an answered audit reviewed up to, and audits from there. With none, it
falls back to the branch point against the remote's default branch, and
with no branch point to the working tree against HEAD.

| Answered audit | Counts for |
| --- | --- |
| staged (`--cached`) | the commit whose tree it recorded |
| over a range | the head it ran at, and a commit with its snapshot tree |
| working tree against HEAD | **nothing** |

The last row is deliberate. A subagent's audit saw only uncommitted
work; counting its head would mark every commit before it reviewed.

A stacked branch needs no configuration: its parent's answered commits
are in its history, so it starts after them. Everything before the
answer point is taken as reviewed, so a commit made with `--no-verify`
before an answered audit is not audited again.

## The gate points, it does not paste

What `--hook` emits is a **pointer** — the audit id and the two ways to
fetch what is behind it — never the prompt. Every host renders a blocked
turn's message into the transcript verbatim, so pasting the prompt puts
a document written for the agent in front of the developer after every
turn.

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

The prompt is stored and P4 means pruning never reclaims it. It holds
rule text and at most five hits per rule, not the diff, so a row does
not grow with the branch. `render_pointer` and its golden file pin the
emitted text, beside `render_prompt` and its own.

## The prompt points at the diff, it does not carry it

Measured on 2026-09-25: a mean prompt of 72 KB over 410 audits, and in
one ordinary audit the diff was 31.4 KB of a 36.2 KB prompt. The agent
that wrote the change can read it with git. Spec section 9.2.

Each selected learning carries `start here` — the changed lines its
matcher found, as `path:line` and text, at most five, then a count — and
`slice`, the `git diff RANGE -- PATHS` command that prints the part of
the diff it applies to. The range is the audit's own `diff_range`, so
the command prints exactly what writ hashed.

- **A hit is shown only on a changed line.** `ast-grep` scans whole
  files, so without the filter `start here` points at old code. A
  pre-image hit is shown at its old line, marked `removed`.
- **Selection is unchanged.** A match on an unchanged line still
  selects, as section 8.2 always said. The filter decides only what is
  shown. A learning selected that way says its matcher matched only
  unchanged lines.
- **A hit is where to look, not a finding.** The prompt says so: a
  matcher can match code that obeys the rule and miss a violation
  written another way.
- **Hits are not rule text.** They render after `rule_block` and are
  bounded by `MAX_HITS` per learning, not by `max_chars`.

The risk is rubber-stamping: an agent that does not run the command
sees no code. Watch `times_applied` against `times_selected`. If it
falls, paste the diff again when it is small.

## Changed since your last answer

A gate that fires again on one range is usually firing on the agent's
own fix. `audits.head` (schema 7) records `git rev-parse HEAD` at emit.
When an earlier audit of the same repo and `diff_range` was ingested
against and has a head, the prompt names it, the paths of this diff that
changed since (`git diff --name-only HEAD_THEN`), the command that
prints what changed, and marks `new` each selected learning that audit
did not carry, read from its `audit_coverage` rows.

The path list is a superset: uncommitted work already in the answered
audit is listed again. Listing too much costs a line; listing too little
would hide new work. When git cannot compare against the old head, the
section is left out. It changes no gate decision.

**Ingest only happens if the prompt asks for it.** The hook process
exits when it has printed. A reply the agent leaves in the conversation
reaches nothing, so the prompt has to name a return path — the
`writ_audit` tool, or a pipe to `writ audit --ingest` — and not merely a
JSON shape. When it does not, `times_applied` never moves, the ranking
has no acceptance rate, `blocking` has no effect at all, and recurrence
has no data. That shipped once. `render_prompt` and its golden file are
where it is now pinned.

The retry cap is the host's own signal, never a count writ keeps. writ
cannot tell one turn from the next, so an agent that reports `ignored`
would loop forever. Claude Code and Codex send `stop_hook_active`, and
Cursor sends `loop_count`.

**Check the cap before selecting, not after.** It used to sit after
`audit::select`, so a turn the cap was about to let stop still rendered
the prompt, opened an `audits` row, and moved `times_selected` for every
rule it then discarded — rules credited with an appearance no reviewer
saw. Nothing forces the old order: the payload is already read.

## The gate does not re-nag a diff it already covered

**The cap bounds one stop-cycle, and nothing bounds the next.**
`stop_hook_active` is set only while the host is continuing a turn it
already continued, and the next user message clears it. So an unchanged
diff that was audited and answered used to be selected again on the very
next turn, and again after that. Measured: one unchanged `diff_range`
blocked at 12:03:18, passed capped at 12:04:03, and would have blocked
again on any following turn.

`audits.diff_digest` (schema 4) is a hash of the **diff text**, and
`audits.ingested_at` is stamped when findings come back. A gate passes
when an earlier audit in the same repo carries the same digest *and* was
ingested against. Both halves matter:

- the digest alone would let any audit disarm the gate, rewarding the
  agent that ignored the pointer with silence;
- `findings` cannot stand in for `ingested_at`, because it is a count
  and an honest clean report ingests zero. Reading `findings = 0` as
  "never answered" re-nags precisely the agent that did the work.

Digest the diff, never the prompt. The prompt carries the rendered
rules, so a `max_chars` change would re-nag a diff nobody touched.

The check is for **gates**, not every caller: `writ audit` by hand still
audits. The ingest moment is untouched — a `blocking` finding still
`open` still refuses the handoff.

## The digest matches the granularity of selection

**"Touch one byte and the digest changes" was the wrong granularity.**
Schema 4 keyed coverage on the whole diff, while selection matches per
learning, per path. Those are the same thing only when every active
learning is `global`. They come apart the moment a rule carries a
`glob:` scope and the gate carries a `merge-base` range, which is the
combination *What the gate audits* mandates.

Measured: one branch, 75 minutes, eleven selections of a single rule
scoped `glob:.github/workflows/**` — the first reporting three
violations and each of the ten after it ingesting clean. Eleven distinct
digests, so `covered` never matched once. Every later turn edited a
source file the rule does not scope, and the whole-diff digest moved
with it while the rule's own concern had not. The workflow file was
committed early, so it sat inside the `merge-base` range for the life of
the branch: the scope kept matching, the digest kept changing, and the
rule could never settle.

`audit_coverage` (schema 5) holds one row per learning per audit, keyed
on `slice_digest` — a hash of the hunks of the paths *that learning's*
`glob:` and `language:` scopes selected, concatenated in sorted-path
order so the same change hashes the same however git laid it out. A
`global` scope's slice is the whole diff, which is deliberate rather
than a special case: a global rule does care about every byte.

A gate passes when **every** selected learning is covered — a row with
the same repo, learning and slice digest whose parent audit has
`ingested_at` set. Both halves of the schema-4 condition survive: a
learning nobody answered has no covered row and still fires. One
uncovered learning blocks, and the prompt then carries every selected
learning rather than only the uncovered ones, because a reviewer handed
three rules and told to re-check one has to re-read the other two to
know they still hold.

`audits.diff_digest` stays. It is evidence of what an audit looked at
(P4), and it is still the key for the all-`global` case.

A rule activated after the fact has no coverage row, so unlike schema 4
this *does* notice it, and fires. That is the correct reading and not a
regression: the rule has never been put in front of anyone.

**Check coverage after selecting, and move no counters when it
suppresses.** It cannot run before: the slices are not known until the
learnings are. So the order is cap, select, rank, budget, coverage,
emit. A gate suppressed at the coverage step opens no `audits` row and
leaves `times_selected` alone — crediting a rule that was selected and
then suppressed records an appearance no reviewer saw, which is the same
defect the retry cap had before it was hoisted above `select`.

This buys nothing in selection cost, and no test may imply it does
(invariant 5). Selection still runs in full. What is saved is the prompt
render, the nag, and the counter.

## Two surfaces, one gate

`writ install` and the `writ@writ` plugin are two delivery paths for one
fact, and **exactly one may be live**. A test pins their text identical
(P8), which is what made carrying both invisible: two identical gates
fire per turn, the pointer lands in the transcript twice, and
`times_selected` moves twice for one gate. `writ install claude-code`
reads `enabledPlugins` in the settings file it is about to edit and
writes nothing when the plugin is enabled. `--force` overrides.

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
   true. The prompt carries no diff; the diff the agent reads is not
   bounded at all: see *The prompt points at the diff*.
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
