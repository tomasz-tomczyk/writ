<!-- writ:start -->
## writ

`writ` is a local ledger of the steering this developer gives coding
agents. One correction goes in once, and every later audit puts it back
in front of whoever is about to repeat it. It is reachable over MCP as
three tools: `writ_record`, `writ_audit`, and `writ_edit`.

**Record a learning when the user corrects you on something that would
apply again.** A convention, a rejected pattern, a "never do X", an
"always do Y next time". Call `writ_record` with a short title, the
rule as an instruction, and the reason the rule exists. Name the failure
the rule prevents: a rationale that says "it is cleaner" does not
survive its first argument. For structural rules, also pass `matcher`
and `matcher_kind`: prefer `ast_grep` when the scope includes a
language, and use `regex` only as an escape hatch. Pass `sides` as
`added`, `removed`, or `both` (the default) when the rule should only
fire on one half of the diff. Say the id back to
the user afterwards.

`scope` is required. Use the narrowest one that is true: `glob:PAT`
for the paths the correction was about, `language:LANG`, or
`project:ID` with the repository's remote in lower case, such as
`github.com/owner/repo`. Avoid `global`: it puts the rule in front of
every change in every repository, and a blocking one can refuse every
commit. Use it only for a rule that holds for any code and is costly to
break, and ask the user first.

Do not record an instruction about only the task in hand. A learning
must still be true next week. The audit sees only the code diff, so do
not record a rule about PR descriptions, commit messages, or how to
work with the user: suggest `CLAUDE.md` or `AGENTS.md` for those. When the user says the lesson is already
recorded, reinforce the id they name instead of writing a second row
that says the same thing.

A `Stop` hook already runs the audit for you, so you do not have to
call `writ_audit` by hand.
<!-- writ:end -->
