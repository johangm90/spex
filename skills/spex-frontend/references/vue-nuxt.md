# Vue 3 + Nuxt 3 Reference — spex-frontend

Deep patterns for Vue 3 Composition API, Pinia v2, Nuxt 3, TanStack Query (Vue adapter),
VeeValidate v4 + Zod, and Vitest + Vue Test Utils.

---

## 1. Project Structure

### Nuxt 3 (recommended default for Vue)
```
├── app.vue                     # Root component (or src/app.vue)
├── pages/                      # File-based routing — auto-imported
│   ├── index.vue
│   └── orders/
│       ├── index.vue
│       └── [id].vue
├── layouts/                    # Shared layouts
│   └── default.vue
├── components/                 # Auto-imported globally
│   └── ui/                     # Design-system primitives
├── composables/                # Auto-imported — useXxx convention
│   └── useOrders.ts
├── stores/                     # Pinia stores — NOT auto-imported (import manually)
│   └── cart.ts
├── server/                     # Nitro server routes (API)
│   └── api/
│       └── orders/
│           ├── index.get.ts
│           └── index.post.ts
├── features/                   # Complex features — opt-in, not auto-imported
│   └── orders/
│       ├── components/
│       ├── composables/
│       ├── services/
│       └── types.ts
├── plugins/                    # Nuxt plugins (run once on app init)
└── nuxt.config.ts
```

### Vue 3 + Vite SPA (no Nuxt)
```
src/
├── router/                     # vue-router v4
│   └── index.ts
├── stores/                     # Pinia stores
├── features/
├── components/ui/
├── composables/
└── main.ts                     # createApp + pinia + router
```

---

## 2. Composition API — Core Patterns

### Composable — encapsulate reactive logic
```ts
// composables/useOrders.ts  (auto-imported in Nuxt)
import { ref, computed } from "vue";
import { useQuery } from "@tanstack/vue-query";
import { fetchOrders, type Order } from "~/features/orders/services/ordersApi";

export function useOrders() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["orders"],
    queryFn: fetchOrders,
  });

  const pendingOrders = computed(() =>
    data.value?.filter((o) => o.status === "pending") ?? []
  );

  return { orders: data, pendingOrders, isPending, isError, error };
}
```

### Typed `ref` and `reactive`
```ts
// Prefer ref for primitives and nullable values
const count = ref<number>(0);
const selectedId = ref<string | null>(null);

// Use reactive for related grouped state (avoid for large objects — harder to destructure)
const form = reactive({ name: "", email: "" });

// Always type reactive arrays via generics
const items = ref<Order[]>([]);
```

### `watch` vs `watchEffect`
```ts
// watch — explicit source, access old + new value
watch(selectedId, (newId, oldId) => {
  if (newId !== oldId) loadDetail(newId);
});

// watchEffect — auto-tracks all reactive reads inside
watchEffect(() => {
  document.title = `${count.value} orders pending`;
});

// Always stop watchers created outside setup (e.g. in a plugin)
const stop = watchEffect(() => { /* ... */ });
onUnmounted(stop);
```

### `provide` / `inject` — dependency injection
```ts
// Parent (e.g. layout or feature root)
import { provide, ref } from "vue";
const theme = ref<"light" | "dark">("light");
provide("theme", theme);

// Child — typed inject
import { inject, type Ref } from "vue";
const theme = inject<Ref<"light" | "dark">>("theme");
if (!theme) throw new Error("theme not provided");
```

---

## 3. TanStack Query (Vue adapter)

### Setup — Nuxt plugin
```ts
// plugins/vue-query.client.ts
import { VueQueryPlugin, QueryClient } from "@tanstack/vue-query";

export default defineNuxtPlugin((nuxtApp) => {
  const qc = new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 60_000,
        retry: (count, err) => count < 2 && (err as any)?.status !== 404,
      },
    },
  });
  nuxtApp.vueApp.use(VueQueryPlugin, { queryClient: qc });
});
```

### useQuery — list + detail
```ts
// composables/useOrderDetail.ts
import { useQuery } from "@tanstack/vue-query";
import type { MaybeRef } from "@tanstack/vue-query";

export function useOrderDetail(id: MaybeRef<string>) {
  return useQuery({
    queryKey: computed(() => ["orders", "detail", unref(id)]),
    queryFn: () => fetchOrderById(unref(id)),
    enabled: computed(() => !!unref(id)),
  });
}
```

### useMutation — optimistic update
```ts
import { useMutation, useQueryClient } from "@tanstack/vue-query";
import type { Order } from "~/features/orders/types";

export function useCreateOrder() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (payload: CreateOrderPayload) => createOrder(payload),
    onMutate: async (payload) => {
      await qc.cancelQueries({ queryKey: ["orders"] });
      const prev = qc.getQueryData<Order[]>(["orders"]);
      qc.setQueryData<Order[]>(["orders"], (old = []) => [
        ...old,
        { id: `local_${crypto.randomUUID()}`, status: "pending", ...payload },
      ]);
      return { prev };
    },
    onError: (_err, _vars, ctx) => {
      qc.setQueryData(["orders"], ctx?.prev);
    },
    onSettled: () => qc.invalidateQueries({ queryKey: ["orders"] }),
  });
}
```

---

## 4. Pinia v2 — State Management

### Setup store (recommended — uses Composition API syntax)
```ts
// stores/cart.ts
import { defineStore } from "pinia";
import { ref, computed } from "vue";

interface CartItem { id: string; name: string; qty: number; price: number; }

export const useCartStore = defineStore("cart", () => {
  const items = ref<CartItem[]>([]);

  const total = computed(() =>
    items.value.reduce((sum, i) => sum + i.price * i.qty, 0)
  );

  function add(item: CartItem) {
    const existing = items.value.find((i) => i.id === item.id);
    if (existing) { existing.qty += item.qty; } else { items.value.push(item); }
  }

  function remove(id: string) {
    items.value = items.value.filter((i) => i.id !== id);
  }

  function clear() { items.value = []; }

  return { items, total, add, remove, clear };
}, {
  persist: true,  // requires @pinia-plugin-persistedstate
});
```

### Options store (for simpler cases)
```ts
export const useUiStore = defineStore("ui", {
  state: () => ({ sidebarOpen: false, theme: "light" as "light" | "dark" }),
  getters: {
    isDark: (s) => s.theme === "dark",
  },
  actions: {
    toggleSidebar() { this.sidebarOpen = !this.sidebarOpen; },
    setTheme(t: "light" | "dark") { this.theme = t; },
  },
});
```

### Using a store in a component
```vue
<script setup lang="ts">
import { useCartStore } from "~/stores/cart";
import { storeToRefs } from "pinia";

const cart = useCartStore();
// storeToRefs preserves reactivity when destructuring
const { items, total } = storeToRefs(cart);
// actions can be destructured directly (they are not reactive values)
const { add, remove, clear } = cart;
</script>
```

### Testing Pinia stores
```ts
import { setActivePinia, createPinia } from "pinia";
import { useCartStore } from "~/stores/cart";

beforeEach(() => setActivePinia(createPinia()));

test("add item increases total", () => {
  const cart = useCartStore();
  cart.add({ id: "p1", name: "Widget", qty: 2, price: 10 });
  expect(cart.total).toBe(20);
});
```

---

## 5. Nuxt 3 — Key Patterns

### Data fetching — `useFetch` vs `useAsyncData`
```ts
// useFetch — convenience wrapper; auto-generates key from URL
const { data: orders, status, error } = await useFetch<Order[]>("/api/orders");

// useAsyncData — full control; use when calling composables or external APIs
const { data: orders } = await useAsyncData("orders", () => fetchOrders());

// Lazy (non-blocking — component renders while data loads)
const { data, pending } = useLazyFetch<Order[]>("/api/orders");
```

> **Rule:** prefer `useAsyncData` + service functions over `useFetch` with raw URLs for testability.

### Server routes (Nitro)
```ts
// server/api/orders/index.get.ts
import { z } from "zod";

export default defineEventHandler(async (event) => {
  const query = getQuery(event);
  // ... fetch from DB
  return orders;
});

// server/api/orders/index.post.ts
const CreateOrderSchema = z.object({
  productId: z.string().uuid(),
  quantity: z.number().int().min(1),
});

export default defineEventHandler(async (event) => {
  const body = await readBody(event);
  const result = CreateOrderSchema.safeParse(body);
  if (!result.success) {
    throw createError({ statusCode: 422, data: result.error.flatten() });
  }
  // ... persist
  setResponseStatus(event, 201);
  return order;
});
```

### Middleware — auth guard
```ts
// middleware/auth.ts  (runs before every route by default when named "auth")
export default defineNuxtRouteMiddleware((to) => {
  const { loggedIn } = useAuth();  // custom composable or nuxt-auth-utils
  if (!loggedIn.value) {
    return navigateTo({ path: "/login", query: { redirect: to.fullPath } });
  }
});
```

```vue
<!-- Apply to a specific page -->
<script setup>
definePageMeta({ middleware: "auth" });
</script>
```

### Layouts
```vue
<!-- layouts/dashboard.vue -->
<template>
  <div class="dashboard-layout">
    <AppSidebar />
    <main><slot /></main>
  </div>
</template>

<!-- pages/orders/index.vue -->
<script setup>
definePageMeta({ layout: "dashboard" });
</script>
```

### Environment variables (Nuxt)
```ts
// nuxt.config.ts
export default defineNuxtConfig({
  runtimeConfig: {
    // Private (server-only)
    databaseUrl: process.env.DATABASE_URL,
    // Public (exposed to client)
    public: {
      apiBase: process.env.NUXT_PUBLIC_API_BASE ?? "http://localhost:3000",
    },
  },
});

// In code
const config = useRuntimeConfig();
console.log(config.public.apiBase);  // client + server
console.log(config.databaseUrl);     // server only
```

---

## 6. VeeValidate v4 + Zod

### `useForm` with Zod schema
```vue
<script setup lang="ts">
import { useForm } from "vee-validate";
import { toTypedSchema } from "@vee-validate/zod";
import { z } from "zod";

const schema = z.object({
  email:    z.string().email("Invalid email"),
  quantity: z.coerce.number().int().min(1).max(99),
});

const { handleSubmit, errors, defineField } = useForm({
  validationSchema: toTypedSchema(schema),
});

const [email, emailAttrs]       = defineField("email");
const [quantity, quantityAttrs] = defineField("quantity");

const onSubmit = handleSubmit(async (values) => {
  await createOrder(values);
});
</script>

<template>
  <form @submit="onSubmit" novalidate>
    <div>
      <label for="email">Email</label>
      <input id="email" v-model="email" v-bind="emailAttrs"
             type="email" :aria-describedby="errors.email ? 'email-error' : undefined" />
      <p v-if="errors.email" id="email-error" role="alert">{{ errors.email }}</p>
    </div>
    <button type="submit">Submit</button>
  </form>
</template>
```

### Field-level validation
```vue
<script setup lang="ts">
import { useField } from "vee-validate";
import { z } from "zod";
import { toTypedSchema } from "@vee-validate/zod";

const { value, errorMessage } = useField(
  "username",
  toTypedSchema(z.string().min(3))
);
</script>
```

---

## 7. Testing — Vitest + Vue Test Utils

See `references/testing-a11y.md` for full patterns. Quick reference:

### Component test
```ts
// features/orders/components/__tests__/OrderList.test.ts
import { mount } from "@vue/test-utils";
import { createTestingPinia } from "@pinia/testing";
import OrderList from "../OrderList.vue";

test("renders order list", () => {
  const wrapper = mount(OrderList, {
    global: {
      plugins: [createTestingPinia({
        initialState: { orders: { items: [{ id: "1", status: "pending" }] } },
      })],
    },
  });
  expect(wrapper.findAll("[data-testid='order-row']")).toHaveLength(1);
});
```

### Composable test
```ts
// composables/__tests__/useOrders.test.ts
import { mount } from "@vue/test-utils";
import { VueQueryPlugin, QueryClient } from "@tanstack/vue-query";
import { vi } from "vitest";
import * as api from "~/features/orders/services/ordersApi";

vi.mock("~/features/orders/services/ordersApi");

test("useOrders fetches and returns orders", async () => {
  vi.mocked(api.fetchOrders).mockResolvedValue([{ id: "1", status: "pending", total: 50 }]);

  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = mount(defineComponent({
    setup() { return useOrders(); },
    template: "<div />",
  }), { global: { plugins: [[VueQueryPlugin, { queryClient: qc }]] } });

  await flushPromises();
  expect(wrapper.vm.orders).toHaveLength(1);
});
```

---

## 8. Common Pitfalls

| Pitfall | Fix |
|---------|-----|
| Destructuring reactive object loses reactivity | Use `storeToRefs` (Pinia) or `toRefs` (reactive object) |
| `watch` not triggering on nested object mutation | Use `{ deep: true }` or watch a computed that returns the specific property |
| `async setup()` without `<Suspense>` | Nuxt handles this via `useAsyncData`; in plain Vue 3 wrap with `<Suspense>` |
| `useFetch` key collision (same URL, different components) | Provide explicit key: `useFetch("/api/orders", { key: "orders-list" })` |
| Pinia store shared across SSR requests | Nuxt auto-handles this; in plain Node SSR, create one Pinia instance per request |
| `defineProps` type not matching `v-model` | Use `defineModel()` (Vue 3.4+) for two-way binding in child components |
| `ref` in `<template>` without `.value` | Template auto-unwraps `ref` — `.value` is only needed in `<script setup>` |
| Missing `await` before `useFetch` in Nuxt | Without `await`, SSR won't wait for data and hydration will mismatch |
