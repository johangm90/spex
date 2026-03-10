# Domain Modeling Reference — spex-architect

Patterns for identifying bounded contexts, defining domain events, and running lightweight event-storming sessions. Use this when starting a new project or when a feature touches multiple domain areas.

---

## Bounded Context Identification

A bounded context is a **named, non-overlapping region of the domain** where a specific model applies and a ubiquitous language is consistent.

### Identification heuristics

| Heuristic | Example |
|-----------|---------|
| **Team ownership** — one team owns one context | Payments team owns `Billing` context |
| **Database boundary** — separate schema = separate context | Orders DB ≠ Inventory DB |
| **Ubiquitous language divergence** — same word means different things | "Product" in `Catalog` (rich content) vs. `Inventory` (stock unit) vs. `Billing` (line item) |
| **Different lifecycle** — entities change at different rates | User profile (slow) vs. Session (fast) → separate contexts |
| **Regulatory / compliance boundary** — data that must stay isolated | PII in `Identity` context; payment data in `Billing` context |

### Common bounded contexts (e-commerce reference)

| Context | Responsibilities | Key entities |
|---------|-----------------|-------------|
| **Identity** | Registration, login, password, OAuth, roles | `User`, `Session`, `Role` |
| **Catalog** | Products, categories, search, media | `Product`, `Category`, `Media` |
| **Inventory** | Stock levels, warehouse locations, reservations | `StockItem`, `Reservation` |
| **Orders** | Cart, checkout, order lifecycle, returns | `Order`, `OrderLine`, `Cart` |
| **Billing** | Invoices, payments, refunds, subscriptions | `Invoice`, `Payment`, `Subscription` |
| **Notifications** | Email, push, SMS dispatch | `Notification`, `Template`, `Channel` |
| **Shipping** | Carriers, tracking, fulfillment | `Shipment`, `TrackingEvent` |

### Context mapping patterns

| Pattern | When to use |
|---------|-------------|
| **Shared Kernel** | Two contexts share a small, agreed-upon model (e.g. `UserId` type) — minimise |
| **Customer/Supplier** | Upstream context (supplier) publishes events; downstream (customer) consumes them |
| **Anti-Corruption Layer (ACL)** | Downstream context translates upstream model into its own language |
| **Published Language** | Shared API schema / event format agreed by all consumers |
| **Conformist** | Downstream adopts upstream model as-is (use only when integration cost > translation cost) |

---

## Domain Event Catalog

Domain events are **facts that happened** in the domain. They cross context boundaries and drive integration.

### Naming convention

- Past tense, descriptive: `OrderPlaced`, `PaymentFailed`, `InventoryReserved`
- No verbs in present tense: not `PlaceOrder` (that's a command)
- Prefix with context name when ambiguous: `Billing.PaymentFailed`

### Event envelope (canonical structure)

```json
{
  "id":         "evt-a1b2c3d4",
  "type":       "OrderPlaced",
  "version":    "1",
  "source":     "orders-service",
  "occurred_at":"2026-03-10T14:32:00Z",
  "aggregate":  { "type": "Order", "id": "ord-9876" },
  "payload": {
    "customer_id":   "usr-1234",
    "total_amount":  149.99,
    "currency":      "EUR",
    "line_count":    3
  },
  "correlation_id": "req-xyz",
  "causation_id":   "cmd-abc"
}
```

### Event catalog template (per slice)

| Event | Source context | Consumer contexts | Payload (key fields) |
|-------|---------------|------------------|---------------------|
| `OrderPlaced` | Orders | Inventory, Billing, Notifications | `order_id`, `customer_id`, `lines[]` |
| `PaymentFailed` | Billing | Orders, Notifications | `order_id`, `reason`, `amount` |
| `InventoryReserved` | Inventory | Orders | `order_id`, `reservation_id` |
| `ShipmentDispatched` | Shipping | Orders, Notifications | `order_id`, `tracking_number` |

---

## Lightweight Event Storming

Run a 30-minute session with the human when starting a new bounded context. Walk through these steps in order.

### Step 1 — Domain events (orange stickies)

Ask: _"What significant things can happen in this system?"_

Write each as a past-tense fact:
```
UserRegistered  →  EmailVerified  →  ProfileCompleted  →  AccountSuspended
```

### Step 2 — Commands (blue stickies)

For each event, ask: _"What caused this to happen?"_

```
RegisterUser  →  UserRegistered
VerifyEmail   →  EmailVerified
SuspendAccount → AccountSuspended
```

### Step 3 — Aggregates (yellow stickies)

Group commands + events around the entity that enforces the business rules:

```
[User aggregate]
  RegisterUser → UserRegistered
  VerifyEmail  → EmailVerified
  SuspendAccount → AccountSuspended
```

### Step 4 — Bounded context boundaries

Draw a box around clusters of aggregates that share a ubiquitous language and team ownership. Each box is a bounded context.

### Step 5 — Integration events

Identify events that cross context boundaries — these become the integration event catalog in the slice spec.

---

## Ubiquitous Language Glossary Template

Write a glossary in `docs/PRD.md` under a `## Domain Glossary` heading:

```markdown
## Domain Glossary

| Term | Context | Definition |
|------|---------|-----------|
| **Product** | Catalog | A sellable item with rich content (name, description, images, variants) |
| **Product** | Inventory | A stock-keeping unit (SKU) tracked by quantity |
| **Product** | Billing | A line item on an invoice with a price |
| **Order** | Orders | A confirmed purchase request from a customer |
| **Cart** | Orders | A temporary, unconfirmed collection of items before checkout |
| **Customer** | Identity | A registered user with billing and shipping information |
```

> **Never use the same term to mean different things within one bounded context.** If a term is overloaded, it is a signal that two contexts are bleeding together — split them.

---

## Anti-Corruption Layer Pattern (PHP / Symfony)

When consuming events from an upstream context, translate the external model into your context's language:

```php
// src/Orders/Infrastructure/ACL/BillingEventTranslator.php
namespace App\Orders\Infrastructure\ACL;

use App\Orders\Domain\Event\PaymentConfirmed;

final class BillingEventTranslator
{
    /**
     * Translate a raw Billing.PaymentSucceeded event payload
     * into the Orders context's PaymentConfirmed domain event.
     */
    public function translate(array $billingPayload): PaymentConfirmed
    {
        return new PaymentConfirmed(
            orderId:    $billingPayload['reference_id'],   // Billing calls it reference_id
            amount:     (float) $billingPayload['charged_amount'],
            currency:   $billingPayload['currency_code'],
            confirmedAt: new \DateTimeImmutable($billingPayload['processed_at']),
        );
    }
}
```

> The ACL isolates the Orders context from upstream model changes. If Billing renames `reference_id` to `order_reference`, only this translator needs updating.

---

## Slice Decomposition Patterns

### By capability layer (horizontal — avoid this)

```
SLICE-001: Database schema for orders       ← NOT a shippable increment
SLICE-002: API endpoints for orders
SLICE-003: UI for orders
```

### By user scenario (vertical — preferred)

```
SLICE-001: Customer can place a single-item order (happy path)
SLICE-002: Customer can add multiple items to cart and checkout
SLICE-003: Customer can apply a discount code at checkout
SLICE-004: Customer can view order history
```

Each vertical slice delivers a **shippable user-facing outcome** and touches all layers (schema + API + UI) needed for that specific scenario.

### By event flow (event-driven systems)

```
SLICE-001: OrderPlaced triggers inventory reservation (Orders → Inventory integration)
SLICE-002: InventoryReserved triggers payment capture (Inventory → Billing integration)
SLICE-003: PaymentFailed triggers order cancellation and stock release
```

Each slice owns one integration path end-to-end.
