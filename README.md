# writ

A local-first ledger of the steering you give coding agents.

Say a thing once. writ keeps it, and puts it back in front of whoever is
about to repeat the mistake. Nothing leaves the machine.

## Install

```
cargo install --path crates/writ-cli
```

That installs a binary named `writ`.

## Use

```
writ record --title T --rule R --rationale WHY --activate
writ audit
writ list
writ ui
```

- **`writ record`** writes a learning. It is the only way into the
  database. A write with no `--activate` waits in the Inbox as
  `proposed`, so a forgotten flag fails safe.
- **`writ audit`** reads the diff you have not committed, selects the
  learnings that apply to it, and prints them for a reviewing agent.
- **`writ list`** reads the collection back. `--unused-days` and
  `--never-applied` find the rules that are not earning their place.
- **`writ ui`** opens the four screens: Inbox, Collection, Detail and
  Health.

An audit costs what the diff costs, not what the collection costs. It
sends at most `max_rules` learnings and `max_chars` of rule text,
whether you have fifty learnings or a thousand.

## Agents

`writ mcp` serves two tools, `writ_record` and `writ_audit`, on stdio.
Both are shells over the same code the commands above run.

`writ audit --hook HOST` emits the audit in a host's gate protocol, so
the review is not optional. See
[`plugins/claude-code/README.md`](plugins/claude-code/README.md) for the
Claude Code plugin, the hosts that can enforce a gate, and the one that
cannot.

## Where things live

`$XDG_DATA_HOME/writ/learnings.db` and
`$XDG_CONFIG_HOME/writ/config.toml`. Both take a file path, and `--db`
and `--config` override them on every command.
