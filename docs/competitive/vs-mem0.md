# mem0 — competitive brief

Objective notes on [mem0](https://mem0.ai/) (also `mem0ai` on GitHub/PyPI), an AI
memory layer for agents and applications. Compared against writ, a local-first
ledger of steering/corrections for coding agents.

## What it is

mem0 adds persistent, retrieval-based memory to AI agents and apps. It
intercepts messages, extracts facts with an LLM, embeds them, and stores them in
a vector database. On the next turn or session it retrieves the most relevant
memories and injects them into context. It is positioned as drop-in
infrastructure: a few lines of SDK code or an MCP server, rather than a custom
memory pipeline.

Primary form factors:

- **Open-source library/server** (`pip install mem0ai`, Apache 2.0) — library
  for prototyping, or a Docker Compose self-hosted server.
- **Managed cloud platform** (`app.mem0.ai`) — hosted API, dashboard,
  analytics, enterprise features.
- **OpenMemory MCP** — local-first MCP-compatible memory server for Claude
  Desktop, Cursor, Windsurf, VS Code, and other MCP clients.

Sources: [mem0.ai homepage](https://mem0.ai/),
[GitHub README](https://github.com/mem0ai/mem0),
[docs introduction](https://docs.mem0.ai/introduction),
[pricing](https://mem0.ai/pricing).

## Goals

- Let agents remember user preferences, account state, and conversation history
  across sessions without replaying full transcripts.
- Reduce token usage by retrieving only relevant extracted memories instead of
  keeping entire message history in context.
- Provide a portable memory layer that works across agent frameworks, vector
  stores, and hosting models (library, self-hosted, cloud, local MCP).

## Memory / steering model

mem0 stores **extracted facts**, not raw transcripts. The current algorithm
(April 2026) uses:

- **Single-pass ADD-only extraction** — one LLM call per `add()`; new memories
  are appended, not overwritten or deleted. When facts change, both old and new
  versions can coexist; retrieval ranking decides which surfaces.
- **Multi-signal retrieval** — semantic similarity, BM25 keyword matching, and
  entity linking are scored in parallel and fused into one ranking.
- **Temporal reasoning / memory decay** — recency is applied as a search-time
  soft re-rank factor (0.3×–1.5×), not a hard delete.
- **Multi-level storage** — conversation, session, user, and organizational
  memory layers with different lifetimes.

Memory is scoped by `user_id`, `run_id` (session), `agent_id`, and `org_id`.
This is multi-tenant scoping, not the same as writ's rule-level applicability
scopes.

mem0 **requires an LLM to function** (default `gpt-5-mini`) and an embedding
model (default `text-embedding-3-small`). writ does not call a model; the host
agent reasons over selected rules.

Sources:
[memory types docs](https://docs.mem0.ai/core-concepts/memory-types),
[blog on AI memory management](https://mem0.ai/blog/ai-memory-management-for-llms-and-agents),
[State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026),
[benchmark comparison blog](https://mem0.ai/blog/benchmarked-openai-memory-vs-langmem-vs-memgpt-vs-mem0-for-long-term-memory-here-s-how-they-stacked-up).

## Local vs cloud

mem0 spans both, with a clear tier split:

| Form factor | Data location | Best for |
|---|---|---|
| Library | Local process | Prototyping, testing |
| Self-hosted server | Own infrastructure / Docker | Teams that need control |
| OpenMemory MCP | Local machine with dashboard | Individual devs across AI tools |
| Managed platform | mem0 cloud | Zero-ops production |

Enterprise offers on-prem / air-gapped deployment, SSO, audit logs, HIPAA BAA,
and SLA. SOC 2 Type I is reported; SOC 2 Type II is in progress per third-party
summary. Data residency and compliance details are available on request.

writ, by contrast, is local-only: a single SQLite database under XDG paths, no
server, no account, no telemetry.

Sources:
[GitHub README hosting table](https://github.com/mem0ai/mem0),
[pricing page](https://mem0.ai/pricing),
[theaiagentindex.com profile](https://theaiagentindex.com/agents/mem0).

## Agent integration

mem0 integrates broadly:

- **SDKs** for Python and Node.js; language-agnostic REST API.
- **Agent frameworks** — LangChain, LangGraph, CrewAI, Flowise, Langflow,
  Mastra, Vercel AI SDK, LlamaIndex, AWS Agent SDK, and others.
- **Coding agents** — Claude Code, Cursor, Codex, Windsurf, OpenCode via skills
  / MCP.
- **MCP** — official OpenMemory MCP server exposes `Mem0-memorize` and
  `Mem0-remember` style tools.
- **Vector stores** — 19–20 backends including Qdrant, Chroma, Weaviate,
  PGVector, Pinecone, Azure AI Search, etc.

writ's integration surface is narrower by design: MCP server + `writ install`
for Claude Code, Codex, Cursor, and OpenCode, plus the `writ audit` CLI for the
terminal.

Sources:
[docs introduction](https://docs.mem0.ai/introduction),
[GitHub README integrations list](https://github.com/mem0ai/mem0),
[State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026).

## Curation / prune / audit

**Curation.** mem0 provides a dashboard for browsing and managing stored
memories. OpenMemory MCP has a local dashboard. Self-hosted and cloud also offer
analytics. There is no explicit human-approval gate analogous to writ's
`proposed → active` workflow; memories become retrievable once added.

**Pruning.** ADD-only extraction means memories accumulate by default. Memory
Decay soft-ranks stale items down but does not delete them. Whether there is a
first-class archive/retire operation (equivalent to `writ archive`) is not
documented in the public pages reviewed; the dashboard likely supports delete,
but that is destructive, not archival. **Marked as unknown.**

**Audit.** mem0 logs reads/writes in enterprise deployments. It does not appear
to run a diff-time review or emit a prompt asking the host agent to report
violations. It is a retrieval layer, not a gate on code handoff.

## Overlap with writ

Both tools aim to make coding agents more consistent over time:

- Both can sit beside the host agent and feed context into its prompt.
- Both support local operation and have paths that keep data on the machine.
- Both target multi-agent environments (Claude Code, Cursor, Codex, OpenCode,
  Windsurf).
- Both preserve history to some degree: mem0 via ADD-only extraction, writ via
  archiving instead of deleting.

## Gaps vs writ

For writ's specific use case — durable, human-approved steering/corrections for
code review — mem0 lacks:

- **Required rationale for each rule.** mem0 stores extracted facts, not
  human-authored rules with explanations of why they apply.
- **Human approval before activation.** mem0 memories enter retrieval on add;
  there is no `proposed` inbox or activation step.
- **Blocking vs advisory distinction.** writ's `blocking` boolean gates whether
  an unfixed finding stops a handoff; mem0 has no equivalent review gate.
- **Diff-time selection against code changes.** writ selects rules by
  project/language/glob scope matched to the current diff. mem0 retrieves by
  semantic/keyword/entity similarity to the current query.
- **Bounded audit prompt.** writ caps emitted rules at `max_rules` and
  `max_chars` and asks for findings back; mem0 retrieval is bounded by token
  budget but is not a structured review prompt.
- **Evidence-preserving archive.** `writ archive` retires a rule without
  deleting it. mem0's delete behavior is not confirmed to be non-destructive.
- **No dependency on a model for core operation.** writ selects in SQLite; mem0
  requires an LLM for memory extraction.

## writ gaps vs mem0

mem0 is stronger where writ deliberately does not play:

- **Cross-session factual memory.** mem0 remembers user preferences, account
  state, and prior conversation facts across arbitrary sessions. writ only
  knows the rules in its ledger.
- **Natural-language retrieval.** Semantic + keyword + entity search lets an
  agent find memories without knowing exact labels. writ requires explicit
  scopes.
- **Multi-tenant production memory.** mem0 offers managed cloud, org-level
  memory, SSO, audit logs, and compliance certifications. writ has no hosted
  service.
- **Broad framework integration.** mem0 ships SDKs and integrations for 20+
  frameworks and 19+ vector stores. writ integrates only with coding-agent
  hosts.
- **Token efficiency focus.** mem0 benchmarks its retrieval under ~7K tokens
  per query. writ caps prompt size but does not optimize semantic compression
  of memories.

## Sources

1. mem0.ai homepage — https://mem0.ai/
2. mem0 GitHub README — https://github.com/mem0ai/mem0
3. mem0 docs introduction — https://docs.mem0.ai/introduction
4. mem0 pricing — https://mem0.ai/pricing
5. Memory types — https://docs.mem0.ai/core-concepts/memory-types
6. AI memory management blog — https://mem0.ai/blog/ai-memory-management-for-llms-and-agents
7. State of AI Agent Memory 2026 — https://mem0.ai/blog/state-of-ai-agent-memory-2026
8. Benchmark comparison (OpenAI Memory, LangMem, MemGPT, mem0) — https://mem0.ai/blog/benchmarked-openai-memory-vs-langmem-vs-memgpt-vs-mem0-for-long-term-memory-here-s-how-they-stacked-up
9. The AI Agent Index profile — https://theaiagentindex.com/agents/mem0
10. Eden AI comparison — https://www.edenai.co/post/ai-agent-memory-mempalace-mem0-and-persistent-context

Reviewed 2026-09-07.
