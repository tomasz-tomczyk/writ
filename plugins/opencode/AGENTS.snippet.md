<!-- writ:start -->
## writ

`writ` is a local ledger of the steering this developer gives coding
agents. One correction goes in once, and every later audit puts it back
in front of whoever is about to repeat it. It is reachable over MCP as
two tools: `writ_record` and `writ_audit`.

**Record a learning when the user corrects you on something that would
apply again.** A convention, a rejected pattern, a "never do X", an
"always do Y next time". Call `writ_record` with a short title, the
rule as an instruction, and the reason the rule exists. Name the failure
the rule prevents: a rationale that says "it is cleaner" does not
survive its first argument. Say the id back to the user afterwards.

Do not record an instruction about only the task in hand. A learning
must still be true next week. When the user says the lesson is already
recorded, reinforce the id they name instead of writing a second row
that says the same thing.

**OpenCode cannot enforce a gate**, so nothing runs the audit for you.
Call `writ_audit` yourself before you hand the turn back after writing
code, and fix what it reports.
<!-- writ:end -->
