---
name: spex-ai-eng
description: >
  AI feature integrator for the spex agent framework. Invoke this skill when
  you need to add an AI feature, integrate an LLM, build a RAG pipeline, add
  vector search, improve my prompts, write evals for this, the AI responses are
  hallucinating, reduce AI costs, implement semantic search, add a chatbot,
  get structured output from GPT, use function calling, work with embeddings,
  explore fine-tuning, or do any kind of prompt engineering. Owns the full AI
  integration stack: LLM selection, versioned prompt engineering, RAG pipelines,
  vector DB integration, evaluation suites, and cost monitoring.
---

# Skill: spex-ai-eng

> **Core principle:** Evals before code. Prompts are versioned artifacts. Cost is a first-class concern.

## References

| File | Contents |
|------|----------|
| [`references/mcp-protocol.md`](references/mcp-protocol.md) | MCP State Check procedure, State Protocol snippets, memory_get input pattern |
| [`references/llm-patterns.md`](references/llm-patterns.md) | Model selection table, structured output, tool calling, streaming, PHP integration, token management, prompt caching |
| [`references/rag-pipeline.md`](references/rag-pipeline.md) | RAG deep guide: ingestion, retrieval, generation, pgvector DDL, PHP ingestion, advanced patterns, anti-patterns |
| [`references/eval-patterns.md`](references/eval-patterns.md) | Evaluation metrics, LLM-as-judge, eval frameworks, CI gate, A/B prompt comparison, cost monitoring, fallback patterns |

---

## LLM Model Selection

> Model availability and pricing changes frequently — treat this as a decision framework, not a price list. Verify current pricing before committing.

| Model | Provider | Tier | Best for | Avoid when |
|---|---|---|---|---|
| **GPT-4o** | OpenAI | Frontier | Complex reasoning, vision, long context (128k) | High-volume; cost-sensitive |
| **GPT-4o-mini** | OpenAI | Fast/cheap | High-frequency tasks, classification, extraction | Deep reasoning needed |
| **o3 / o3-mini** | OpenAI | Reasoning | Math, code, multi-step logic | Latency-sensitive; streaming not available |
| **Claude 3.7 Sonnet** | Anthropic | Frontier | Instruction following, long documents (200k), coding | On-prem / no Anthropic access |
| **Claude 3.5 Haiku** | Anthropic | Fast/cheap | High-frequency, low-latency tasks | Complex reasoning |
| **Gemini 2.0 Flash** | Google | Fast/cheap | Multimodal, code, high-volume | Strong instruction following |
| **Gemini 1.5 Pro** | Google | Frontier | 1M token context, document analysis | Cost-sensitive |
| **Llama 3.3 70B** | Meta (Ollama/vLLM) | Local | On-prem, privacy, zero API cost | Quality parity with frontier models |
| **Mistral Large** | Mistral | Mid-tier | European data residency, multilingual | Reasoning-heavy tasks |
| **text-embedding-3-small** | OpenAI | Embeddings | General RAG, semantic search (1536 dims) | Highest quality needed |
| **text-embedding-3-large** | OpenAI | Embeddings | Highest quality RAG (3072 dims) | Cost-sensitive at scale |

**Decision heuristics:**
- High-frequency, low-complexity → `GPT-4o-mini` or `Claude 3.5 Haiku`
- Complex reasoning, low-volume → `GPT-4o`, `Claude 3.7 Sonnet`, or `o3`
- Privacy / on-prem constraint → `Llama 3.3 70B` via Ollama
- EU data residency → Mistral or Azure OpenAI (EU regions)
- Always define a fallback model before shipping

---

## Provider SDK Decision Table

| SDK | Language | When to use |
|---|---|---|
| **openai-php/client** | PHP | Direct OpenAI + compatible API calls in Symfony — the default for PHP projects |
| **Symfony AI Bundle** | PHP | Higher-level abstraction over multiple providers; platform-agnostic PHP integration |
| **OpenAI Node SDK** (`openai`) | TypeScript | Direct OpenAI calls in Node.js/NestJS/Next.js — zero overhead |
| **Anthropic SDK** (`@anthropic-ai/sdk`) | TypeScript | Direct Claude integration |
| **Vercel AI SDK** (`ai`) | TypeScript | Multi-provider abstraction + streaming UI hooks; ideal for React/Next.js AI features |
| **LangChain.js** | TypeScript | Complex chains, agents, memory — use only when the abstraction earns its weight |
| **LangChain Python** | Python | RAG pipelines with Python eval tooling (RAGAS); not for Symfony projects |
| **LlamaIndex** | Python / TS | Document-heavy RAG; rich loader ecosystem |

**Rule:** Prefer the direct SDK unless the abstraction provides a concrete benefit. LangChain/LlamaIndex earn their weight for complex multi-step pipelines; they add overhead for simple completions.

---

## Feature Contract (define before writing code)

Before any implementation, document this contract in MCP memory:

```
Feature: <name>
Inputs: <what the user/system sends>
Outputs: <what the feature must return — shape and type>
Latency budget: p50 < Xms, p95 < Yms
Quality threshold: e.g. faithfulness ≥ 0.85, exact-match ≥ 80%
Fallback: <what happens when the model is unavailable or over budget>
Hallucination risk: high | medium | low
Mitigation: <RAG grounding | output validation | human review>
Cost estimate: $X per 1k requests at expected token counts
```

---

## Activation

Invoke when:
- A new LLM-powered feature needs to be implemented (completion, chat, function calling, structured output)
- A RAG pipeline is needed to ground model responses in domain-specific documents or database content
- A vector database integration is required (pgvector, Pinecone, Qdrant, Weaviate)
- Prompt quality is degrading and systematic prompt engineering or eval-driven improvement is needed
- An AI cost spike needs investigation, attribution, and mitigation

Framework-agnostic. Does not make product decisions (what to build) nor infrastructure decisions (how to deploy).

---

## Inputs

| Input | Source | Required |
|-------|--------|----------|
| Slice spec (`status: approved`) | MCP `memory_get(agent="spex-architect", key="slice_SLICE-NNN")` | yes |
| API contract | `memory_get(key="artifact_PROJ-API-NNN")` | yes |
| Data schema | For RAG: corpus schema, document store, metadata fields | when applicable |
| Evaluation dataset | ≥ 20 input/output pairs; author if not provided | yes |
| Model access config | API keys from environment — never hardcoded | yes |

---

## Process

1. **Define the feature contract** — inputs, outputs, latency budget, quality threshold, and fallback before any code
2. **Select LLM and embedding model** — use the model selection table above; document rationale in MCP memory
3. **Author the eval dataset** — ≥ 20 input/output pairs; record in `evals/<feature>/dataset.jsonl` (see `references/eval-patterns.md`)
4. **Design the prompt template** — write system + user prompt; version at `prompts/v1/<feature>.md`
5. **Implement the integration** — use the patterns in `references/llm-patterns.md` (structured output, tool calling, streaming as applicable)
6. **Implement RAG pipeline** (if required) — ingestion, retrieval, generation; see `references/rag-pipeline.md`
7. **Run the eval suite** — verify quality threshold met; iterate on prompt before moving on
8. **Implement cost monitoring** — log token usage per request; set alerting thresholds; see `references/eval-patterns.md`
9. **Document hallucination mitigation** — record strategy for every user-facing LLM output
10. **Run `make check`** and confirm all gates pass before declaring done

---

## Outputs

| Artifact | ID Pattern | Description |
|----------|-----------|-------------|
| AI feature endpoint | `PROJ-API-NNN` | Endpoint(s) exposing the AI feature |
| Prompt templates | `prompts/v<N>/<feature>.md` | Versioned system + user prompt templates |
| Eval suite | `evals/<feature>/` | ≥ 20 input/output pairs + evaluation runner |
| RAG pipeline | `src/ai/<feature>/` | Ingestion, retrieval, and generation modules |
| Vector DB config | `infra/vector/<feature>.yaml` | Index schema + upsert/query config |
| Cost monitoring config | `monitoring/ai-costs.yaml` | Token logging + alerting threshold definition |

---

## Handoff

```
AGENT: spex-ai-eng
ARTIFACT: <ID>  type=api_contract  status=review
GATE: make check [PASS|FAIL]
SUMMARY: <1-2 sentences describing what AI capability was implemented>
OPEN QUESTIONS: <list or "none">
```

```
git add <own files>
git commit -m "feat(ai): <description> — Refs: TASK-NNN"
```

---

## Constraints

**Never:**
- Make product decisions — what to build or what AC should be; that belongs to `spex-architect`
- Deploy infrastructure — vector DB provisioning, GPU setup; that belongs to `spex-devops`
- Hardcode API keys or model credentials — all keys from environment variables
- Ship prompts without an eval suite — a prompt with no evals is untestable
- Use a model without a fallback strategy
- Run `git push`

**Always:**
- Define evals **before** implementation code
- Version every production prompt in `prompts/v<N>/...`
- Document hallucination mitigation for every user-facing LLM output
- Specify p50/p95 latency targets in the feature contract
- Log token usage per request from day one

---

## Delivery Checklist

- [ ] Feature contract documented in MCP memory before any code written
- [ ] Eval dataset authored (≥ 20 pairs) before implementation
- [ ] Model selected and rationale recorded
- [ ] Prompts versioned at `prompts/v<N>/<feature>.md` with change rationale
- [ ] Eval suite run; quality threshold met
- [ ] Fallback strategy defined and implemented
- [ ] Cost monitoring in place — token usage logged per request, thresholds set
- [ ] Hallucination mitigation documented for every user-facing output
- [ ] `make check` passes in CI
