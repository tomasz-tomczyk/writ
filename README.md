# writ

A local-first ledger of the steering you give coding agents.

You correct an agent, the correction works, and the session ends. Next
week you type it again. writ keeps the correction, and puts it back in
front of whoever is about to repeat the mistake. Nothing leaves the
machine, and writ never calls a model — your agent does the reasoning.

## Install

### Homebrew

```
brew install tomasz-tomczyk/tap/writ
```

### Nix

```
nix profile install github:tomasz-tomczyk/writ
nix run github:tomasz-tomczyk/writ
```

### GitHub Releases

Prebuilt binaries are available on the [releases page](https://github.com/tomasz-tomczyk/writ/releases).

### cargo

```
cargo install writ-cli
```

That installs a binary named `writ`. On crates.io the package is
`writ-cli`, because `writ` belongs to an unrelated markdown editor.

From a checkout:

```
cargo install --path crates/writ-cli
```

## The loop

```
writ record --title T --rule R --rationale WHY --scope language:rust --activate
writ audit
writ list --never-applied
writ edit ID --matcher PATTERN --matcher-kind ast_grep
writ edit ID --sides added
writ archive ID
writ ui
```

1. **`writ record`** writes a learning. It is the only way into the
   database. A write with no `--activate` waits in the Inbox as
   `proposed`, so a forgotten flag fails safe.
2. **`writ audit`** reads the diff you have not committed, selects the
   learnings that apply to it, and prints them for a reviewing agent.
3. **`writ list`** reads the collection back. `--unused-days` and
   `--never-applied` find the rules that are not earning their place.
4. **`writ edit ID`** mutates an existing learning. Omitted fields stay
   unchanged; `--scope` and `--example-text` replace their full sets;
   `--activate` moves a `proposed` learning to `active`.
5. **`writ archive`** prunes one. It stops being selected and stays in
   the database.
6. **`writ ui`** opens four screens: Inbox, Collection, Detail, Health.

An audit costs what the diff costs, not what the collection costs. It
sends at most `max_rules` learnings and `max_chars` of rule text,
whether you hold fifty learnings or a thousand.

## Agents

`writ mcp` serves three tools, `writ_record`, `writ_audit`, and
`writ_edit`, on stdio. All are shells over the same code the commands
above run. That makes writ reachable. It does not make the review
happen.

`writ audit --hook HOST` is what makes the review not optional. It emits
the same verdict in each host's own gate protocol, so a turn that
touched code the learnings cover does not hand over unreviewed.

| Host | Gate | Subagents |
| --- | --- | --- |
| Claude Code | `Stop` hook | `SubagentStop` hook |
| Codex CLI | `Stop` hook | no such event |
| Cursor | `stop` hook | no such event |
| OpenCode | **none — it cannot enforce one** | — |

The turn's gate audits everything the branch changed, resolving a
`merge-base` against the remote's default branch first. Left at the
`--diff` default — the working tree against HEAD — it would see an
empty diff and pass for any agent that commits as it goes. The subagent
gate keeps that default on purpose: a subagent has not committed, so the
working tree is exactly its own work.

`writ install <host>` writes all of this. It merges rather than
replaces, backs up first, and `--print` shows the entry it would add
rather than reprinting your configuration file.

**OpenCode cannot enforce a gate.** Every one of its plugin hooks
returns `Promise<void>`, so nothing there can block a turn or inject a
prompt. Its `AGENTS.md` can ask the agent to run the audit, and that is
a request, not a gate. `writ audit --hook opencode` is rejected rather
than accepted and quietly ignored.

See [`plugins/claude-code/README.md`](plugins/claude-code/README.md) for
the Claude Code plugin and the exact protocol each host receives.

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

## Design

[`AGENTS.md`](AGENTS.md) holds the principles, the data model, and the
invariants. Read it before changing behavior. `CLAUDE.md` is a symlink
to it, because Claude Code reads that name and the other three hosts
read `AGENTS.md`.
