# Zep / Letta — competitive brief

Objective notes on [Zep](https://www.getzep.com/) and [Letta](https://www.letta.com/)
(formerly MemGPT), two long-term memory systems for AI agents, compared against
writ, a local-first ledger of steering/corrections for coding agents.

Zep and Letta are distinct products today. Zep is the closer peer to writ in the
memory-layer sense: it is infrastructure that adds persistent, retrievable
context to agents. Letta has become a broader stateful-agent harness/platform;
it is noted briefly at the end. Most of this brief focuses on Zep.

## Zep — what it is

Zep is an agent-memory infrastructure platform built around a temporal context
graph. It ingests chat messages, documents, JSON/CSV events, and business data,
then constructs a per-user (or per-customer, per-session) graph in which
entities, facts, and relationships carry validity windows. At query time it
assembles a token-efficient context block from the most relevant, currently-true
facts plus derived patterns called Observations. It is positioned as
"memory infrastructure" rather than a single-agent feature.

Primary form factors:

- **Zep Cloud** — managed service with dashboards, analytics, and enterprise
deployment options.
- **Graphiti** — the open-source temporal knowledge-graph framework (MIT) that
powers Zep Cloud; self-hosters assemble it with a graph database backend
(Neo4j, FalkorDB, or Kuzu) and their own vector store.
- **Memory MCP Server** — exposes Zep memory to MCP clients.

The older Zep Community Edition (self-hosted all-in-one package) was deprecated
in April 2025; new self-hosted deployments use Graphiti directly.

Sources:
[Zep homepage](https://www.getzep.com/),
[Agent Memory product page](https://www.getzep.com/product/agent-memory/),
[Context Lake page](https://www.getzep.com/platform/context-lake/),
[GitHub zep repo (examples + integrations)](https://github.com/getzep/zep),
[Graphiti repo](https://github.com/getzep/graphiti),
[Zep pricing](https://www.getzep.com/pricing/),
[How to give an AI agent long-term memory](https://www.getzep.com/ai-agents/how-to-give-ai-agents-long-term-memory/),
[Announcing a new direction for Zep's open source strategy](https://blog.getzep.com/announcing-a-new-direction-for-zeps-open-source-strategy/),
[Zep temporal knowledge graph paper/blog](https://blog.getzep.com/zep-a-temporal-knowledge-graph-architecture-for-agent-memory/).

## Zep — goals

- Persist facts, preferences, and events across sessions without stuffing full
  chat history into the context window.
- Maintain temporal accuracy: when a fact changes, the old version is
  invalidated rather than overwritten, so retrieval can answer "what is true
  now" or "what was true then".
- Provide governed, multi-tenant memory at scale (Context Lake) with access
  control, retention policies, provenance, and audit logs.
- Serve relevant context in under 200 ms p95 for production agents.

## Zep — memory / steering model

Zep stores a **temporal knowledge graph**, not raw transcripts or flat vector
chunks. The graph has three conceptual layers:

- **Episodes** — the raw, lossless record of inputs (messages, documents,
  structured payloads).
- **Entities and facts** — extracted nodes and edges, each with a validity
  window (`valid_at` / `invalid_at`). When a fact changes, the old edge is
  closed and a new edge is opened, preserving history.
- **Observations** — derived patterns, recurrences, and co-occurrences surfaced
  by analyzing the graph structure.

Retrieval combines semantic similarity search, BM25 full-text search, and graph
traversal (breadth-first search), then reranks with RRF, MMR, episode-mention
frequency, node-distance, and optionally cross-encoders. The output is a
prompt-ready Context Block shaped by a template.

Zep **requires an LLM and an embedding model** for extraction and search. writ
does not call a model; the host agent reasons over selected rules.

Sources:
[Agent Memory product page](https://www.getzep.com/product/agent-memory/),
[How to give an AI agent long-term memory](https://www.getzep.com/ai-agents/how-to-give-ai-agents-long-term-memory/),
[temporal knowledge graph paper/blog](https://blog.getzep.com/zep-a-temporal-knowledge-graph-architecture-for-agent-memory/),
[LongMemEval results discussion](https://www.getzep.com/ai-agents/how-to-give-ai-agents-long-term-memory/).

## Zep — local vs cloud

| Form factor | Data location | Best for |
|---|---|---|
| Zep Cloud | Managed SaaS | Teams that want zero-ops memory |
| Cloud + BYOK | Managed compute, customer-managed keys | Compliance without self-hosting |
| Bring Your Own Cloud | Customer VPC | Full network/perimeter control |
| Graphiti self-hosted | Customer infrastructure (Neo4j/FalkorDB/Kuzu + vector store) | Teams with DevOps capacity and data-residency requirements |

Zep Cloud is credit-based: ingestion of Episodes consumes credits; storage,
retrieval, users, and graph storage are unmetered. Free tier: 10,000 credits/month.
Paid self-serve starts at $125/month (Flex). Enterprise adds SOC 2 Type II,
HIPAA BAA, audit logs, and SLA.

writ is local-only: one SQLite database under XDG paths, no server, no account,
no telemetry.

Sources:
[Zep pricing](https://www.getzep.com/pricing/),
[Context Lake page](https://www.getzep.com/platform/context-lake/),
[GitHub zep repo](https://github.com/getzep/zep),
[Graphiti repo](https://github.com/getzep/graphiti).

## Zep — agent integration

Zep integrates with several agent frameworks:

- **SDKs** — Python (`zep-cloud`), TypeScript (`@getzep/zep-cloud`), Go
  (`zep-go/v3`).
- **Framework packages** — LangGraph, LangChain, LlamaIndex, CrewAI, AutoGen /
  AG2, Microsoft Agent Framework, Google ADK, Pydantic AI, LiveKit, Mastra,
  Vercel AI SDK.
- **MCP** — Memory MCP Server for Claude Desktop, Cursor, Windsurf, VS Code,
  and other MCP clients.
- **Ingestion** — `zep-ingest` pipeline for Slack, documents, email, JSON/CSV,
  and fact triples.

writ's integration surface is narrower by design: MCP server + `writ install`
for Claude Code, Codex, Cursor, and OpenCode, plus the `writ audit` CLI.

Sources:
[GitHub zep repo](https://github.com/getzep/zep),
[Agent Memory product page](https://www.getzep.com/product/agent-memory/).

## Zep — curation / prune / audit

**Curation.** Zep Cloud provides a dashboard for browsing memories, plus
analytics on the Flex Plus and Enterprise tiers. Open-source Graphiti users
manage the graph through the Graphiti API and their own tooling.

**Prune.** Temporal invalidation closes facts rather than deleting them. There
is also policy-driven retention. Whether there is a first-class archive/retire
operation equivalent to `writ archive` that preserves evidence while stopping
selection is not documented in the public pages reviewed. **Marked as unknown.**

**Audit.** Zep Cloud keeps API logs (1 day on Flex, 7 days on Flex Plus, 1 year
on Enterprise) and audit logs on Enterprise. It does not appear to run a
diff-time review or emit a prompt asking the host agent to report violations of
human-authored rules; it is a retrieval layer, not a gate on code handoff.

## Zep — overlap with writ

Both tools aim to make agents more consistent over time:

- Both sit beside the host agent and feed relevant context into its prompt.
- Both preserve history to some degree: Zep via temporal invalidation and
  episode provenance, writ via archiving instead of deleting.
- Both support paths that keep data on the user's machine (writ by default;
  Zep via Graphiti self-hosting).
- Both can be used with coding-agent hosts through MCP or SDK integrations.

## Zep — gaps vs writ

For writ's specific use case — durable, human-approved steering/corrections for
code review — Zep lacks:

- **Required rationale for each rule.** Zep stores extracted facts and
  Observations, not human-authored rules with explanations of why they apply.
- **Human approval before activation.** Zep memories enter the graph on ingest;
  there is no `proposed` inbox or activation step before retrieval.
- **Blocking vs advisory distinction.** writ's `blocking` boolean gates whether
  an unfixed finding stops a handoff; Zep has no equivalent review gate.
- **Diff-time selection against code changes.** writ selects rules by
  project/language/glob scope matched to the current diff. Zep retrieves by
  semantic/keyword/graph similarity to the current query.
- **Bounded audit prompt.** writ caps emitted rules at `max_rules` and
  `max_chars` and asks for findings back; Zep assembles a Context Block within a
  token budget but is not a structured review prompt.
- **Evidence-preserving archive.** `writ archive` retires a rule without
  deleting it. Zep's archive/retire behavior is not confirmed to be
  non-destructive.
- **No dependency on a model for core operation.** writ selects in SQLite; Zep
  requires an LLM for entity/fact extraction and embeddings for retrieval.

## writ gaps vs Zep

Zep is stronger where writ deliberately does not play:

- **Cross-session factual memory.** Zep remembers user preferences, account
  state, and prior conversation facts across arbitrary sessions. writ only
  knows the rules in its ledger.
- **Temporal reasoning.** Validity windows let Zep answer what was true at a
  point in time; writ has no temporal model.
- **Natural-language / graph retrieval.** Semantic + keyword + graph traversal
  lets an agent find context without exact labels. writ requires explicit
  scopes.
- **Multi-tenant production memory.** Zep offers managed cloud, org-level
  memory, ABAC, retention, SSO, audit logs, and compliance certifications. writ
  has no hosted service.
- **Broad framework integration.** Zep ships SDKs and framework packages for
  many agent stacks. writ integrates only with coding-agent hosts.
- **Structured business-data ingestion.** Zep ingests documents, JSON/CSV,
  email, Slack, and fact triples into the same graph. writ ingests only rules.

## Letta (formerly MemGPT) — brief note

Letta began as MemGPT, an academic project on virtual context management for
LLMs. It has evolved into a stateful-agent harness and platform: agents have
memory blocks, identity, skills, subagents, channels, schedules, and git-backed
context repositories (MemFS). It is not a pure memory layer like Zep or mem0;
it is a runtime for building persistent agents.

Key 2026 form factors:

- **Letta Code** — open-source agent harness (Apache 2.0, active development in
  `letta-ai/letta-code`). Runs locally or self-hosted with PostgreSQL/SQLite,
  optional Redis, and a desktop app for macOS/Windows/Linux.
- **Letta Cloud** — hosted sync for agent memory, identity, and conversations
  across devices, with usage-based pricing.
- **Letta Agent SDK** — TypeScript SDK for embedding Letta agents in
  applications.

Letta's memory model uses memory blocks loaded into the system prompt, archival
vector memory searched via tools, and recall memory for conversation history.
More recently, MemFS projects memory as git-backed Markdown files the agent
reads and edits with ordinary file tools. Like Zep, Letta requires an LLM to
function.

Compared with writ, Letta is even less directly comparable: it is an agent
platform rather than a rule ledger. It shares the broad goal of making agents
consistent over time, but it does not provide human-authored, rationale-bearing,
diff-scoped steering rules with a blocking/advisory review gate.

Sources:
[Letta homepage](https://www.letta.com/),
[Letta Code GitHub](https://github.com/letta-ai/letta-code),
[Letta landing/archive repo](https://github.com/letta-ai/letta),
[Letta docs — MemFS](https://docs.letta.com/concepts/memfs/index.md),
[Letta pricing](https://docs.letta.com/pricing),
[Letta's next phase blog](https://www.letta.com/blog/our-next-phase/).

## Suggested matrix values

The following are proposed values for the `zep-letta` column of
`COMPARISON.md`, pending synthesis. They describe Zep unless noted; Letta's
values differ because it is an agent harness, not a memory layer.

| Dimension | Suggested value for Zep / Letta |
|---|---|
| Local-first / data leaves machine | Zep Cloud is SaaS; Graphiti can self-host on own infrastructure. Not local-only by design. Letta Code can run fully local; Letta Cloud syncs state. |
| Durable rules with rationale | Zep stores extracted facts/Observations, not human-authored rules with required rationale. Letta stores memory blocks, skills, and identity, also without per-rule rationale. |
| Human approve before active | No explicit `proposed → active` approval gate; memories enter retrieval on ingest. |
| Scope model | Zep: multi-tenant scoping by user/customer/session, plus custom entity/edge types. Letta: per-agent memory blocks with optional shared memory. Neither matches writ's project/language/glob scopes. |
| Diff-time selection / audit | Zep retrieves by semantic/keyword/graph similarity; not diff-time rule selection. Letta agents read memory blocks and search archival memory via tools; not a diff audit. |
| Prune / archive | Zep uses temporal invalidation and retention policies; evidence-preserving archive equivalent to `writ archive` not confirmed. Letta has git version history for MemFS and block history, but archive semantics differ. |
| UI for collection | Zep Cloud dashboard + analytics; Graphiti users build their own. Letta has the Agent Development Environment (ADE) and desktop app. |
| Terminal findings | Neither is a terminal findings tool; both feed context into the agent. |
| Multi-agent host neutrality | Zep: MCP server + framework packages. Letta: agent harness with MCP client, channels, SDK. writ is narrower: MCP + `writ install` for coding-agent hosts only. |
| Open source | Zep: Graphiti is MIT open source; Zep Cloud is proprietary. Letta Code is Apache 2.0; Letta Cloud is proprietary. |
| Pricing posture | Zep: freemium managed cloud (free tier → $125+/mo Flex → Enterprise); Graphiti is free but self-assembled. Letta: free self-host / BYOK, with paid cloud tiers ($20+/mo). |

## Sources

1. Zep homepage — https://www.getzep.com/
2. Zep Agent Memory product page — https://www.getzep.com/product/agent-memory/
3. Zep Context Lake page — https://www.getzep.com/platform/context-lake/
4. Zep GitHub (examples + integrations) — https://github.com/getzep/zep
5. Graphiti repository — https://github.com/getzep/graphiti
6. Zep pricing — https://www.getzep.com/pricing/
7. How to give an AI agent long-term memory — https://www.getzep.com/ai-agents/how-to-give-ai-agents-long-term-memory/
8. Zep: A temporal knowledge graph architecture for agent memory — https://blog.getzep.com/zep-a-temporal-knowledge-graph-architecture-for-agent-memory/
9. Announcing a new direction for Zep's open source strategy — https://blog.getzep.com/announcing-a-new-direction-for-zeps-open-source-strategy/
10. Letta homepage — https://www.letta.com/
11. Letta Code repository — https://github.com/letta-ai/letta-code
12. Letta archive/landing repository — https://github.com/letta-ai/letta
13. Letta docs — MemFS — https://docs.letta.com/concepts/memfs/index.md
14. Letta pricing — https://docs.letta.com/pricing
15. Letta's next phase blog — https://www.letta.com/blog/our-next-phase/

Reviewed 2026-09-07.
