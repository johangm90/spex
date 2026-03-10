# RAG Pipeline Reference — spex-ai-eng

A Retrieval-Augmented Generation (RAG) pipeline has three stages: **Ingestion**, **Retrieval**, and **Generation**. Each stage has distinct implementation concerns.

---

## Stage 1 — Ingestion

Ingestion converts raw source documents into searchable vector embeddings stored in a vector database.

### Chunking Strategies

| Strategy | When to Use | Notes |
|----------|-------------|-------|
| **Fixed-size** | Uniform documents (logs, structured records) | Simple; use overlap (10–15%) to preserve context across boundaries |
| **Sentence / paragraph** | Prose documents (articles, docs, policies) | Respects semantic boundaries; use NLP sentence splitters |
| **Recursive character** | General-purpose fallback | Split on `\n\n`, `\n`, ` ` in order; LangChain `RecursiveCharacterTextSplitter` |
| **Semantic** | High-quality RAG where chunk coherence matters | Embed each sentence; split when cosine similarity drops below threshold |
| **Document-structure aware** | Markdown, HTML, PDF with headings | Split on heading levels; preserves hierarchical context |
| **Parent-document** | Long docs needing both broad context and precise retrieval | Store large parent + small child chunks; retrieve child, return parent |

**Recommended defaults:**
- Chunk size: 512–1024 tokens
- Chunk overlap: 10–15% of chunk size
- Metadata to preserve per chunk: `doc_id`, `source_url`, `title`, `section`, `page_number`, `created_at`

### Embedding Models

| Model | Dimensions | Provider | Notes |
|-------|-----------|----------|-------|
| `text-embedding-3-small` | 1536 | OpenAI | Good quality/cost ratio; supports dimension reduction |
| `text-embedding-3-large` | 3072 | OpenAI | Higher quality; higher cost |
| `embed-english-v3.0` | 1024 | Cohere | Strong retrieval performance; multilingual variant available |
| `all-MiniLM-L6-v2` | 384 | HuggingFace (local) | Fast; good for on-prem / privacy constraints |
| `nomic-embed-text` | 768 | Nomic / Ollama | Local; open weights |

**Rule:** The embedding model used at ingestion **must** be identical to the one used at query time. Changing models requires re-embedding the entire corpus.

### Upsert Pattern (TypeScript)

```typescript
// Canonical upsert pattern
async function upsertChunks(chunks: Chunk[]): Promise<void> {
  const embeddings = await embedModel.embedBatch(chunks.map(c => c.text))
  const vectors = chunks.map((chunk, i) => ({
    id: chunk.id,         // deterministic: sha256(doc_id + chunk_index)
    values: embeddings[i],
    metadata: {
      doc_id:      chunk.docId,
      source_url:  chunk.sourceUrl,
      title:       chunk.title,
      section:     chunk.section,
      text:        chunk.text,       // store for retrieval without extra lookup
      created_at:  chunk.createdAt,
      chunk_index: chunk.index,
    },
  }))
  // Batch to avoid rate limits and memory pressure
  for (let i = 0; i < vectors.length; i += 200) {
    await vectorDb.upsert(vectors.slice(i, i + 200))
  }
}
```

### PHP Ingestion (Symfony + pgvector)

```php
<?php
// src/Ai/Ingestion/DocumentIngester.php
declare(strict_types=1);

namespace App\Ai\Ingestion;

use App\Ai\Embedding\EmbeddingClient;
use App\Repository\DocumentChunkRepository;
use Doctrine\ORM\EntityManagerInterface;

final class DocumentIngester
{
    public function __construct(
        private readonly EmbeddingClient $embedder,
        private readonly DocumentChunkRepository $repo,
        private readonly EntityManagerInterface $em,
        private readonly TextChunker $chunker,
    ) {}

    public function ingest(Document $document): void
    {
        $chunks = $this->chunker->chunk($document->getContent(), chunkSize: 512, overlap: 51);

        // Batch embed for efficiency
        $texts = array_map(fn(Chunk $c) => $c->getText(), $chunks);
        $embeddings = $this->embedder->embedBatch($texts); // returns float[][]

        foreach ($chunks as $i => $chunk) {
            $existing = $this->repo->findByDocumentAndIndex($document->getId(), $i);
            $entity = $existing ?? new DocumentChunk();

            $entity->setDocumentId($document->getId())
                   ->setChunkIndex($i)
                   ->setText($chunk->getText())
                   ->setEmbedding($embeddings[$i]) // stored as vector type
                   ->setMetadata([
                       'source_url' => $document->getSourceUrl(),
                       'title'      => $document->getTitle(),
                       'section'    => $chunk->getSection(),
                       'created_at' => (new \DateTimeImmutable())->format(\DateTimeInterface::ATOM),
                   ]);

            $this->em->persist($entity);
        }

        $this->em->flush();
    }
}
```

---

## Stage 2 — Retrieval

### pgvector Schema (PostgreSQL — self-hosted preferred)

```sql
-- Enable extension
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;   -- for hybrid search

-- Chunks table
CREATE TABLE document_chunks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id UUID NOT NULL,
    chunk_index INT  NOT NULL,
    text        TEXT NOT NULL,
    embedding   vector(1536),             -- match your embedding model dimensions
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (document_id, chunk_index)
);

-- HNSW index for approximate nearest-neighbour (fast at scale)
CREATE INDEX ON document_chunks USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

-- GIN index for hybrid text search
CREATE INDEX ON document_chunks USING gin (to_tsvector('english', text));
CREATE INDEX ON document_chunks (document_id);
CREATE INDEX ON document_chunks USING gin (metadata);
```

```sql
-- Similarity search query
SELECT
    id,
    text,
    metadata,
    1 - (embedding <=> $1::vector) AS score   -- cosine similarity
FROM document_chunks
ORDER BY embedding <=> $1::vector
LIMIT $2;

-- Hybrid search (dense + sparse BM25-like)
WITH dense AS (
    SELECT id, text, metadata,
           1 - (embedding <=> $1::vector) AS dense_score
    FROM document_chunks
    ORDER BY embedding <=> $1::vector
    LIMIT 20
),
sparse AS (
    SELECT id,
           ts_rank_cd(to_tsvector('english', text), plainto_tsquery('english', $2)) AS sparse_score
    FROM document_chunks
    WHERE to_tsvector('english', text) @@ plainto_tsquery('english', $2)
    LIMIT 20
)
SELECT d.id, d.text, d.metadata,
       (0.7 * d.dense_score + 0.3 * COALESCE(s.sparse_score, 0)) AS hybrid_score
FROM dense d
LEFT JOIN sparse s USING (id)
ORDER BY hybrid_score DESC
LIMIT $3;
```

### PHP Retrieval (Doctrine + pgvector)

```php
<?php
// src/Ai/Retrieval/VectorRetriever.php
declare(strict_types=1);

namespace App\Ai\Retrieval;

use App\Ai\Embedding\EmbeddingClient;
use Doctrine\DBAL\Connection;

final class VectorRetriever
{
    public function __construct(
        private readonly Connection $db,
        private readonly EmbeddingClient $embedder,
    ) {}

    /** @return array{id: string, text: string, metadata: array, score: float}[] */
    public function retrieve(string $query, int $topK = 5): array
    {
        $embedding = $this->embedder->embed($query); // returns float[]
        $vectorLiteral = '[' . implode(',', $embedding) . ']';

        $rows = $this->db->fetchAllAssociative(
            'SELECT id, text, metadata,
                    1 - (embedding <=> :embedding::vector) AS score
             FROM document_chunks
             ORDER BY embedding <=> :embedding::vector
             LIMIT :topK',
            ['embedding' => $vectorLiteral, 'topK' => $topK],
        );

        return array_map(
            fn(array $row) => [
                'id'       => $row['id'],
                'text'     => $row['text'],
                'metadata' => json_decode($row['metadata'], true),
                'score'    => (float) $row['score'],
            ],
            $rows,
        );
    }
}
```

### Similarity Search (TypeScript)

```typescript
async function retrieve(query: string, topK: number = 5): Promise<Chunk[]> {
  const queryEmbedding = await embedModel.embed(query)
  const results = await vectorDb.query({
    vector: queryEmbedding,
    topK,
    includeMetadata: true,
    filter: { created_at: { $gte: '2024-01-01' } },  // optional metadata filter
  })
  return results.matches.map(m => ({
    text:  m.metadata.text,
    score: m.score,
    ...m.metadata,
  }))
}
```

**topK guidance:** Start with `topK=5`; tune based on context window size and eval results. More chunks = more context but also more noise and higher cost.

### Re-Ranking

After similarity search, apply a cross-encoder re-ranker to improve precision:

```typescript
const reranked = await reranker.rank({
  query,
  documents: initialResults.map(r => r.text),
  topN: 3,     // return only top-3 after re-ranking
})
```

**Re-ranker options:**
- Cohere Rerank API (`rerank-english-v3.0`) — managed, best quality
- `cross-encoder/ms-marco-MiniLM-L-6-v2` (local, HuggingFace) — free, ~50ms latency
- Jina Reranker

**When to re-rank:** When recall is good but precision is poor (eval shows relevant chunks retrieved but ranked low). Adds ~50–200ms latency.

### Hybrid Search

Combine dense (vector) and sparse (keyword/BM25) retrieval:

```
hybrid_score = α × dense_score + (1 - α) × sparse_score
```

- `α = 0.7` is a reasonable starting point; tune on your eval set
- Sparse retrieval catches exact keyword matches that semantic search misses
- Supported natively by: Weaviate, Qdrant, Pinecone (sparse vectors), pgvector (via `tsvector`)

### Advanced Retrieval Patterns

#### Parent-Document Retrieval

Store small child chunks for precise retrieval, but return the larger parent for richer context:

```typescript
// At ingestion: store both parent (2048 tokens) and child (256 tokens) chunks
// Child chunks reference their parent_id in metadata

// At retrieval: search child chunks, then return their parents
const childResults = await vectorDb.query({ vector: queryEmbedding, topK: 10 })
const parentIds = [...new Set(childResults.map(r => r.metadata.parent_id))]
const parentChunks = await documentStore.getByIds(parentIds)
```

#### Sentence-Window Retrieval

Embed individual sentences but return a ±N sentence window around the matching sentence:

```typescript
// Store: sentence-level embeddings with sentence_index + doc_id
// Retrieve: get top-K sentence matches
// Expand: fetch sentences [index-2 .. index+2] from the same document
const sentence = await vectorDb.query({ vector: queryEmbedding, topK: 5 })
const expanded = await documentStore.getSentenceWindow(sentence.doc_id, sentence.index, window: 2)
```

#### Metadata Filtering

Always filter by metadata where possible to reduce search space and improve relevance:

```typescript
const results = await vectorDb.query({
  vector: queryEmbedding,
  topK: 5,
  filter: {
    doc_type:  { $eq: 'policy' },
    tenant_id: { $eq: currentUser.tenantId },  // mandatory for multi-tenant
  },
})
```

---

## Stage 3 — Generation

### Context Assembly

```typescript
function assembleContext(chunks: Chunk[], maxTokens: number = 2000): string {
  let context = ''
  let tokenCount = 0
  for (const chunk of chunks) {
    const chunkTokens = estimateTokens(chunk.text)
    if (tokenCount + chunkTokens > maxTokens) break
    context += `### Source: ${chunk.title} (${chunk.source_url})\n${chunk.text}\n\n`
    tokenCount += chunkTokens
  }
  return context.trim()
}
```

Always include **source attribution** in assembled context so the model can cite sources and you can audit hallucinations.

### Prompt Construction

```typescript
// Load from versioned prompt file — never hardcode inline
const promptTemplate = await loadPrompt('prompts/v1/rag-search.md')

const systemPrompt = promptTemplate.systemPrompt
const userPrompt = promptTemplate.userPromptTemplate
  .replace('{{context}}', assembledContext)
  .replace('{{query}}', userQuery)
```

### LLM Call with Token Logging

```typescript
const response = await llm.chat({
  model: 'gpt-4o-mini',
  messages: [
    { role: 'system', content: systemPrompt },
    { role: 'user',   content: userPrompt },
  ],
  temperature: 0.1,    // low temperature for factual RAG
  max_tokens:  1024,
})

// Always log token usage at the feature boundary
logTokenUsage(response, feature: 'rag-search')
```

---

## Vector DB Selection

| DB | Hosting | Hybrid Search | Best for |
|----|---------|---------------|----------|
| **pgvector** | Self-hosted (PostgreSQL extension) | Via `tsvector` + `pg_trgm` | Teams already on Postgres; no extra infra — **preferred for Symfony projects** |
| **Pinecone** | Managed cloud | Yes (sparse+dense) | Zero-ops; per-query pricing |
| **Qdrant** | Self-hosted or managed | Yes (built-in) | Open source; strong performance; good Rust + Python clients |
| **Weaviate** | Self-hosted or managed | Yes (BM25 + vector) | GraphQL API; structured metadata; multi-tenancy |
| **Chroma** | Local / self-hosted | No (dense only) | Prototyping only; not production-hardened at scale |

**Selection heuristic:**
- Already on PostgreSQL → **pgvector** (zero new infra, integrates with Doctrine)
- Need managed, zero-ops → Pinecone or Weaviate Cloud
- On-prem / open-source only → Qdrant or Weaviate
- Prototype → Chroma

---

## RAG Anti-Patterns

| Anti-pattern | Symptom | Fix |
|---|---|---|
| **No metadata filtering** | Irrelevant chunks from other tenants or document types retrieved | Always filter by `tenant_id` and `doc_type` |
| **Chunk size too large** | Model ignores most of the context; low context utilization score | Reduce chunk size to 256–512 tokens; use parent-document pattern |
| **Chunk size too small** | Answers missing context; low faithfulness score | Increase chunk size or use sentence-window expansion |
| **Different embed model at query** | All similarity scores near-random | Use identical model at ingest and query time |
| **No re-ranking** | Correct answer retrieved but ranked 4th; model ignores it | Add cross-encoder re-ranker after top-K retrieval |
| **No overlap between chunks** | Answers cut off mid-sentence; context boundary artifacts | Add 10–15% overlap at chunking time |
| **Source not in assembled context** | Model cannot cite; hallucination risk increases | Include `title` and `source_url` in assembled context string |
| **topK too large** | Context window overflow; high cost; model distracted | Tune topK on eval set; use re-ranker to reduce to 3–5 |
| **Stale embeddings after doc update** | Old content returned | Re-embed on document update; use `updated_at` in metadata filter |
| **No eval before ship** | Hallucination rate unknown; no baseline to regress against | Author ≥ 20 eval pairs before writing implementation code |
