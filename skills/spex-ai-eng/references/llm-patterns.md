# LLM Integration Patterns Reference — spex-ai-eng

Canonical patterns for integrating LLMs into production applications. Each section covers TypeScript (Node/Vercel AI SDK) and PHP (openai-php/client + Symfony AI Bundle) where relevant.

---

## Structured Output / JSON Mode

Force the model to return valid JSON matching a schema. Use this for any feature that parses LLM output programmatically.

### OpenAI — TypeScript (Zod schema)

```typescript
import OpenAI from "openai";
import { zodResponseFormat } from "openai/helpers/zod";
import { z } from "zod";

const client = new OpenAI();

const ProductSchema = z.object({
  name:        z.string(),
  category:    z.string(),
  price_usd:   z.number().positive(),
  in_stock:    z.boolean(),
  tags:        z.array(z.string()),
});
type Product = z.infer<typeof ProductSchema>;

async function extractProduct(text: string): Promise<Product> {
  const completion = await client.beta.chat.completions.parse({
    model:           "gpt-4o-2024-08-06",   // structured output requires this model or later
    response_format: zodResponseFormat(ProductSchema, "product"),
    messages: [
      { role: "system", content: "Extract product details from the user text." },
      { role: "user",   content: text },
    ],
  });

  const parsed = completion.choices[0].message.parsed;
  if (!parsed) throw new Error("Model refused or failed to produce structured output");
  return parsed;
}
```

### OpenAI — PHP (openai-php/client)

```php
// src/AI/Extractor/ProductExtractor.php
namespace App\AI\Extractor;

use OpenAI\Client;

final class ProductExtractor
{
    public function __construct(private readonly Client $openai) {}

    /** @return array{name: string, category: string, price_usd: float, in_stock: bool, tags: string[]} */
    public function extract(string $text): array
    {
        $schema = [
            'type'       => 'object',
            'properties' => [
                'name'      => ['type' => 'string'],
                'category'  => ['type' => 'string'],
                'price_usd' => ['type' => 'number'],
                'in_stock'  => ['type' => 'boolean'],
                'tags'      => ['type' => 'array', 'items' => ['type' => 'string']],
            ],
            'required'             => ['name', 'category', 'price_usd', 'in_stock', 'tags'],
            'additionalProperties' => false,
        ];

        $result = $this->openai->chat()->create([
            'model'           => 'gpt-4o-2024-08-06',
            'response_format' => [
                'type'        => 'json_schema',
                'json_schema' => [
                    'name'   => 'product',
                    'strict' => true,
                    'schema' => $schema,
                ],
            ],
            'messages' => [
                ['role' => 'system', 'content' => 'Extract product details from the user text.'],
                ['role' => 'user',   'content' => $text],
            ],
        ]);

        return json_decode($result->choices[0]->message->content, true);
    }
}
```

### Anthropic — TypeScript

```typescript
import Anthropic from "@anthropic-ai/sdk";
import { z } from "zod";

const client = new Anthropic();

const SentimentSchema = z.object({
  sentiment: z.enum(["positive", "negative", "neutral"]),
  score:     z.number().min(-1).max(1),
  summary:   z.string(),
});

async function analyzeSentiment(text: string) {
  const message = await client.messages.create({
    model:      "claude-3-5-sonnet-20241022",
    max_tokens: 256,
    system:
      "Analyze the sentiment of the user text. " +
      "Respond with ONLY valid JSON matching this schema: " +
      JSON.stringify(SentimentSchema._def),   // embed schema in system prompt
    messages: [{ role: "user", content: text }],
  });

  const raw = message.content[0].type === "text" ? message.content[0].text : "";
  return SentimentSchema.parse(JSON.parse(raw));
}
```

> **Anthropic note:** Claude does not have a native `json_schema` response format as of 2025. Embed the schema in the system prompt and validate with Zod after parsing.

### Decision Table

| Scenario | Recommended approach |
|----------|---------------------|
| OpenAI model, complex nested schema | `zodResponseFormat` + `beta.chat.completions.parse` |
| OpenAI model, PHP backend | `json_schema` response format with `strict: true` |
| Anthropic Claude | Schema in system prompt + Zod/JSON validation |
| Gemini 2.0 | `response_mime_type: "application/json"` + `response_schema` |
| Simple key-value extraction | `json_object` mode (legacy, no schema enforcement) |

---

## Tool Calling / Function Calling

Let the model call your application functions to retrieve data or perform actions.

### TypeScript — agentic loop

```typescript
import OpenAI from "openai";

const client = new OpenAI();

// 1. Define tools
const tools: OpenAI.Chat.Completions.ChatCompletionTool[] = [
  {
    type: "function",
    function: {
      name:        "get_order_status",
      description: "Return the current status of a customer order.",
      parameters: {
        type: "object",
        properties: {
          order_id: { type: "string", description: "The order ID (e.g. ORD-12345)" },
        },
        required:             ["order_id"],
        additionalProperties: false,
      },
      strict: true,
    },
  },
  {
    type: "function",
    function: {
      name:        "list_open_orders",
      description: "Return a list of all open orders for a customer.",
      parameters: {
        type:                 "object",
        properties:           { customer_id: { type: "string" } },
        required:             ["customer_id"],
        additionalProperties: false,
      },
      strict: true,
    },
  },
];

// 2. Application-side tool dispatch
async function dispatchTool(name: string, args: Record<string, string>): Promise<string> {
  switch (name) {
    case "get_order_status":
      return JSON.stringify(await orderService.getStatus(args.order_id));
    case "list_open_orders":
      return JSON.stringify(await orderService.listOpen(args.customer_id));
    default:
      throw new Error(`Unknown tool: ${name}`);
  }
}

// 3. Agentic loop — keep running until the model stops calling tools
async function chat(userMessage: string): Promise<string> {
  const messages: OpenAI.Chat.Completions.ChatCompletionMessageParam[] = [
    { role: "system",  content: "You are a helpful order management assistant." },
    { role: "user",    content: userMessage },
  ];

  while (true) {
    const response = await client.chat.completions.create({ model: "gpt-4o", tools, messages });
    const choice   = response.choices[0];
    messages.push(choice.message);  // always append assistant message

    if (choice.finish_reason === "stop") {
      return choice.message.content ?? "";
    }

    if (choice.finish_reason === "tool_calls") {
      for (const toolCall of choice.message.tool_calls ?? []) {
        const args   = JSON.parse(toolCall.function.arguments);
        const result = await dispatchTool(toolCall.function.name, args);
        messages.push({
          role:         "tool",
          tool_call_id: toolCall.id,
          content:      result,
        });
      }
      // loop again — model will process tool results
    }
  }
}
```

### PHP — tool calling (openai-php/client)

```php
// src/AI/Agent/OrderAgent.php
namespace App\AI\Agent;

use OpenAI\Client;

final class OrderAgent
{
    private array $tools = [
        [
            'type'     => 'function',
            'function' => [
                'name'        => 'get_order_status',
                'description' => 'Return the current status of a customer order.',
                'parameters'  => [
                    'type'       => 'object',
                    'properties' => ['order_id' => ['type' => 'string']],
                    'required'   => ['order_id'],
                    'additionalProperties' => false,
                ],
                'strict' => true,
            ],
        ],
    ];

    public function __construct(
        private readonly Client       $openai,
        private readonly OrderService $orderService,
    ) {}

    public function chat(string $userMessage): string
    {
        $messages = [
            ['role' => 'system', 'content' => 'You are a helpful order management assistant.'],
            ['role' => 'user',   'content' => $userMessage],
        ];

        while (true) {
            $response = $this->openai->chat()->create([
                'model'    => 'gpt-4o',
                'tools'    => $this->tools,
                'messages' => $messages,
            ]);

            $choice = $response->choices[0];
            $messages[] = ['role' => 'assistant', 'content' => $choice->message->content, 'tool_calls' => $choice->message->toolCalls];

            if ($choice->finishReason === 'stop') {
                return $choice->message->content ?? '';
            }

            if ($choice->finishReason === 'tool_calls') {
                foreach ($choice->message->toolCalls as $toolCall) {
                    $args   = json_decode($toolCall->function->arguments, true);
                    $result = $this->dispatch($toolCall->function->name, $args);
                    $messages[] = [
                        'role'         => 'tool',
                        'tool_call_id' => $toolCall->id,
                        'content'      => json_encode($result),
                    ];
                }
            }
        }
    }

    private function dispatch(string $name, array $args): mixed
    {
        return match ($name) {
            'get_order_status' => $this->orderService->getStatus($args['order_id']),
            default            => throw new \RuntimeException("Unknown tool: {$name}"),
        };
    }
}
```

### Tool calling rules

| Rule | Rationale |
|------|-----------|
| Always use `strict: true` | Prevents the model from hallucinating extra parameters |
| Validate tool args before executing | The model can still pass wrong values; validate with your domain logic |
| Limit to ≤ 20 tools per request | Beyond 20 tools, model accuracy degrades |
| Log every tool call and result | Required for debugging and for eval replay |
| Cap agentic loop iterations | Always set a `maxIterations` guard (default 10) to prevent runaway loops |

---

## Streaming Responses

Stream tokens to the client for a responsive UX. Required for any interactive chat feature.

### TypeScript — Node.js HTTP SSE

```typescript
// src/api/chat.ts  (Express)
import express from "express";
import OpenAI  from "openai";

const router = express.Router();
const client = new OpenAI();

router.post("/chat/stream", async (req, res) => {
  const { message } = req.body as { message: string };

  // SSE headers
  res.setHeader("Content-Type",  "text/event-stream");
  res.setHeader("Cache-Control", "no-cache");
  res.setHeader("Connection",    "keep-alive");

  const stream = client.beta.chat.completions.stream({
    model:    "gpt-4o",
    messages: [{ role: "user", content: message }],
  });

  for await (const chunk of stream) {
    const delta = chunk.choices[0]?.delta?.content;
    if (delta) {
      res.write(`data: ${JSON.stringify({ text: delta })}\n\n`);
    }
  }

  const finalMsg = await stream.finalMessage();
  res.write(`data: ${JSON.stringify({ done: true, usage: finalMsg.usage })}\n\n`);
  res.end();
});

export default router;
```

### TypeScript — Vercel AI SDK (Next.js route handler)

```typescript
// app/api/chat/route.ts
import { openai }    from "@ai-sdk/openai";
import { streamText } from "ai";

export async function POST(req: Request) {
  const { messages } = await req.json();

  const result = streamText({
    model:    openai("gpt-4o"),
    messages,
    system:   "You are a helpful assistant.",
  });

  return result.toDataStreamResponse();
}
```

### PHP — Symfony SSE endpoint

```php
// src/Controller/ChatStreamController.php
namespace App\Controller;

use OpenAI\Client;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\StreamedResponse;
use Symfony\Component\Routing\Attribute\Route;

final class ChatStreamController extends AbstractController
{
    public function __construct(private readonly Client $openai) {}

    #[Route('/api/chat/stream', methods: ['POST'])]
    public function stream(Request $request): StreamedResponse
    {
        $data    = json_decode($request->getContent(), true);
        $message = $data['message'] ?? '';

        return new StreamedResponse(function () use ($message) {
            $stream = $this->openai->chat()->createStreamed([
                'model'    => 'gpt-4o',
                'messages' => [['role' => 'user', 'content' => $message]],
            ]);

            foreach ($stream as $response) {
                $delta = $response->choices[0]->delta->content ?? '';
                if ($delta !== '') {
                    echo 'data: ' . json_encode(['text' => $delta]) . "\n\n";
                    ob_flush();
                    flush();
                }
            }

            echo "data: " . json_encode(['done' => true]) . "\n\n";
            ob_flush();
            flush();
        }, 200, [
            'Content-Type'  => 'text/event-stream',
            'Cache-Control' => 'no-cache',
            'X-Accel-Buffering' => 'no',   // disable nginx buffering
        ]);
    }
}
```

> **Nginx note:** Always set `X-Accel-Buffering: no` for SSE endpoints behind nginx, or configure `proxy_buffering off` in the location block.

---

## PHP Integration Patterns

### openai-php/client — Service setup (Symfony)

```php
// config/services.yaml
services:
  OpenAI\Client:
    factory: ['OpenAI', 'client']
    arguments:
      $apiKey: '%env(OPENAI_API_KEY)%'

  App\AI\:
    resource: '../src/AI/'
    autowire:  true
```

```php
// src/AI/ChatService.php
namespace App\AI;

use OpenAI\Client;

final class ChatService
{
    public function __construct(private readonly Client $openai) {}

    public function complete(string $prompt, string $model = 'gpt-4o-mini'): string
    {
        $response = $this->openai->chat()->create([
            'model'    => $model,
            'messages' => [['role' => 'user', 'content' => $prompt]],
        ]);

        return $response->choices[0]->message->content;
    }
}
```

### Symfony AI Bundle (symfony/ai-bundle)

The Symfony AI Bundle provides a higher-level abstraction over multiple providers.

```php
// composer require symfony/ai-bundle
// config/packages/ai.yaml
ai:
    platform:
        openai:
            api_key: '%env(OPENAI_API_KEY)%'
    llm:
        default:
            provider: openai
            model:    gpt-4o
```

```php
// src/AI/AssistantService.php
namespace App\AI;

use Symfony\AI\Bundle\Attribute\WithModel;
use Symfony\AI\Contract\LlmInterface;

final class AssistantService
{
    public function __construct(
        #[WithModel('default')]
        private readonly LlmInterface $llm,
    ) {}

    public function answer(string $question): string
    {
        return $this->llm->generate($question);
    }
}
```

> **Prefer `symfony/ai-bundle`** for new Symfony projects — it handles provider switching, retry, and logging out of the box. Use `openai-php/client` directly only when you need raw API access (e.g., streaming SSE, custom tool dispatch, or features the bundle does not yet expose).

---

## Token Counting and Context Window Management

Always count tokens before making an API call when dealing with dynamic context (RAG chunks, conversation history).

### TypeScript — tiktoken

```typescript
import { encoding_for_model } from "tiktoken";

function countTokens(text: string, model: "gpt-4o" | "gpt-4o-mini" = "gpt-4o"): number {
  const enc    = encoding_for_model(model);
  const tokens = enc.encode(text);
  enc.free();
  return tokens.length;
}

const MODEL_CONTEXT_LIMITS: Record<string, number> = {
  "gpt-4o":          128_000,
  "gpt-4o-mini":     128_000,
  "gpt-4-turbo":     128_000,
  "claude-3-5-sonnet-20241022": 200_000,
  "gemini-2.0-flash":           1_000_000,
};

function fitInContext(
  systemPrompt: string,
  chunks: string[],
  model: string = "gpt-4o",
  reserveForCompletion: number = 2048,
): string[] {
  const limit   = MODEL_CONTEXT_LIMITS[model] ?? 8192;
  const budget  = limit - countTokens(systemPrompt) - reserveForCompletion;
  const result: string[] = [];
  let used = 0;

  for (const chunk of chunks) {
    const tokens = countTokens(chunk, "gpt-4o");
    if (used + tokens > budget) break;
    result.push(chunk);
    used += tokens;
  }

  return result;
}
```

### PHP — token counting (approximation)

```php
// src/AI/Util/TokenCounter.php
namespace App\AI\Util;

/**
 * Approximate token count (cl100k_base: ~4 chars/token for English).
 * For exact counts use the OpenAI /v1/embeddings or tiktoken via Python sidecar.
 */
final class TokenCounter
{
    private const CHARS_PER_TOKEN = 4;

    private const CONTEXT_LIMITS = [
        'gpt-4o'          => 128_000,
        'gpt-4o-mini'     => 128_000,
        'claude-3-5-sonnet-20241022' => 200_000,
    ];

    public function approximate(string $text): int
    {
        return (int) ceil(mb_strlen($text) / self::CHARS_PER_TOKEN);
    }

    /**
     * Trim $chunks to fit within the model's context window.
     * @param string[] $chunks
     * @return string[]
     */
    public function fitInContext(string $systemPrompt, array $chunks, string $model = 'gpt-4o', int $reserveForCompletion = 2048): array
    {
        $limit  = self::CONTEXT_LIMITS[$model] ?? 8192;
        $budget = $limit - $this->approximate($systemPrompt) - $reserveForCompletion;
        $result = [];
        $used   = 0;

        foreach ($chunks as $chunk) {
            $tokens = $this->approximate($chunk);
            if ($used + $tokens > $budget) {
                break;
            }
            $result[] = $chunk;
            $used    += $tokens;
        }

        return $result;
    }
}
```

### Context window management rules

| Rule | Action |
|------|--------|
| Always reserve tokens for completion | Never fill 100% of context — leave ≥ 10% or 2048 tokens |
| Truncate from the middle, not the end | For conversation history, drop the oldest middle turns; keep system + last N turns |
| Chunk documents before ingestion | Target 512–1024 tokens per chunk with 15% overlap |
| Log context utilization | Emit `ai.context_utilization = used_tokens / limit_tokens` per request |

---

## Prompt Caching

Reduce cost and latency by caching repeated prefix tokens. Supported on Anthropic (explicit) and OpenAI (automatic).

### Anthropic — explicit cache control (TypeScript)

```typescript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();

// Large static system prompt — cache the prefix
const response = await client.messages.create({
  model:      "claude-3-5-sonnet-20241022",
  max_tokens: 1024,
  system: [
    {
      type: "text",
      text: LARGE_KNOWLEDGE_BASE,          // e.g. 50k-token document
      cache_control: { type: "ephemeral" }, // cache this block
    },
    {
      type: "text",
      text: "Answer the user's question using only the knowledge base above.",
    },
  ],
  messages: [{ role: "user", content: userQuestion }],
});

// Check cache hit
const usage = response.usage as Anthropic.Usage & { cache_read_input_tokens?: number };
console.log("Cache read tokens:", usage.cache_read_input_tokens ?? 0);
```

### OpenAI — automatic prompt caching

OpenAI automatically caches prompts longer than 1024 tokens with no code changes required. Cache hits are reflected in the usage object.

```typescript
const response = await client.chat.completions.create({
  model: "gpt-4o",
  messages,
});

// Cached tokens are in: response.usage?.prompt_tokens_details?.cached_tokens
const cachedTokens = (response.usage as any)?.prompt_tokens_details?.cached_tokens ?? 0;
console.log(`Cached: ${cachedTokens} / ${response.usage?.prompt_tokens} tokens`);
```

### PHP — Anthropic cache control

```php
$response = $this->anthropic->messages()->create([
    'model'      => 'claude-3-5-sonnet-20241022',
    'max_tokens' => 1024,
    'system'     => [
        [
            'type'          => 'text',
            'text'          => $largeKnowledgeBase,
            'cache_control' => ['type' => 'ephemeral'],
        ],
        [
            'type' => 'text',
            'text' => 'Answer the user\'s question using only the knowledge base above.',
        ],
    ],
    'messages' => [['role' => 'user', 'content' => $question]],
]);
```

### Prompt caching rules

| Rule | Detail |
|------|--------|
| Minimum cacheable prefix | Anthropic: ≥ 1024 tokens; OpenAI: ≥ 1024 tokens (automatic) |
| TTL | Anthropic ephemeral: ~5 minutes. Extend by sending the same prefix again before expiry. |
| Cache-friendly design | Put static content (system instructions, knowledge base) first; dynamic content (user query) last |
| Cost | Anthropic charges 25% of normal input price for cache hits; OpenAI charges 50% |
| Monitoring | Log `cache_read_input_tokens` per request — target > 60% cache hit rate for knowledge-base features |

---

## Rate Limiting and Retry with Exponential Backoff

Always implement retry with exponential backoff + jitter. Never retry immediately.

### TypeScript — retry utility

```typescript
// src/ai/lib/retry.ts

export interface RetryOptions {
  maxAttempts?: number;    // default 3
  baseDelayMs?:  number;   // default 1000ms
  maxDelayMs?:   number;   // default 60_000ms
  jitter?:       boolean;  // default true
}

export async function withRetry<T>(
  fn: () => Promise<T>,
  options: RetryOptions = {},
): Promise<T> {
  const {
    maxAttempts = 3,
    baseDelayMs  = 1_000,
    maxDelayMs   = 60_000,
    jitter       = true,
  } = options;

  let attempt = 0;

  while (true) {
    try {
      return await fn();
    } catch (err: any) {
      attempt++;
      const isRetryable = err?.status === 429 || err?.status === 500 || err?.status === 503;

      if (!isRetryable || attempt >= maxAttempts) throw err;

      // Exponential backoff with optional full jitter
      const exponential = Math.min(baseDelayMs * 2 ** (attempt - 1), maxDelayMs);
      const delay        = jitter ? Math.random() * exponential : exponential;

      console.warn(`[retry] attempt ${attempt}/${maxAttempts}, waiting ${Math.round(delay)}ms (status ${err?.status})`);
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}

// Usage
const response = await withRetry(
  () => client.chat.completions.create({ model: "gpt-4o", messages }),
  { maxAttempts: 4, baseDelayMs: 500 },
);
```

### PHP — retry utility (Symfony)

```php
// src/AI/Lib/RetryHandler.php
namespace App\AI\Lib;

use Psr\Log\LoggerInterface;

final class RetryHandler
{
    public function __construct(private readonly LoggerInterface $logger) {}

    /**
     * @template T
     * @param  callable(): T $operation
     * @return T
     */
    public function execute(callable $operation, int $maxAttempts = 3, int $baseDelayMs = 1000): mixed
    {
        $attempt = 0;

        while (true) {
            try {
                return $operation();
            } catch (\Throwable $e) {
                $attempt++;
                $statusCode = method_exists($e, 'getCode') ? $e->getCode() : 0;
                $isRetryable = in_array($statusCode, [429, 500, 503], true);

                if (!$isRetryable || $attempt >= $maxAttempts) {
                    throw $e;
                }

                $exponential = min($baseDelayMs * (2 ** ($attempt - 1)), 60_000);
                $delay       = (int) ($exponential * (0.5 + (mt_rand() / mt_getrandmax()) * 0.5)); // jitter

                $this->logger->warning('AI retry', [
                    'attempt'     => $attempt,
                    'maxAttempts' => $maxAttempts,
                    'delayMs'     => $delay,
                    'error'       => $e->getMessage(),
                ]);

                usleep($delay * 1_000); // microseconds
            }
        }
    }
}
```

### Rate limit headers

Parse `x-ratelimit-*` headers to implement proactive throttling:

```typescript
// OpenAI returns these headers on every response (via fetch — not available on the SDK object directly)
// Use the raw fetch interceptor or a proxy to capture them:
const response = await fetch("https://api.openai.com/v1/chat/completions", {
  method:  "POST",
  headers: { "Authorization": `Bearer ${process.env.OPENAI_API_KEY}`, "Content-Type": "application/json" },
  body:    JSON.stringify({ model: "gpt-4o", messages }),
});

const remainingRequests = Number(response.headers.get("x-ratelimit-remaining-requests"));
const resetMs           = Number(response.headers.get("x-ratelimit-reset-requests")?.replace("s", "")) * 1000;

if (remainingRequests < 10) {
  console.warn(`Rate limit low — ${remainingRequests} requests remaining; resets in ${resetMs}ms`);
}
```

### Retry rules

| Rule | Detail |
|------|--------|
| Retry on: 429, 500, 503 | These are transient. Never retry 400, 401, 404. |
| Max attempts | 3–4 for interactive requests; up to 6 for background jobs |
| Base delay | 500ms–1000ms |
| Always use jitter | Prevents thundering herd — all retrying clients hit the API at different times |
| Log every retry | Include attempt number, delay, and HTTP status — essential for debugging rate limit issues |
| Circuit breaker | For high-traffic services, wrap the retry in a circuit breaker (open after 5 failures in 60s) |
