# Testing & Accessibility Reference — spex-frontend

Patterns for Vitest + Testing Library (React and Vue), Playwright E2E, ARIA roles,
keyboard navigation, and axe-core accessibility auditing.

---

## 1. Vitest Setup

### `vite.config.ts` / `vitest.config.ts`
```ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";   // or vue()

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,                  // removes need to import describe/test/expect
    setupFiles: ["./src/test/setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      exclude: ["src/test/**", "**/*.d.ts"],
    },
  },
});
```

### Setup file
```ts
// src/test/setup.ts
import "@testing-library/jest-dom";     // adds toBeInTheDocument, toHaveTextContent, etc.
import { cleanup } from "@testing-library/react";  // or @testing-library/vue
import { afterEach } from "vitest";

afterEach(() => {
  cleanup();                            // unmount components after each test
  vi.resetAllMocks();                   // clear mock call history
});
```

---

## 2. React Testing Library — Patterns

### Render helpers
```tsx
// src/test/utils.tsx — shared render wrapper
import { render, type RenderOptions } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { type PropsWithChildren } from "react";

function AllProviders({ children }: PropsWithChildren) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <MemoryRouter>
      <QueryClientProvider client={qc}>{children}</QueryClientProvider>
    </MemoryRouter>
  );
}

export function renderWithProviders(ui: React.ReactElement, opts?: RenderOptions) {
  return render(ui, { wrapper: AllProviders, ...opts });
}
```

### Query priority (prefer accessible queries)
```
getByRole        ← prefer — finds by ARIA role; most resilient to refactors
getByLabelText   ← forms — finds by associated <label>
getByPlaceholderText ← fallback for inputs without label
getByText        ← for static content
getByTestId      ← last resort — requires data-testid attribute
```

```tsx
// Good
screen.getByRole("button", { name: /submit order/i });
screen.getByLabelText("Email address");

// Avoid unless no accessible alternative exists
screen.getByTestId("submit-btn");
```

### Async queries
```tsx
// findBy* — returns a Promise; use for elements that appear after async operations
const submitBtn = await screen.findByRole("button", { name: /order created/i });

// waitFor — for assertions that need to become true over time
await waitFor(() => {
  expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
});
```

### User events (prefer over `fireEvent`)
```tsx
import userEvent from "@testing-library/user-event";

test("submits form", async () => {
  const user = userEvent.setup();
  renderWithProviders(<OrderForm onSuccess={vi.fn()} />);

  await user.type(screen.getByLabelText("Email"), "test@example.com");
  await user.selectOptions(screen.getByRole("combobox", { name: "Status" }), "pending");
  await user.click(screen.getByRole("button", { name: /create order/i }));

  expect(await screen.findByText(/order created/i)).toBeInTheDocument();
});
```

### Mocking API calls
```tsx
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";

const server = setupServer(
  http.get("/api/orders", () =>
    HttpResponse.json([{ id: "1", status: "pending", total: 100 }])
  ),
  http.post("/api/orders", () =>
    HttpResponse.json({ id: "2", status: "pending" }, { status: 201 })
  )
);

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

test("renders orders from API", async () => {
  renderWithProviders(<OrderList />);
  expect(await screen.findByText("Order #1")).toBeInTheDocument();
});

// Override for an error case
test("shows error state", async () => {
  server.use(http.get("/api/orders", () => HttpResponse.error()));
  renderWithProviders(<OrderList />);
  expect(await screen.findByRole("alert")).toHaveTextContent(/failed to load/i);
});
```

### Testing a hook in isolation
```tsx
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientWrapper } from "./utils";

test("useOrders returns data", async () => {
  const { result } = renderHook(() => useOrders(), { wrapper: QueryClientWrapper });
  expect(result.current.isPending).toBe(true);
  await waitFor(() => expect(result.current.isSuccess).toBe(true));
  expect(result.current.data).toHaveLength(1);
});
```

---

## 3. Vue Test Utils — Patterns

### Render with plugins
```ts
import { mount, flushPromises } from "@vue/test-utils";
import { createTestingPinia } from "@pinia/testing";
import { VueQueryPlugin, QueryClient } from "@tanstack/vue-query";

function makeQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

export function mountWithPlugins(component: any, options = {}) {
  return mount(component, {
    global: {
      plugins: [
        createTestingPinia({ createSpy: vi.fn }),
        [VueQueryPlugin, { queryClient: makeQueryClient() }],
      ],
    },
    ...options,
  });
}
```

### Interacting with the DOM
```ts
test("clicking delete calls store action", async () => {
  const wrapper = mountWithPlugins(OrderRow, { props: { orderId: "1" } });
  await wrapper.find("[data-testid='delete-btn']").trigger("click");
  const store = useOrdersStore();
  expect(store.deleteOrder).toHaveBeenCalledWith("1");
});
```

### Testing emitted events
```ts
test("emits 'selected' when row is clicked", async () => {
  const wrapper = mountWithPlugins(OrderRow, { props: { order: mockOrder } });
  await wrapper.trigger("click");
  expect(wrapper.emitted("selected")).toBeTruthy();
  expect(wrapper.emitted("selected")![0]).toEqual([mockOrder.id]);
});
```

---

## 4. Playwright — E2E Patterns

### Configuration
```ts
// playwright.config.ts
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: [["html"], ["line"]],
  use: {
    baseURL: process.env.PLAYWRIGHT_BASE_URL ?? "http://localhost:3000",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "firefox",  use: { ...devices["Desktop Firefox"] } },
  ],
});
```

### Page Object Model (POM)
```ts
// e2e/pages/OrdersPage.ts
import { type Page, type Locator } from "@playwright/test";

export class OrdersPage {
  readonly page: Page;
  readonly createBtn: Locator;
  readonly emailInput: Locator;
  readonly submitBtn: Locator;
  readonly orderList: Locator;

  constructor(page: Page) {
    this.page = page;
    this.createBtn  = page.getByRole("button", { name: /create order/i });
    this.emailInput = page.getByLabel("Email");
    this.submitBtn  = page.getByRole("button", { name: /submit/i });
    this.orderList  = page.getByRole("list", { name: /orders/i });
  }

  async goto() {
    await this.page.goto("/orders");
  }

  async createOrder(email: string) {
    await this.createBtn.click();
    await this.emailInput.fill(email);
    await this.submitBtn.click();
  }
}
```

### Test with POM
```ts
// e2e/orders.spec.ts
import { test, expect } from "@playwright/test";
import { OrdersPage } from "./pages/OrdersPage";

test("user can create an order", async ({ page }) => {
  const ordersPage = new OrdersPage(page);
  await ordersPage.goto();
  await ordersPage.createOrder("test@example.com");
  await expect(ordersPage.orderList).toContainText("test@example.com");
});

test("shows error when API is unavailable", async ({ page }) => {
  // Mock API failure
  await page.route("/api/orders", (route) =>
    route.fulfill({ status: 500, body: "Internal Server Error" })
  );
  await page.goto("/orders");
  await expect(page.getByRole("alert")).toContainText(/failed to load/i);
});
```

### Authentication fixture
```ts
// e2e/fixtures.ts
import { test as base } from "@playwright/test";

export const test = base.extend({
  authenticatedPage: async ({ page }, use) => {
    // Set auth cookie / local storage before every test that needs it
    await page.goto("/login");
    await page.getByLabel("Email").fill("user@example.com");
    await page.getByLabel("Password").fill("password");
    await page.getByRole("button", { name: "Sign in" }).click();
    await page.waitForURL("/dashboard");
    await use(page);
  },
});
```

### Keyboard navigation test
```ts
test("order form is keyboard accessible", async ({ page }) => {
  await page.goto("/orders/new");
  await page.keyboard.press("Tab");
  await expect(page.locator(":focus")).toHaveAttribute("id", "email");
  await page.keyboard.type("test@example.com");
  await page.keyboard.press("Tab");
  await expect(page.locator(":focus")).toHaveAttribute("id", "quantity");
  await page.keyboard.press("Tab");
  await expect(page.locator(":focus")).toHaveRole("button");
  await page.keyboard.press("Enter");
  await expect(page.getByText(/order created/i)).toBeVisible();
});
```

---

## 5. ARIA — Roles and Labels Quick Reference

### Interactive element roles
| Element | Implicit role | When to add explicit role |
|---------|--------------|--------------------------|
| `<button>` | `button` | Never — use native element |
| `<a href="...">` | `link` | Never — use native element |
| `<input type="text">` | `textbox` | Use `role="combobox"` for autocompletes |
| `<select>` | `listbox` | Never — use native element |
| `<ul>` | `list` | `role="menu"` for action menus |
| `<div>` (interactive) | none | Add appropriate role |
| `<div>` (modal) | none | `role="dialog"` + `aria-modal="true"` |

### Essential ARIA attributes
```html
<!-- All interactive elements need an accessible name -->
<button aria-label="Delete order #42">×</button>

<!-- Associate inputs with labels -->
<label for="email">Email</label>
<input id="email" type="email" aria-describedby="email-hint email-error" />
<p id="email-hint">We'll send your receipt here.</p>
<p id="email-error" role="alert">Invalid email address</p>

<!-- Live regions — announce dynamic content -->
<div aria-live="polite" aria-atomic="true">
  <!-- Status messages go here; will be announced to screen readers -->
</div>
<div aria-live="assertive">
  <!-- Urgent errors — interrupts current speech -->
</div>

<!-- Loading states -->
<div role="status" aria-live="polite">
  <span class="sr-only">Loading orders…</span>
</div>

<!-- Expanded/collapsed -->
<button aria-expanded={isOpen} aria-controls="menu-panel">
  Menu
</button>
<ul id="menu-panel" hidden={!isOpen}>...</ul>

<!-- Dialogs -->
<div role="dialog" aria-modal="true" aria-labelledby="dialog-title">
  <h2 id="dialog-title">Confirm Delete</h2>
  ...
</div>
```

### Screen-reader-only utility class
```css
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
```

---

## 6. Keyboard Navigation

### Focus management rules
1. **Focus trap in modals** — when a dialog opens, move focus to the first interactive element inside it; Tab and Shift+Tab must cycle within the dialog; Escape closes and returns focus to the trigger
2. **Skip links** — provide a "Skip to main content" link as the first focusable element on every page
3. **Focus visible** — never remove `:focus-visible` outline globally; use `outline: none` only when providing a custom focus indicator
4. **After async operations** — if a form submission removes the form from the DOM, move focus to the success message or next logical element

### Focus trap (React)
```tsx
import { useEffect, useRef } from "react";

function Modal({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;

    // Focus the first focusable element
    const focusable = panel.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    focusable[0]?.focus();

    function trapFocus(e: KeyboardEvent) {
      if (e.key !== "Tab") return;
      const first = focusable[0];
      const last  = focusable[focusable.length - 1];
      if (e.shiftKey ? document.activeElement === first : document.activeElement === last) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      }
    }

    function handleEscape(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }

    document.addEventListener("keydown", trapFocus);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("keydown", trapFocus);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [onClose]);

  return (
    <div role="dialog" aria-modal="true" ref={panelRef}>
      {children}
    </div>
  );
}
```

### Skip link
```tsx
// In root layout — first element in the DOM
<a href="#main-content" className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2">
  Skip to main content
</a>
<main id="main-content">...</main>
```

---

## 7. axe-core — Automated Accessibility Audits

### Integration with Vitest + Testing Library
```ts
// src/test/setup.ts (add to existing setup)
import { toHaveNoViolations } from "jest-axe";
expect.extend(toHaveNoViolations);
```

```tsx
import { axe } from "jest-axe";
import { renderWithProviders } from "@/test/utils";

test("OrderForm has no accessibility violations", async () => {
  const { container } = renderWithProviders(<OrderForm onSuccess={vi.fn()} />);
  const results = await axe(container);
  expect(results).toHaveNoViolations();
});
```

### Integration with Playwright
```ts
// e2e/accessibility.spec.ts
import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test("orders page has no critical a11y violations", async ({ page }) => {
  await page.goto("/orders");
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa"])   // target WCAG 2.1 AA
    .analyze();
  expect(results.violations).toHaveLength(0);
});
```

---

## 8. Color Contrast Requirements (WCAG 2.1 AA)

| Text type | Minimum contrast ratio |
|-----------|----------------------|
| Normal text (< 18pt / < 14pt bold) | **4.5 : 1** |
| Large text (≥ 18pt or ≥ 14pt bold) | **3 : 1** |
| UI components and graphical objects | **3 : 1** |
| Decorative text / disabled elements | No requirement |

Tools: [WebAIM Contrast Checker](https://webaim.org/resources/contrastchecker/), browser DevTools accessibility panel, axe DevTools extension.

---

## 9. Common Accessibility Pitfalls

| Pitfall | Fix |
|---------|-----|
| Icon-only button with no accessible name | Add `aria-label` or visually hidden `<span>` |
| `onClick` on a `<div>` | Use `<button>` or `<a>` — gets keyboard + ARIA for free |
| Form error message not associated with input | Use `aria-describedby` pointing to error `<p id="...">` |
| Modal that doesn't trap focus | Implement focus trap (see §6) |
| Placeholder as the only label | Add a visible `<label>` — placeholder disappears on input |
| `display: none` used for "visually hidden" labels | Use `.sr-only` CSS class — `display: none` hides from screen readers too |
| Missing `alt` on images | `alt=""` for decorative; meaningful description for informative images |
| Color as the only error indicator | Add text message or icon in addition to red color |
| Focus order doesn't match visual order | Ensure DOM order matches visual tab order; avoid positive `tabindex` values |
| `aria-live` region injected before content | Region must exist in DOM before content is inserted for reliable announcement |
