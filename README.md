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

## Local telemetry

Telemetry is opt-in, aggregate-only, and local. It is disabled by default and
no first-run prompt enables it. Its counters live in the physically separate
`$XDG_DATA_HOME/writ/telemetry.db`; the telemetry store is never joined to the
learnings store. A `--db PATH` override changes only the learnings database;
telemetry remains under the XDG data home.

There is no upload command, endpoint, or HTTP client in the writ binary. A JSON
dump is a file the user may choose to inspect or share themselves.

```console
writ telemetry            # same as show
writ telemetry on         # show the disclosure, confirm, and opt in
writ telemetry on --yes   # explicit non-interactive opt in
writ telemetry off        # stop collection and retain existing aggregates
writ telemetry show       # enabled state, disclosure, and every held row
writ telemetry dump       # complete versioned JSON payload on stdout
writ telemetry purge      # delete telemetry.db entirely
```

Only `writ telemetry on` writes `enabled = true`. To opt out in configuration:

```toml
[telemetry]
enabled = false
```

Telemetry holds daily counter rows for command, surface, exit code, record
source/status, scope kind, allowlisted language, matcher kind/result, hook host,
gate result, finding outcome, and Health action. It holds only
named distribution buckets (`0`, `1-5`, `6-20`, `21-50`, `51+`) for collection
size, rules considered/sent, findings, diff files, prompt characters, and
command duration. Raw distribution values and event rows are never stored.

The complete privacy disclosure follows verbatim.

**Captured:**

- A random install id, generated when telemetry is enabled.
- The writ version and the operating system family (`macos`, `linux`,
  `windows`).
- The date, to the day.
- The counters and buckets in section 4.

**Never captured:**

- Rule text, titles, rationale, exemplar snippets, notes.
- File paths, directory names, glob patterns, repository names, remote
  URLs, branch names.
- Scope *values*. Only the scope kind, and language from a fixed
  allowlist.
- Search queries, matcher patterns, finding details.
- The author field, any email, any username, any hostname.
- Learning ids, audit ids, finding ids.
- Timestamps finer than one day.
- Anything from the diff being audited.

If a future metric cannot be added without touching that second list, it
does not get added.

Telemetry writes are best-effort. An unwritable or corrupt `telemetry.db`
cannot change another command's output, selection, audit result, or exit code.
The explicit `telemetry show`, `dump`, `on`, and `purge` administration commands
still report failures because the telemetry store is their requested result.

The dump has format version `"writ_telemetry": 1` and includes all fixed bucket
edges, so it remains self-describing if a later release changes the format.
