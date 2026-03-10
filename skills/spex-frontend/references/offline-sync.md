# Offline-First Patterns — spex-frontend

Patterns for writes, optimistic updates, background sync, and retry logic in browser/PWA targets.

---

## Core Constraint

**Assume the network can drop at any moment.** Every user-initiated write must be queued, idempotent, and resumable across page reloads. Never fire a one-shot `fetch` for a mutation without a safety net.

---

## 1. Serial Write Queue with Idempotency Keys

### Why serial?
Parallel writes create ordering races and duplicate-key errors on the server. Process one operation at a time; the next starts only after the previous receives a definitive response (2xx or a non-retryable 4xx).

### Queue entry shape

```ts
interface QueueEntry {
  id: string;            // idempotency key — stable UUID, generated once
  createdAt: string;     // ISO-8601
  attempts: number;
  nextRetryAt: string;   // ISO-8601; exponential back-off
  operation: {
    method: "POST" | "PUT" | "PATCH" | "DELETE";
    url: string;
    body: unknown;
  };
  status: "pending" | "inflight" | "failed";
}
```

### Persistence — survive page reloads

Store the queue in `IndexedDB` (never `localStorage` for write queues — size limits and synchronous API are unsafe):

```ts
// Pseudocode — adapt to your IDB wrapper of choice (e.g. idb, Dexie)
const db = await openDB("write-queue", 1, {
  upgrade(db) {
    db.createObjectStore("entries", { keyPath: "id" });
  }
});

async function enqueue(entry: Omit<QueueEntry, "id" | "createdAt" | "attempts" | "nextRetryAt" | "status">) {
  const record: QueueEntry = {
    id: crypto.randomUUID(),
    createdAt: new Date().toISOString(),
    attempts: 0,
    nextRetryAt: new Date().toISOString(),
    status: "pending",
    operation: entry.operation
  };
  await db.put("entries", record);
  scheduleFlush();
}
```

### Queue flush loop

```ts
let flushing = false;

async function flush() {
  if (flushing) return;
  flushing = true;
  try {
    const all = await db.getAll("entries");
    const due = all
      .filter(e => e.status === "pending" && new Date(e.nextRetryAt) <= new Date())
      .sort((a, b) => a.createdAt.localeCompare(b.createdAt)); // FIFO

    for (const entry of due) {
      await processEntry(entry);   // one at a time — serial
    }
  } finally {
    flushing = false;
  }
}

async function processEntry(entry: QueueEntry) {
  await db.put("entries", { ...entry, status: "inflight" });
  try {
    const res = await fetch(entry.operation.url, {
      method: entry.operation.method,
      headers: {
        "Content-Type": "application/json",
        "Idempotency-Key": entry.id      // send key to server
      },
      body: JSON.stringify(entry.operation.body)
    });

    if (res.ok || isNonRetryable(res.status)) {
      await db.delete("entries", entry.id);  // success or permanent failure
    } else {
      await markForRetry(entry);
    }
  } catch {
    await markForRetry(entry);             // network error — retry later
  }
}

function isNonRetryable(status: number) {
  return status >= 400 && status < 500 && status !== 429;
}
```

---

## 2. Idempotency Key Rules

- **Generate once, store immediately** — create the UUID _before_ the first attempt and persist it to `IndexedDB` in the same transaction as the queue entry
- **Never regenerate on retry** — the same key must be sent on every attempt for the same logical operation
- **Send as HTTP header** — `Idempotency-Key: <uuid>` (RFC draft standard; many APIs also accept it in the body)
- **Scope to the operation** — one key per user intent (e.g. one key for "create order #42"), not per HTTP call
- **Expiry** — after a definitive server response (success or non-retryable error), delete the entry; do not reuse keys

---

## 3. Exponential Back-off with Jitter

```ts
async function markForRetry(entry: QueueEntry) {
  const MAX_ATTEMPTS = 8;
  const BASE_DELAY_MS = 1_000;
  const MAX_DELAY_MS = 5 * 60_000;  // 5 minutes

  if (entry.attempts >= MAX_ATTEMPTS) {
    // Move to dead-letter: notify user, stop retrying
    await db.put("entries", { ...entry, status: "failed" });
    notifyUser("Some changes could not be saved. Please check your connection.");
    return;
  }

  const delay = Math.min(BASE_DELAY_MS * 2 ** entry.attempts, MAX_DELAY_MS);
  const jitter = Math.random() * delay * 0.25;  // ±25 % jitter
  const nextRetryAt = new Date(Date.now() + delay + jitter).toISOString();

  await db.put("entries", {
    ...entry,
    status: "pending",
    attempts: entry.attempts + 1,
    nextRetryAt
  });
}
```

---

## 4. Optimistic Updates

Show the user the expected result immediately; reconcile with the server response asynchronously.

```ts
// 1. Apply optimistic state
dispatch({ type: "ORDER_ADDED", payload: optimisticOrder });

// 2. Enqueue the write
const id = await enqueue({ operation: { method: "POST", url: "/api/orders", body: orderPayload } });

// 3. On server confirmation — replace optimistic entry with canonical one
//    (wire this up in your queue flush completion handler)
onQueueEntrySuccess(id, (serverOrder) => {
  dispatch({ type: "ORDER_CONFIRMED", payload: serverOrder });
});

// 4. On permanent failure — roll back
onQueueEntryFailed(id, () => {
  dispatch({ type: "ORDER_ROLLED_BACK", payload: optimisticOrder.id });
  notifyUser("Order could not be saved.");
});
```

**Rules:**
- Always give the optimistic item a local ID (e.g. `local_<uuid>`) so you can replace it on confirmation
- Never block the UI waiting for server confirmation
- Always implement the rollback path — skipping it is a bug

---

## 5. Background Sync (Service Worker)

For PWA targets, register a `sync` event to drain the queue when connectivity is restored:

```ts
// In service-worker.ts
self.addEventListener("sync", (event: SyncEvent) => {
  if (event.tag === "drain-write-queue") {
    event.waitUntil(flush());  // same flush() from above, running in SW context
  }
});
```

```ts
// In app bootstrap (client)
async function registerBackgroundSync() {
  if (!("serviceWorker" in navigator) || !("SyncManager" in window)) return;
  const reg = await navigator.serviceWorker.ready;
  await reg.sync.register("drain-write-queue");
}
```

**Fallback:** If `SyncManager` is unavailable (most desktop browsers), schedule `flush()` on:
- `window` `online` event
- `visibilitychange` to `visible`
- A 30-second polling interval while the tab is active

---

## 6. Page Reload Survival Checklist

- [ ] Queue entries are written to `IndexedDB` before the first network attempt
- [ ] The flush loop is started in app bootstrap (not only on user action)
- [ ] Idempotency keys are read from `IndexedDB`, not regenerated
- [ ] Optimistic UI state is rehydrated from `IndexedDB` on boot so in-flight items stay visible
- [ ] `background sync` or an `online` listener re-triggers `flush()` after reconnect
- [ ] Dead-letter entries surface a user-visible error with a manual retry affordance
