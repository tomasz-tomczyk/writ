---
name: host-parity
description: "Check that writ's Claude Code, Codex, Cursor, and OpenCode snippets, skills, hooks, MCP configuration, install behavior, and documentation stay semantically aligned. Use after host-integration or CLI-surface changes, or when asked to audit host parity."
disable-model-invocation: true
---

# Host integration parity

Audit the integration artifacts shipped in `plugins/`. This is semantic parity, not byte identity: each host has a different configuration and hook protocol.

## Ground truth

Read these before comparing:

- `AGENTS.md` especially host and gate sections of `AGENTS.md`;
- `plugins/README.md`;
- every host-specific README, instruction snippet, manifest, hook file, MCP file, and shipped skill under `plugins/`;
- `crates/writ-cli/src/install.rs`, `crates/writ-cli/src/hook.rs`, MCP command definitions, and their tests when the relevant behavior changed.

Stop if the spec is missing. If the spec and a primary host contract appear to conflict, report that conflict rather than choosing a new product design.

## Capability matrix

Build and verify a matrix for Claude Code, Codex, Cursor, and OpenCode covering:

- plugin manifest discovery and version;
- home and project MCP configuration shape;
- instruction-file snippet and capture guidance;
- shipped `record` skill or the documented canonical reuse path;
- stop-hook availability, event name, command, payload, output protocol, and retry signal;
- `writ install` target paths, merge behavior, backup behavior, dry rendering, force behavior, and user-facing caveats.

The expected asymmetries are part of correctness:

- Claude Code gates with a `Stop` hook and blocking exit/status behavior.
- Codex gates with a `Stop` hook and its JSON decision protocol; bundled hooks require the documented trust step.
- Cursor uses its `stop` hook and follow-up-message protocol with a loop count.
- OpenCode has no enforceable stop/idle gate. Its docs and output must say so; never manufacture false parity by claiming otherwise.
- A skill may have one canonical source and be reused rather than copied. Lack of duplicate bytes is not drift when the documented loader path works.

## Compare behavior

Trace each capability end to end:

1. The manifest points at files that exist and uses the host's actual schema.
2. The static plugin artifact and `writ install <host>` describe equivalent MCP and hook behavior.
3. Install merges without replacing unrelated user configuration, is idempotent, and backs up an existing file before mutation.
4. Snippets teach the same capture semantics: corrections that transfer become proposed or explicitly activated through the documented path, with a real rationale and scope.
5. Hook entry gates on any selected learning; ingest gates only on unresolved blocking findings. Every prompt names an actual findings return path.
6. Retry signals and empty-diff/non-git pass-through behavior match the host.
7. CLI flags, MCP schemas, examples, READMEs, tests, and plugin versions agree.
8. No artifact contains stale absolute checkout paths or references to unrelated products, Linear, Sentry, or dual-repo workflows.

Classify differences as `IN SYNC`, `INTENTIONAL HOST DIFFERENCE`, or `DRIFT`. For drift, state which artifact is authoritative and the minimal files to change. Do not edit in audit-only use.

## Verification

Run focused install, hook, and MCP tests appropriate to the diff, followed by:

```bash
mise run docs
mise run check
```

Run `mise run lint` as well when scripts, manifests, dependencies, or CI changed. An unavailable optional host executable is a reported limitation, not a reason to skip repository-level fixture and schema tests.

## Report

Return the capability matrix, drift items, intentional differences, commands run, and `PASS` or `FIX BEFORE LANDING`. Any falsely advertised gate, missing return path, destructive install behavior, stale CLI flag, or version mismatch is a blocker.

## Retrospective

If the parity model needed correction or a host added a new capability, propose the exact update to this canonical skill and apply it only after user approval. Otherwise close with one line saying the workflow had no friction.
