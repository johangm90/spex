# Evaluation Patterns Reference — spex-ai-eng

Evals come before code. Define your evaluation metrics and author your test dataset before writing any implementation. A prompt with no evals is untestable and a production liability.

---

## Evaluation Metrics

### Retrieval Metrics (RAG)

| Metric | What It Measures | Target |
|--------|-----------------|--------|
| **Recall@K** | % of relevant docs appearing in top-K retrieved chunks | ≥ 0.80 at K=5 |
| **Precision@K** | % of retrieved chunks that are actually relevant | ≥ 0.70 at K=5 |
| **MRR** (Mean Reciprocal Rank) | How high the first relevant result ranks | ≥ 0.75 |
| **Hit Rate** | % of queries where ≥1 relevant chunk is in top-K | ≥ 0.90 |

### Generation Metrics (LLM Output)

| Metric | What It Measures | Tool / Method |
|--------|-----------------|---------------|
| **Exact Match** | Output equals expected string exactly | String comparison; good for structured output / function calling |
| **ROUGE-L** | Longest common subsequence overlap between generated and reference answer | `rouge-score` Python library |
| **Faithfulness** | Does the answer contain only claims supported by the retrieved context? | RAGAS `faithfulness`, LLM-as-judge |
| **Answer Relevance** | Does the answer actually address the question asked? | RAGAS `answer_relevance`, LLM-as-judge cosine similarity |
| **Hallucination Rate** | % of responses containing claims not grounded in context | LLM-as-judge; log and alert if > 5% |
| **Context Utilization** | % of retrieved context that was actually used in the answer | RAGAS `context_utilization` |
| **Latency (p50/p95)** | End-to-end response time | Instrument at the feature boundary; assert in load tests |

### Minimum Dataset Requirement

**≥ 20 input/output pairs are required before shipping any LLM feature.** This is a hard gate.

Each pair must include:
- `input`: the user query or input payload
- `expected_output`: the ground-truth answer or structured output
- `context` (for RAG): the reference document(s) that should ground the answer
- `tags`: labels for slicing results (e.g., `["edge-case", "multilingual", "long-context"]`)

```jsonc
// evals/<feature>/dataset.jsonl  — one JSON object per line
{
  "id": "eval-001",
  "input": "What is the refund policy for digital products?",
  "expected_output": "Digital products are non-refundable unless they are defective.",
  "context": ["docs/policies/refunds.md#digital"],
  "tags": ["policy", "refund"]
}
```

---

## Eval Framework Integrations

### LangSmith

```typescript
import { Client } from "langsmith";
import { evaluate } from "langsmith/evaluation";

const client = new Client();

await evaluate(
  async (inputs) => myLLMFeature(inputs.query),
  {
    data: "my-feature-eval-dataset",  // dataset name in LangSmith
    evaluators: [
      "correctness",
      "faithfulness",
    ],
    experimentPrefix: "my-feature-v1",
    client,
  }
);
```

**Use LangSmith when:** the team already uses LangChain; you want a hosted dashboard with trace inspection.

### Braintrust

```typescript
import Braintrust from "braintrust";

const experiment = await Braintrust.init("my-feature", {
  project: "my-project",
});

for (const row of evalDataset) {
  const output = await myLLMFeature(row.input);
  await experiment.log({
    input: row.input,
    output,
    expected: row.expected_output,
    scores: {
      exact_match: output.trim() === row.expected_output.trim() ? 1 : 0,
    },
  });
}

await experiment.summarize({ showSamples: true });
```

**Use Braintrust when:** you need a lightweight, framework-agnostic eval harness with a clean UI.

### RAGAS (RAG-specific)

```python
from ragas import evaluate
from ragas.metrics import faithfulness, answer_relevancy, context_recall
from datasets import Dataset

data = Dataset.from_list(eval_dataset)  # [{question, answer, contexts, ground_truth}]

results = evaluate(
    dataset=data,
    metrics=[faithfulness, answer_relevancy, context_recall],
)
print(results.to_pandas())
```

**Use RAGAS when:** evaluating a RAG pipeline; provides RAG-specific metrics out of the box.

### Custom Eval Runner

When you don't want a third-party dependency, implement a minimal eval runner:

```typescript
// evals/<feature>/run.ts
import { evalDataset } from "./dataset";
import { myLLMFeature } from "../../src/ai/feature";

async function run() {
  const results = [];
  for (const row of evalDataset) {
    const output = await myLLMFeature(row.input);
    results.push({
      id: row.id,
      input: row.input,
      expected: row.expected_output,
      actual: output,
      passed: output.trim() === row.expected_output.trim(),
    });
  }

  const passed = results.filter(r => r.passed).length;
  console.log(`Passed: ${passed}/${results.length}`);
  if (passed / results.length < 0.80) {
    process.exit(1);  // fail the CI gate
  }
}

run();
```

---

## Prompt Versioning Strategy

Every prompt change is a versioned artifact. Treat prompts like code.

### Directory Convention

```
prompts/
  v1/
    <feature-name>.md        # first version
  v2/
    <feature-name>.md        # updated after eval regression or improvement
  v3/
    <feature-name>.md
```

### Prompt File Format

```markdown
# Prompt: <feature-name> — v<N>

## Version
- **Version:** v<N>
- **Date:** YYYY-MM-DD
- **Author:** spex-ai-eng
- **Eval delta:** ROUGE-L +0.04, Faithfulness +0.07 vs v<N-1>
- **Change rationale:** Added explicit instruction to cite sources; reduced hallucination rate from 8% to 3%

## System Prompt

You are a helpful assistant. Answer the user's question using ONLY the provided context.
If the answer is not in the context, say "I don't have information about that."
Cite the source title when referencing information.

## User Prompt Template

Context:
{{context}}

Question: {{query}}

## Notes

- Temperature: 0.1 (factual task — low temperature)
- Max tokens: 1024
- Fallback: gpt-4o-mini if primary model unavailable
```

### Rules

1. **Never edit a versioned prompt file in place.** Create a new version directory.
2. **Record the eval delta** for every version bump — what metric improved and by how much.
3. **Record the change rationale** — why was the prompt changed? What failure mode prompted it?
4. **Register the prompt file as an artifact** in MCP state:
   ```
   artifact_register(id="PROMPT-NNN", type="doc", path="prompts/v2/my-feature.md", ...)
   ```

---

## Cost Monitoring

### Token Usage Logging (per request)

Log token usage at the feature boundary, not inside the LLM client. This enables per-feature cost attribution.

```typescript
interface TokenUsageLog {
  feature: string;          // e.g., "rag-search", "summarize", "chatbot"
  model: string;            // e.g., "gpt-4o-mini"
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  estimated_cost_usd: number;
  request_id: string;
  user_id?: string;
  timestamp: string;        // ISO-8601
}

function logTokenUsage(response: LLMResponse, feature: string): void {
  const cost = estimateCost(response.model, response.usage);
  logger.info("ai.token_usage", {
    feature,
    model: response.model,
    prompt_tokens: response.usage.prompt_tokens,
    completion_tokens: response.usage.completion_tokens,
    total_tokens: response.usage.total_tokens,
    estimated_cost_usd: cost,
    request_id: response.id,
    timestamp: new Date().toISOString(),
  });
}
```

### Cost Attribution

Group costs by feature in your observability platform:
```
ai.token_usage.estimated_cost_usd  grouped_by=feature
```

This lets you identify which feature is responsible for a cost spike.

### Alerting Thresholds

Define alerting thresholds in `monitoring/ai-costs.yaml`:

```yaml
# monitoring/ai-costs.yaml
alerts:
  - name: ai_cost_spike_hourly
    condition: sum(ai.token_usage.estimated_cost_usd, window=1h) > 10.00
    severity: warning
    notify: ["#ai-alerts"]

  - name: ai_cost_budget_daily
    condition: sum(ai.token_usage.estimated_cost_usd, window=24h) > 50.00
    severity: critical
    notify: ["#ai-alerts", "oncall"]

  - name: ai_hallucination_rate
    condition: rate(ai.hallucination_detected, window=1h) > 0.05
    severity: warning
    notify: ["#ai-quality"]
```

---

## Fallback Strategy Patterns

A fallback strategy is **required** for every production LLM integration. Define it in the feature contract before implementation.

### Pattern 1 — Model Cascade

Try the primary model; fall back to a cheaper/faster model on rate limit or error:

```typescript
async function callWithFallback(prompt: Prompt): Promise<string> {
  try {
    return await primaryModel.complete(prompt);  // e.g., gpt-4o
  } catch (err) {
    if (isRateLimitOrBudgetError(err)) {
      logger.warn("ai.fallback", { reason: err.code, fallback_model: "gpt-4o-mini" });
      return await fallbackModel.complete(prompt);  // e.g., gpt-4o-mini
    }
    throw err;
  }
}
```

### Pattern 2 — Cached Response

For read-heavy, low-variance queries, serve a cached response when the model is unavailable:

```typescript
async function cachedLLMCall(query: string): Promise<string> {
  const cacheKey = `ai:cache:${hash(query)}`;
  const cached = await cache.get(cacheKey);
  if (cached) return cached;

  const response = await llm.complete(query);
  await cache.set(cacheKey, response, { ttl: 3600 });  // 1h TTL
  return response;
}
```

### Pattern 3 — Graceful Degradation

When AI is unavailable, return a deterministic non-AI response:

```typescript
async function smartSearch(query: string): Promise<SearchResult[]> {
  try {
    return await semanticSearch(query);  // vector search + LLM re-ranking
  } catch (err) {
    logger.error("ai.degraded", { error: err.message });
    return await keywordSearch(query);   // fallback: plain text search
  }
}
```

**Document which pattern applies** to each feature in the feature contract and in the feature's `README` or ADR.

---

## LLM-as-Judge Pattern

Use an LLM to score another LLM's output when there is no deterministic ground truth. The judge model should be **at least as capable** as the model being judged (e.g., use GPT-4o to judge GPT-4o-mini outputs).

### TypeScript Implementation

```typescript
// evals/lib/llm-judge.ts
import OpenAI from "openai";

const client = new OpenAI();

export interface JudgeResult {
  score: number;          // 0.0 – 1.0
  reasoning: string;
  passed: boolean;        // score >= threshold
}

const JUDGE_SYSTEM_PROMPT = `You are an impartial evaluator. Score the assistant's response on the following criteria.
Return a JSON object with keys: score (float 0.0-1.0), reasoning (string, max 100 words).
Be strict: a score of 1.0 means the response is perfect; 0.0 means completely wrong or harmful.`;

export async function judgeResponse(params: {
  question: string;
  context: string;
  response: string;
  criteria: string;       // e.g. "faithfulness to context, relevance, conciseness"
  threshold?: number;     // default 0.7
}): Promise<JudgeResult> {
  const { question, context, response, criteria, threshold = 0.7 } = params;

  const completion = await client.chat.completions.create({
    model: "gpt-4o",
    response_format: { type: "json_object" },
    messages: [
      { role: "system", content: JUDGE_SYSTEM_PROMPT },
      {
        role: "user",
        content: `Criteria: ${criteria}\n\nQuestion: ${question}\n\nContext:\n${context}\n\nResponse to evaluate:\n${response}`,
      },
    ],
    temperature: 0,  // deterministic — always use 0 for judges
  });

  const raw = JSON.parse(completion.choices[0].message.content!);
  return {
    score: raw.score,
    reasoning: raw.reasoning,
    passed: raw.score >= threshold,
  };
}
```

### PHP Implementation (openai-php/client)

```php
// src/AI/Eval/LlmJudge.php
namespace App\AI\Eval;

use OpenAI\Client;

final class LlmJudge
{
    public function __construct(private readonly Client $openai) {}

    /**
     * @return array{score: float, reasoning: string, passed: bool}
     */
    public function judge(
        string $question,
        string $context,
        string $response,
        string $criteria,
        float  $threshold = 0.7,
    ): array {
        $result = $this->openai->chat()->create([
            'model'           => 'gpt-4o',
            'response_format' => ['type' => 'json_object'],
            'temperature'     => 0,
            'messages'        => [
                [
                    'role'    => 'system',
                    'content' => 'You are an impartial evaluator. Score the assistant\'s response. '
                               . 'Return JSON: {"score": float 0.0-1.0, "reasoning": string}.',
                ],
                [
                    'role'    => 'user',
                    'content' => "Criteria: {$criteria}\n\nQuestion: {$question}\n\nContext:\n{$context}\n\nResponse:\n{$response}",
                ],
            ],
        ]);

        $data = json_decode($result->choices[0]->message->content, true);

        return [
            'score'     => (float) $data['score'],
            'reasoning' => $data['reasoning'],
            'passed'    => $data['score'] >= $threshold,
        ];
    }
}
```

### Guidance

| Concern | Rule |
|---------|------|
| Judge model choice | Always use the most capable available model (GPT-4o, Claude 3.7 Sonnet). A cheaper model is biased toward its own style. |
| Temperature | Always 0 — you want deterministic scoring. |
| Response format | Always `json_object` / structured output — never parse free text. |
| Threshold | Default 0.70. Raise to 0.85 for safety-critical features. Lower to 0.60 only for creative tasks. |
| Self-judging | Never judge with the same model that produced the output — always use a different (stronger) model. |
| Calibration | Manually review 10 random judge outputs per dataset; adjust the criteria prompt until judge aligns with human rating. |
| Confidence | Aggregate ≥ 20 samples before trusting a score; single-sample judge scores are noisy. |

---

## CI Gate — GitHub Actions

Block merges to `main` when eval scores drop below the threshold. This prevents prompt regressions from shipping silently.

```yaml
# .github/workflows/ai-eval.yml
name: AI Eval Gate

on:
  pull_request:
    paths:
      - "src/ai/**"
      - "prompts/**"
      - "evals/**"

jobs:
  eval:
    runs-on: ubuntu-latest
    timeout-minutes: 15

    steps:
      - uses: actions/checkout@v4

      - name: Set up Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "22"
          cache: "npm"

      - name: Install dependencies
        run: npm ci

      # TypeScript eval runner — exits 1 if pass rate < threshold
      - name: Run eval suite
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: npx ts-node evals/run-all.ts --threshold 0.80

      - name: Upload eval results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: eval-results-${{ github.sha }}
          path: evals/results/
          retention-days: 30
```

### PHP / Symfony project variant (Pest or PHPUnit)

```yaml
# .github/workflows/ai-eval.yml  (PHP variant)
name: AI Eval Gate

on:
  pull_request:
    paths:
      - "src/AI/**"
      - "prompts/**"
      - "tests/AI/**"

jobs:
  eval:
    runs-on: ubuntu-latest
    timeout-minutes: 15

    steps:
      - uses: actions/checkout@v4

      - name: Set up PHP
        uses: shivammathur/setup-php@v2
        with:
          php-version: "8.3"
          extensions: mbstring, curl

      - name: Install dependencies
        run: composer install --no-interaction --prefer-dist

      - name: Run AI eval tests
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: |
          php bin/console ai:eval:run --threshold=0.80 --fail-on-regression
        # Command exits 1 if pass_rate < threshold OR any metric regressed > 5% vs baseline
```

### Eval runner contract (`evals/run-all.ts`)

```typescript
// evals/run-all.ts
import { runFeatureEval } from "./lib/runner";

const THRESHOLD = parseFloat(process.argv[process.argv.indexOf("--threshold") + 1] ?? "0.80");

async function main() {
  const suites = [
    await import("./rag-search/eval"),
    await import("./summarize/eval"),
    await import("./chatbot/eval"),
  ];

  let totalPassed = 0;
  let totalRun = 0;

  for (const suite of suites) {
    const result = await runFeatureEval(suite.default);
    totalPassed += result.passed;
    totalRun    += result.total;
    console.log(`[${suite.default.name}] ${result.passed}/${result.total} — ${result.passRate.toFixed(2)}`);
  }

  const overallRate = totalPassed / totalRun;
  console.log(`\nOverall: ${totalPassed}/${totalRun} (${(overallRate * 100).toFixed(1)}%)`);

  if (overallRate < THRESHOLD) {
    console.error(`FAIL: overall pass rate ${overallRate.toFixed(2)} < threshold ${THRESHOLD}`);
    process.exit(1);
  }
}

main().catch((err) => { console.error(err); process.exit(1); });
```

---

## A/B Prompt Comparison

Compare two prompt versions side-by-side on the same dataset to decide which to promote to production.

### TypeScript: side-by-side scoring

```typescript
// evals/lib/ab-compare.ts
import { evalDataset } from "../dataset";
import { judgeResponse } from "./llm-judge";
import { runPromptV1 } from "../../prompts/v1/feature";
import { runPromptV2 } from "../../prompts/v2/feature";

interface ABResult {
  id: string;
  input: string;
  scoreV1: number;
  scoreV2: number;
  winner: "v1" | "v2" | "tie";
}

async function runABComparison(): Promise<void> {
  const results: ABResult[] = [];

  for (const row of evalDataset) {
    const [outputV1, outputV2] = await Promise.all([
      runPromptV1(row.input),
      runPromptV2(row.input),
    ]);

    const [judgeV1, judgeV2] = await Promise.all([
      judgeResponse({ question: row.input, context: row.context, response: outputV1, criteria: "faithfulness, relevance, conciseness" }),
      judgeResponse({ question: row.input, context: row.context, response: outputV2, criteria: "faithfulness, relevance, conciseness" }),
    ]);

    results.push({
      id: row.id,
      input: row.input,
      scoreV1: judgeV1.score,
      scoreV2: judgeV2.score,
      winner: judgeV1.score > judgeV2.score + 0.05 ? "v1"
            : judgeV2.score > judgeV1.score + 0.05 ? "v2"
            : "tie",
    });
  }

  // Summary
  const v1wins  = results.filter(r => r.winner === "v1").length;
  const v2wins  = results.filter(r => r.winner === "v2").length;
  const ties    = results.filter(r => r.winner === "tie").length;
  const avgV1   = results.reduce((s, r) => s + r.scoreV1, 0) / results.length;
  const avgV2   = results.reduce((s, r) => s + r.scoreV2, 0) / results.length;

  console.table({ "v1 wins": v1wins, "v2 wins": v2wins, ties, "avg v1": avgV1.toFixed(3), "avg v2": avgV2.toFixed(3) });

  // Decision rule: promote v2 only if it wins on ≥ 60% of samples AND avg score is higher
  if (v2wins / results.length >= 0.60 && avgV2 > avgV1) {
    console.log("DECISION: Promote v2 to production.");
  } else {
    console.log("DECISION: Keep v1. v2 did not meet promotion threshold.");
  }
}

runABComparison();
```

### Promotion Decision Table

| Condition | Decision |
|-----------|----------|
| v2 wins ≥ 60% of samples **AND** avg(v2) > avg(v1) | Promote v2 |
| v2 wins 40–60% of samples OR avg delta < 0.03 | No significant difference — keep v1; iterate further |
| v1 wins ≥ 60% of samples | Revert; v2 is a regression — do not merge |
| Any safety/hallucination regression vs v1 | Hard block — never promote regardless of other scores |

### Rules

1. **Always test on the same dataset** — never compare runs from different samples.
2. **Run the judge at temperature 0** — non-determinism in the judge inflates variance.
3. **Require ≥ 20 samples** — A/B results with fewer samples are statistically unreliable.
4. **Record the comparison result in the prompt file** — add an `## AB vs v(N-1)` section:
   ```markdown
   ## AB vs v1
   - Dataset: `evals/rag-search/dataset.jsonl` (47 samples)
   - v2 wins: 31/47 (66%) — avg score 0.84 vs v1 0.77
   - Decision: Promoted v2 on 2025-03-10
   ```
5. **Log all comparisons as artifacts** in MCP state so the history is auditable.
