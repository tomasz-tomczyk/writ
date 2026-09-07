# LangMem — competitive brief

Objective notes on [LangMem](https://github.com/langchain-ai/langmem), the LangChain
 team's open-source SDK for agent long-term memory. Compared against writ, a
 local-first ledger of steering/corrections for coding agents.

## What it is

LangMem is a Python toolkit that helps agents learn and adapt from their
interactions over time. It provides memory managers that extract and consolidate
information from conversations, prompt optimizers that refine system
instructions from feedback, and stateful integrations that persist memories in
LangGraph's long-term memory store.

Primary form factors:

- **Open-source SDK** (`pip install langmem`, MIT license) — core memory API and
  LangGraph store integrations.
- **LangGraph Platform integration** — long-term memory store is available by
  default in LangGraph Platform deployments.
- **Managed service** — LangChain has an interest form for a managed offering;
  this is not a separately launched paid product in the reviewed sources.

Sources: [LangMem GitHub](https://github.com/langchain-ai/langmem),
[LangMem docs](https://langchain-ai.github.io/langmem/),
[LangMem SDK launch blog](https://www.langchain.com/blog/langmem-sdk-launch),
[LangGraph long-term memory docs](https://docs.langchain.com/oss/python/langchain/long-term-memory).

## Goals

- Let LangGraph agents remember facts, preferences, and successful interactions
  across conversational threads and sessions.
- Enable agents to improve their own behavior by updating system prompts based
  on accumulated experience (procedural memory).
- Provide both "hot path" memory tools and background memory managers so
  developers can trade immediacy against thoroughness.
- Offer a storage-agnostic core API while still integrating natively with
  LangGraph's store layer.

## Memory / steering model

LangMem organizes memory into three types, mirroring common cognitive
 distinctions:

| Memory type | Purpose | Typical storage |
|---|---|---|
| Semantic | Facts and knowledge about users, domain, or state | Collection or profile |
| Episodic | Past interactions that can guide future responses | Collection |
| Procedural | System instructions and behavior rules | Prompt rules or collection |

**Semantic memory** can be stored as an open-ended **collection** of facts or as
a strict-schema **profile** that is updated in place. Collections accumulate and
must reconcile new facts with old ones; profiles always reflect the latest state.

**Episodic memory** preserves full interaction contexts as learning examples.

**Procedural memory** uses `create_prompt_optimizer` to rewrite system prompts
from trajectories and optional feedback.

Memories are written in two modes:

- **Hot path** — the agent calls `create_manage_memory_tool` during the
  conversation, adding latency but giving the agent control over what to store.
- **Background** — a memory manager processes the conversation after the fact,
  extracting, consolidating, and updating memories without blocking the reply.

LangMem **requires an LLM to function** for extraction, consolidation, and
prompt optimization. Embeddings are required for semantic search in the store.
writ does not call a model; the host agent reasons over selected rules.

Sources:
[LangMem core concepts](https://langchain-ai.github.io/langmem/concepts/conceptual_guide/),
[LangMem memory tools](https://langchain-ai.github.io/langmem/guides/memory_tools/),
[LangGraph memory storage docs](https://docs.langchain.com/oss/python/langchain/long-term-memory).

## Local vs cloud

LangMem is an open-source library first, with storage left to the application:

| Form factor | Data location | Best for |
|---|---|---|
| In-memory store | Local process | Development and tests; data lost on restart. |
| Self-hosted Postgres / Redis / MongoDB | Own infrastructure | Production persistence under application control. |
| LangGraph Platform | LangGraph managed runtime | Deployed LangGraph agents with a provisioned store. |
| Managed service | LangChain cloud (if launched) | Zero-ops scaling; currently an interest form only. |

The core SDK is local in the sense that it runs in the application process, but
it is not local-only by design. Production deployments typically use a
DB-backed LangGraph store or LangGraph Platform.

writ, by contrast, is local-only: a single SQLite database under XDG paths, no
server, no account, no telemetry.

Sources:
[LangMem GitHub](https://github.com/langchain-ai/langmem),
[LangGraph store integrations](https://docs.langchain.com/oss/python/langchain/long-term-memory),
[MongoDB LangGraph store docs](https://www.mongodb.com/docs/atlas/ai-integrations/langgraph/),
[LangGraph Redis store](https://github.com/redis-developer/langgraph-redis).

## Agent integration

LangMem is LangGraph-native:

- **LangGraph / LangChain** — stateful operators bind directly to LangGraph's
  `BaseStore`; examples use `create_react_agent` and `create_agent`.
- **Core API** — storage-agnostic primitives (`create_memory_manager`,
  `create_prompt_optimizer`) can in principle be used with other frameworks, but
  the documented examples and tooling are centered on LangGraph.
- **Python only** in the main public repository (`pyproject.toml`,
  `src/langmem`). A JavaScript/TypeScript SDK is not evident in the reviewed
  sources; one third-party analysis claims there is no TypeScript SDK.
  **Marked as unknown** pending an official LangChain statement.
- **Storage backends** — `InMemoryStore`, `PostgresStore`, `AsyncPostgresStore`,
  Redis, MongoDB, and any other LangGraph-compatible `BaseStore`.

writ's integration surface is narrower by design: MCP server + `writ install`
for Claude Code, Codex, Cursor, and OpenCode, plus the `writ audit` CLI.

Sources:
[LangMem GitHub README](https://github.com/langchain-ai/langmem),
[LangMem docs](https://langchain-ai.github.io/langmem/),
[AgentMarketCap comparison](https://agentmarketcap.ai/blog/2026/04/08/agent-long-term-memory-architecture-letta-mem0-langmem-zep).

## Curation / prune / audit

**Curation.** LangMem memories are extracted and consolidated by an LLM
according to developer-provided instructions and schemas. There is no explicit
human-approval gate analogous to writ's `proposed → active` workflow; once a
memory is written to the store it is available for retrieval.

**Prune.** The toolkit provides update, delete, and consolidation operations via
the store and memory managers, but it does not enforce a single universal
lifecycle policy. Whether old revisions are preserved (equivalent to
`writ archive`) depends on the chosen store and application logic.
**Marked as unknown** for a first-class non-destructive archive operation.

**Audit.** LangMem does not run a diff-time review or emit a prompt asking the
host agent to report violations against a rule set. It is a retrieval and
prompt-optimization layer, not a gate on code handoff.

Sources:
[LangMem core concepts](https://langchain-ai.github.io/langmem/concepts/conceptual_guide/),
[maksim-tsi LangMem research notes](https://github.com/maksim-tsi/yet-another-agents-memory/blob/main/docs/research/systems/2026-06-02-langmem.md).

## Overlap with writ

Both tools aim to make agents more consistent over time:

- Both can sit beside the host agent and feed context into its prompt.
- Both support local operation and have paths that keep data on the machine.
- Both preserve history rather than discarding it: LangMem via extraction and
  consolidation, writ via archiving instead of deleting.
- Both distinguish what an agent should know from raw conversation transcripts.

## Gaps vs writ

For writ's specific use case — durable, human-approved steering/corrections for
code review — LangMem lacks:

- **Required rationale for each rule.** LangMem stores extracted facts,
  profiles, episodes, and prompt rules, but it does not require a human-authored
  rationale explaining why a rule applies.
- **Human approval before activation.** Memories enter the store on extraction;
  there is no `proposed` inbox or activation step.
- **Blocking vs advisory distinction.** writ's `blocking` boolean gates whether
  an unfixed finding stops a handoff; LangMem has no equivalent review gate.
- **Diff-time selection against code changes.** writ selects rules by
  project/language/glob scope matched to the current diff. LangMem retrieves by
  semantic similarity and metadata filtering against a query.
- **Bounded audit prompt asking for findings back.** writ caps emitted rules at
  `max_rules` and `max_chars` and asks the host to return findings. LangMem
  retrieval is bounded by the store query but is not a structured review prompt.
- **Evidence-preserving archive.** `writ archive` retires a rule without deleting
  it. Whether LangMem can do the same out of the box is not confirmed.
- **No dependency on a model for core operation.** writ selects in SQLite; LangMem
  requires an LLM for memory extraction and an embedding model for semantic
  search.

## writ gaps vs LangMem

LangMem is stronger where writ deliberately does not play:

- **Cross-session factual and personal memory.** LangMem remembers user
  preferences, account state, and prior conversation facts across arbitrary
  sessions. writ only knows the rules in its ledger.
- **Procedural memory / prompt optimization.** LangMem can rewrite system
  prompts from trajectories and feedback. writ stores static rules and does not
  mutate its own instructions.
- **Semantic retrieval.** LangGraph store search finds memories by vector
  similarity. writ requires explicit scopes.
- **LangGraph ecosystem integration.** LangMem is designed for LangGraph agents,
  with native store tools and platform support. writ integrates only with
  coding-agent hosts.
- **Multiple storage backends.** LangMem can use Postgres, Redis, MongoDB, or any
  `BaseStore` implementation. writ uses one local SQLite database.

## Sources

1. LangMem GitHub repository — https://github.com/langchain-ai/langmem
2. LangMem documentation — https://langchain-ai.github.io/langmem/
3. LangMem SDK launch blog — https://www.langchain.com/blog/langmem-sdk-launch
4. LangGraph long-term memory docs — https://docs.langchain.com/oss/python/langchain/long-term-memory
5. LangMem core concepts — https://langchain-ai.github.io/langmem/concepts/conceptual_guide/
6. LangGraph memory overview — https://docs.langchain.com/oss/python/concepts/memory
7. MongoDB LangGraph store integration — https://www.mongodb.com/docs/atlas/ai-integrations/langgraph/
8. LangGraph Redis store — https://github.com/redis-developer/langgraph-redis
9. AgentMarketCap memory comparison — https://agentmarketcap.ai/blog/2026/04/08/agent-long-term-memory-architecture-letta-mem0-langmem-zep
10. maksim-tsi LangMem research notes — https://github.com/maksim-tsi/yet-another-agents-memory/blob/main/docs/research/systems/2026-06-02-langmem.md

Reviewed 2026-09-07.
