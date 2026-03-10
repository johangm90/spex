# React + Next.js Reference — spex-frontend

Deep patterns for React 18+, Next.js 14+ App Router, TanStack Query v5, Zustand v4,
React Hook Form v7 + Zod, and Vitest + Testing Library.

---

## 1. Project Structure

### Next.js App Router (recommended default)
```
src/
├── app/                        # App Router: routes, layouts, server components
│   ├── layout.tsx              # Root layout — providers go here
│   ├── page.tsx                # Home route
│   └── orders/
│       ├── layout.tsx          # Nested layout (optional)
│       ├── page.tsx            # Server Component — fetch + render
│       └── [id]/
│           └── page.tsx
├── features/                   # Feature-first organization
│   └── orders/
│       ├── components/         # UI components (smart + presentational)
│       ├── hooks/              # Custom React hooks
│       ├── services/           # Typed API calls
│       ├── store/              # Zustand slices
│       ├── types.ts            # Domain types from contract schema
│       └── __tests__/
├── components/                 # Shared / design-system components
│   └── ui/                     # shadcn/ui or custom primitives
├── lib/                        # Singleton clients (queryClient, axios instance)
│   ├── queryClient.ts
│   └── apiClient.ts
└── providers.tsx               # QueryClientProvider + other root providers
```

### React + Vite SPA (no Next.js)
```
src/
├── routes/                     # react-router-dom v6 route components
├── features/                   # Same feature-first structure
├── components/ui/
├── lib/
└── main.tsx                    # App entry: BrowserRouter + providers
```

---

## 2. TanStack Query v5 — Server State

### QueryClient setup
```ts
// src/lib/queryClient.ts
import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 60_000,          // 1 min — avoid re-fetching on every mount
      gcTime: 5 * 60_000,         // 5 min garbage collection
      retry: (count, err) =>
        count < 2 && (err as any)?.status !== 404,
    },
  },
});
```

```tsx
// src/providers.tsx  ("use client")
"use client";
import { QueryClientProvider } from "@tanstack/react-query";
import { ReactQueryDevtools } from "@tanstack/react-query-devtools";
import { queryClient } from "@/lib/queryClient";

export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>
      {children}
      <ReactQueryDevtools initialIsOpen={false} />
    </QueryClientProvider>
  );
}
```

### useQuery — typed fetch
```tsx
import { useQuery } from "@tanstack/react-query";
import { fetchOrders, Order } from "../services/ordersApi";

export function useOrders() {
  return useQuery<Order[], Error>({
    queryKey: ["orders"],
    queryFn: fetchOrders,
  });
}

// In component
function OrderList() {
  const { data: orders, isPending, isError, error } = useOrders();
  if (isPending) return <Skeleton />;
  if (isError) return <ErrorBanner message={error.message} />;
  return <ul>{orders.map((o) => <OrderItem key={o.id} order={o} />)}</ul>;
}
```

### useMutation — optimistic update
```tsx
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createOrder, CreateOrderPayload, Order } from "../services/ordersApi";

export function useCreateOrder() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (payload: CreateOrderPayload) => createOrder(payload),
    onMutate: async (payload) => {
      // 1. Cancel in-flight refetches to avoid overwriting optimistic update
      await qc.cancelQueries({ queryKey: ["orders"] });
      // 2. Snapshot previous value for rollback
      const prev = qc.getQueryData<Order[]>(["orders"]);
      // 3. Apply optimistic update
      qc.setQueryData<Order[]>(["orders"], (old = []) => [
        ...old,
        { id: `local_${crypto.randomUUID()}`, status: "pending", ...payload },
      ]);
      return { prev };
    },
    onError: (_err, _vars, ctx) => {
      // Roll back on failure
      qc.setQueryData(["orders"], ctx?.prev);
    },
    onSettled: () => {
      // Always refetch to sync with server truth
      qc.invalidateQueries({ queryKey: ["orders"] });
    },
  });
}
```

### Query key conventions
```ts
// Centralize query keys to avoid typos
export const orderKeys = {
  all: ["orders"] as const,
  list: (filters?: OrderFilters) => ["orders", "list", filters] as const,
  detail: (id: string) => ["orders", "detail", id] as const,
};
```

### Prefetching in Next.js Server Components
```tsx
// app/orders/page.tsx  — runs on server, prefills cache for client
import { HydrationBoundary, dehydrate } from "@tanstack/react-query";
import { queryClient } from "@/lib/queryClient";

export default async function OrdersPage() {
  await queryClient.prefetchQuery({
    queryKey: orderKeys.all,
    queryFn: fetchOrders,
  });
  return (
    <HydrationBoundary state={dehydrate(queryClient)}>
      <OrderListClient />
    </HydrationBoundary>
  );
}
```

---

## 3. Zustand v4 — UI / Global State

### Slice pattern with devtools + persist
```ts
// src/features/cart/store/cartStore.ts
import { create } from "zustand";
import { devtools, persist } from "zustand/middleware";

interface CartItem { id: string; name: string; qty: number; price: number; }
interface CartState {
  items: CartItem[];
  add: (item: CartItem) => void;
  updateQty: (id: string, qty: number) => void;
  remove: (id: string) => void;
  clear: () => void;
  total: () => number;
}

export const useCartStore = create<CartState>()(
  devtools(
    persist(
      (set, get) => ({
        items: [],
        add: (item) =>
          set((s) => ({ items: [...s.items, item] }), false, "cart/add"),
        updateQty: (id, qty) =>
          set(
            (s) => ({ items: s.items.map((i) => (i.id === id ? { ...i, qty } : i)) }),
            false,
            "cart/updateQty"
          ),
        remove: (id) =>
          set((s) => ({ items: s.items.filter((i) => i.id !== id) }), false, "cart/remove"),
        clear: () => set({ items: [] }, false, "cart/clear"),
        total: () => get().items.reduce((sum, i) => sum + i.price * i.qty, 0),
      }),
      { name: "cart-storage" }
    ),
    { name: "CartStore" }
  )
);
```

### Selector pattern — avoid unnecessary re-renders
```ts
// Never subscribe to the whole store in a component
const items = useCartStore((s) => s.items);        // re-renders only when items change
const total = useCartStore((s) => s.total());       // derived value
const add   = useCartStore((s) => s.add);           // stable reference — no re-render
```

### Combining Zustand + TanStack Query
- **TanStack Query** owns server state (orders, products, user profile)
- **Zustand** owns ephemeral UI state (selected filters, open panels, cart)
- Never duplicate server data in Zustand; read it via `useQuery` in components

---

## 4. React Hook Form v7 + Zod

### Basic form with error display
```tsx
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

const schema = z.object({
  name:     z.string().min(2, "Name must be at least 2 characters"),
  email:    z.string().email("Invalid email address"),
  quantity: z.coerce.number().int().min(1).max(99),
});
type FormData = z.infer<typeof schema>;

export function OrderForm({ onSuccess }: { onSuccess: () => void }) {
  const { register, handleSubmit, formState: { errors, isSubmitting } } = useForm<FormData>({
    resolver: zodResolver(schema),
  });
  const createOrder = useCreateOrder();

  const onSubmit = handleSubmit(async (data) => {
    await createOrder.mutateAsync(data);
    onSuccess();
  });

  return (
    <form onSubmit={onSubmit} noValidate>
      <div>
        <label htmlFor="name">Name</label>
        <input id="name" {...register("name")} aria-describedby="name-error" />
        {errors.name && (
          <p id="name-error" role="alert">{errors.name.message}</p>
        )}
      </div>
      <div>
        <label htmlFor="email">Email</label>
        <input id="email" type="email" {...register("email")} aria-describedby="email-error" />
        {errors.email && (
          <p id="email-error" role="alert">{errors.email.message}</p>
        )}
      </div>
      <button type="submit" disabled={isSubmitting}>
        {isSubmitting ? "Submitting…" : "Create Order"}
      </button>
    </form>
  );
}
```

### Multi-step form with `useFormContext`
```tsx
const FormContext = createContext<ReturnType<typeof useForm<FormData>> | null>(null);

function MultiStepForm() {
  const methods = useForm<FormData>({ resolver: zodResolver(schema) });
  return (
    <FormProvider {...methods}>
      <Step1 />
      <Step2 />
    </FormProvider>
  );
}

function Step1() {
  const { register, formState: { errors } } = useFormContext<FormData>();
  // ...
}
```

---

## 5. Next.js App Router — Key Patterns

### Server vs Client Component rules
| Needs | Component type |
|-------|---------------|
| DB / API fetch at render time | **Server Component** (default — no directive needed) |
| `useState`, `useEffect`, event handlers | **Client Component** (`"use client"` at top of file) |
| Third-party library with browser APIs | **Client Component** |
| Wraps client children with data | **Server Component** passing props to client children |

```
Server Component → passes data as props → Client Component (boundary)
                                         ↓
                                  can import other Server or Client Components
```

### Route handlers (API routes in App Router)
```ts
// app/api/orders/route.ts
import { NextRequest, NextResponse } from "next/server";
import { z } from "zod";

const CreateOrderSchema = z.object({
  productId: z.string().uuid(),
  quantity: z.number().int().min(1),
});

export async function POST(req: NextRequest) {
  const body = CreateOrderSchema.safeParse(await req.json());
  if (!body.success) {
    return NextResponse.json({ errors: body.error.flatten() }, { status: 422 });
  }
  // ... persist
  return NextResponse.json(order, { status: 201 });
}
```

### Streaming with Suspense
```tsx
// app/dashboard/page.tsx
import { Suspense } from "react";

export default function DashboardPage() {
  return (
    <div>
      <h1>Dashboard</h1>
      <Suspense fallback={<MetricsSkeleton />}>
        <MetricsPanel />       {/* async Server Component — streams in */}
      </Suspense>
      <Suspense fallback={<OrdersSkeleton />}>
        <RecentOrders />
      </Suspense>
    </div>
  );
}
```

### Metadata API
```ts
// app/orders/page.tsx
import type { Metadata } from "next";
export const metadata: Metadata = {
  title: "Orders",
  description: "Manage your orders",
};
```

### Environment variables
```ts
// Server-only (never exposed to browser)
process.env.DATABASE_URL

// Exposed to browser (must be prefixed NEXT_PUBLIC_)
process.env.NEXT_PUBLIC_API_URL
```

---

## 6. URL State — nuqs (Next.js)

Use `nuqs` to store shareable UI state (filters, pagination, tabs) in the URL:

```ts
import { useQueryState, parseAsInteger, parseAsString } from "nuqs";

function OrderFilters() {
  const [status, setStatus] = useQueryState("status", parseAsString.withDefault("all"));
  const [page, setPage]     = useQueryState("page",   parseAsInteger.withDefault(1));

  return (
    <select value={status} onChange={(e) => setStatus(e.target.value)}>
      <option value="all">All</option>
      <option value="pending">Pending</option>
    </select>
  );
}
```

---

## 7. Performance Patterns

### Code splitting — lazy load heavy components
```tsx
import { lazy, Suspense } from "react";
const HeavyChart = lazy(() => import("./HeavyChart"));

function Dashboard() {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <HeavyChart />
    </Suspense>
  );
}
```

### Memoization — when it pays
```tsx
// useMemo: expensive derived computation
const sortedOrders = useMemo(
  () => [...orders].sort((a, b) => b.createdAt.localeCompare(a.createdAt)),
  [orders]
);

// useCallback: stable callback reference passed to memoized child
const handleDelete = useCallback((id: string) => {
  deleteOrder.mutate(id);
}, [deleteOrder]);

// React.memo: prevent re-render when props are reference-equal
const OrderRow = React.memo(({ order }: { order: Order }) => { /* ... */ });
```

**Memoization anti-patterns to avoid:**
- Wrapping every component in `memo` — measure first with React DevTools Profiler
- `useCallback` on callbacks not passed as props to memoized children
- `useMemo` for cheap computations (array.filter on < 100 items)

### Virtual lists — large data sets
```tsx
import { useVirtualizer } from "@tanstack/react-virtual";

function VirtualOrderList({ orders }: { orders: Order[] }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: orders.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 56,      // row height in px
    overscan: 5,
  });

  return (
    <div ref={parentRef} style={{ height: "600px", overflowY: "auto" }}>
      <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
        {virtualizer.getVirtualItems().map((vItem) => (
          <div key={vItem.key} style={{ position: "absolute", top: vItem.start, width: "100%" }}>
            <OrderRow order={orders[vItem.index]} />
          </div>
        ))}
      </div>
    </div>
  );
}
```

---

## 8. Testing — Vitest + React Testing Library

See `references/testing-a11y.md` for full patterns. Quick reference:

### Unit test — custom hook
```ts
// src/features/orders/hooks/__tests__/useOrders.test.ts
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientWrapper } from "@/test/utils";
import { useOrders } from "../useOrders";

vi.mock("../services/ordersApi", () => ({
  fetchOrders: vi.fn().mockResolvedValue([{ id: "1", status: "pending", total: 100 }]),
}));

test("returns orders after loading", async () => {
  const { result } = renderHook(() => useOrders(), { wrapper: QueryClientWrapper });
  await waitFor(() => expect(result.current.isSuccess).toBe(true));
  expect(result.current.data).toHaveLength(1);
});
```

### Component test — user interaction
```tsx
// src/features/orders/components/__tests__/OrderForm.test.tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { OrderForm } from "../OrderForm";

test("shows validation error for invalid email", async () => {
  const user = userEvent.setup();
  render(<OrderForm onSuccess={vi.fn()} />);

  await user.type(screen.getByLabelText("Email"), "not-an-email");
  await user.click(screen.getByRole("button", { name: /create order/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent("Invalid email");
});
```

### Test utility — QueryClientWrapper
```tsx
// src/test/utils.tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type PropsWithChildren } from "react";

export function QueryClientWrapper({ children }: PropsWithChildren) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}
```

---

## 9. Common Pitfalls

| Pitfall | Fix |
|---------|-----|
| `useEffect` for data fetching | Use `useQuery` — handles loading, error, caching |
| `useEffect` with missing dependencies | Fix deps or extract to a custom hook |
| Stale closure in event handler | `useCallback` with correct deps, or read from ref |
| Server Component importing a Client Component that uses `useContext` | Add `"use client"` to the context consumer, not the provider |
| Hydration mismatch (SSR vs client) | Ensure server and client render the same initial HTML; use `suppressHydrationWarning` only for timestamps |
| `key` on wrong element in list | `key` goes on the outermost element returned by `.map()`, not a child |
| Zustand store reset between tests | Create a new store instance per test; don't share module-level singletons |
| `any` cast on API response | Use `z.safeParse()` with proper error handling |
