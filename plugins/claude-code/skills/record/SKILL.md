---
name: record
description: Record a piece of steering as a writ learning, so the same correction does not have to be given again. Use when the user corrects an approach, states a convention, rejects a pattern, or says "remember this", "always do X", "never do Y", or "next time".
---

# Record a learning

`writ` is a local ledger of the steering a developer gives coding agents.
One correction goes in once. Every later audit puts it back in front of
whoever is about to repeat it.

Record a learning when the user corrects an approach, states a
convention, or rejects a pattern. Do not record a one-off instruction
about the current task. A learning must be true again next week.

## Write it

```
writ record --title TITLE --rule RULE --rationale WHY --activate
```

| Flag | What goes in it |
| --- | --- |
| `--title` | A short name. A reviewer reads this first |
| `--rule` | What to do, as an instruction |
| `--rationale` | Why. Required, and never empty |
| `--scope` | Where it applies. Repeat it for more than one |
| `--advisory` | Report it, but never block the handoff |
| `--example-text` | A snippet, as `good:TEXT` or `bad:TEXT`. Repeat it |
| `--matcher` | A structural retrieval pattern. Default on for code-shape rules |
| `--matcher-kind` | `ast_grep` (preferred with `language:`) or `regex` (escape hatch) |
| `--sides` | Which half of the diff to care about: `added`, `removed`, or `both` (default) |
| `--activate` | Use the learning from now on |

Without `--activate` the learning lands in the Inbox as `proposed` and
no audit will select it. That is the safe default, not a bug. Use it
when you are unsure the rule is right, and tell the user it is waiting
for review.

## Scope it

`--scope` takes `KIND:VALUE`, and `global` on its own.

- `global` — every repository.
- `project:ID` — this repository, by its normalized remote.
- `language:rust` — every file of that language.
- `glob:crates/*/src/**` — the paths that match.

No `--scope` means the learning is global. Prefer the narrowest scope
that is true: a rule about this repository's test layout is not a rule
about Rust.

## Say why, properly

The rationale is the field that decides whether the rule survives its
first argument. "It is cleaner" does not. Name the failure the rule
prevents.

Good: `sed's -i flag takes an argument on BSD and not on GNU, so the
same script deletes a file on one machine and edits it on the other.`

Bad: `sd is better.`

## Show it, do not only say it

A rule with a snippet teaches. A rule without one asserts. Pass the pair
whenever the correction came from real code: the line as it was, and the
line as it should be.

```
writ record --title T --rule R --rationale WHY --activate \
  --example-text "bad:sed -i '' s/a/b/ f" \
  --example-text "good:sd a b f"
```

writ stores the text and never the path, so a snippet keeps teaching
after the file moves or the branch goes.

## Match structural rules

A matcher lets an audit find the code a rule is about. Add one whenever
the rule describes code shape; omit it only for process or style rules
that have no structural anchor.

- Prefer `ast_grep` when the scope includes `language:`.
- Use `regex` only when `ast_grep` cannot express it.
- Matchers honour `--sides`. Default `both` sees additions and removals
  (`regex` on both line sets; `ast_grep` on the post-change file and its
  pre-image). Use `--sides added` for "do not introduce X" and
  `--sides removed` for "do not delete Y". For "do not remove X" rules,
  match X itself — e.g. Elixir `@spec` for "Do not bulk-delete
  typespecs". A broader durable cue (`def` / `defp`) is fine when you
  want the rule in play on nearby edits too.

## After writing

Say the id back to the user, and say whether it is active or waiting in
the Inbox. Nothing detects duplicates, so when the user says the lesson
is already recorded, use `writ record --reinforce ID` with the id they
name instead of writing a second row that says the same thing.

## Fix a learning in place

When the user corrects a rule that is already recorded — a better
rationale, a narrower scope, a sharper matcher, a missing exemplar —
edit it instead of writing a second one.

```
writ edit ID --rationale "..."
writ edit ID --scope language:rust --scope glob:crates/**
writ edit ID --matcher PATTERN --matcher-kind ast_grep --activate
writ edit ID --sides added
```

Omitted fields stay unchanged. `--scope` and `--example-text` replace
their full sets. `--activate` moves a `proposed` learning to `active`;
it is rejected for archived learnings. The same edit is available as
the `writ_edit` MCP tool.
