# Testing JS / E2E — spex-qa Reference

Canonical patterns for Vitest, Testing Library, MSW v2, Playwright, and k6.

---

## Vitest Configuration

```ts
// vitest.config.ts
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'clover'],
      thresholds: {
        lines: 80,
        functions: 80,
        branches: 75,
      },
      include: ['src/**'],
      exclude: ['src/**/*.stories.*', 'src/types/**'],
    },
  },
})
```

```ts
// tests/setup.ts
import '@testing-library/jest-dom'
import { server } from './msw/server'

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())
```

---

## Vitest — Unit Test (pure function / service)

```ts
// src/domain/order/order.test.ts
import { describe, it, expect } from 'vitest'
import { calculateOrderTotal, applyDiscount } from './order'

describe('calculateOrderTotal', () => {
  it('sums line items correctly', () => {
    const lines = [
      { price: 1000, quantity: 2 },
      { price: 500, quantity: 1 },
    ]
    expect(calculateOrderTotal(lines)).toBe(2500)
  })

  it('returns 0 for an empty order', () => {
    expect(calculateOrderTotal([])).toBe(0)
  })
})

describe('applyDiscount', () => {
  it.each([
    [10_000, 10, 9_000],
    [10_000, 0,  10_000],
    [10_000, 100, 0],
  ])('applies %i% discount to %i → %i', (total, pct, expected) => {
    expect(applyDiscount(total, pct)).toBe(expected)
  })

  it('throws when discount exceeds 100%', () => {
    expect(() => applyDiscount(1000, 101)).toThrow('Discount cannot exceed 100%')
  })
})
```

---

## Vitest — Mocking modules and spies

```ts
import { vi, describe, it, expect, beforeEach } from 'vitest'
import { OrderService } from './OrderService'
import * as repo from '../repository/orderRepository'

// Auto-mock entire module
vi.mock('../repository/orderRepository')

describe('OrderService', () => {
  const mockSave = vi.mocked(repo.saveOrder)

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('calls saveOrder with correct payload', async () => {
    mockSave.mockResolvedValue({ id: 'abc-123', status: 'pending' })

    const service = new OrderService()
    const result = await service.create({ productId: 1, quantity: 2 })

    expect(mockSave).toHaveBeenCalledOnce()
    expect(mockSave).toHaveBeenCalledWith(
      expect.objectContaining({ productId: 1, quantity: 2 }),
    )
    expect(result.status).toBe('pending')
  })

  it('propagates repository errors', async () => {
    mockSave.mockRejectedValue(new Error('DB unavailable'))

    const service = new OrderService()
    await expect(service.create({ productId: 1, quantity: 2 }))
      .rejects.toThrow('DB unavailable')
  })
})
```

---

## Testing Library — React Component Tests

```tsx
// src/features/orders/CreateOrderForm.test.tsx
import { describe, it, expect } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { CreateOrderForm } from './CreateOrderForm'
import { wrapper } from '../../../tests/utils/wrapper'  // QueryClient + Router wrapper

describe('CreateOrderForm', () => {
  it('submits the form with valid data', async () => {
    const user = userEvent.setup()
    render(<CreateOrderForm />, { wrapper })

    // Query by accessible role/label — never by test-id if avoidable
    await user.type(screen.getByLabelText(/quantity/i), '2')
    await user.click(screen.getByRole('button', { name: /place order/i }))

    await waitFor(() => {
      expect(screen.getByText(/order placed successfully/i)).toBeInTheDocument()
    })
  })

  it('shows validation error when quantity is empty', async () => {
    const user = userEvent.setup()
    render(<CreateOrderForm />, { wrapper })

    await user.click(screen.getByRole('button', { name: /place order/i }))

    expect(await screen.findByRole('alert')).toHaveTextContent(/quantity is required/i)
  })

  it('disables submit button while request is pending', async () => {
    const user = userEvent.setup()
    render(<CreateOrderForm />, { wrapper })

    await user.type(screen.getByLabelText(/quantity/i), '2')
    await user.click(screen.getByRole('button', { name: /place order/i }))

    expect(screen.getByRole('button', { name: /place order/i })).toBeDisabled()
  })
})
```

### RTL query priority (use in this order)

1. `getByRole` — most resilient, tied to ARIA semantics
2. `getByLabelText` — for form elements
3. `getByPlaceholderText` — only if no label
4. `getByText` — for non-interactive text
5. `getByDisplayValue` — for inputs with a current value
6. `getByAltText` — for images
7. `getByTestId` — last resort; avoid in new tests

---

## MSW v2 — Request Handlers

```ts
// tests/msw/handlers.ts
import { http, HttpResponse } from 'msw'

export const handlers = [
  http.get('/api/orders', () => {
    return HttpResponse.json({
      data: [
        { id: 'abc-123', status: 'pending', quantity: 2 },
      ],
      total: 1,
    })
  }),

  http.post('/api/orders', async ({ request }) => {
    const body = await request.json() as Record<string, unknown>

    if (!body.quantity) {
      return HttpResponse.json(
        { violations: [{ field: 'quantity', message: 'Quantity is required' }] },
        { status: 400 },
      )
    }

    return HttpResponse.json(
      { id: 'new-123', status: 'pending', quantity: body.quantity },
      { status: 201 },
    )
  }),

  http.get('/api/orders/:id', ({ params }) => {
    if (params.id === 'not-found') {
      return new HttpResponse(null, { status: 404 })
    }
    return HttpResponse.json({ id: params.id, status: 'pending' })
  }),
]
```

```ts
// tests/msw/server.ts
import { setupServer } from 'msw/node'
import { handlers } from './handlers'

export const server = setupServer(...handlers)
```

### Override handlers in a single test

```ts
it('shows error when server returns 500', async () => {
  server.use(
    http.post('/api/orders', () =>
      new HttpResponse(null, { status: 500 }),
    ),
  )
  // ... render and assert error state
})
```

---

## Playwright — Configuration

```ts
// playwright.config.ts
import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [['html'], ['junit', { outputFile: 'playwright-results.xml' }]],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
    {
      name: 'mobile-chrome',
      use: { ...devices['Pixel 7'] },
    },
  ],
  webServer: {
    command: 'docker compose up --wait',
    url: 'http://localhost/healthz',
    reuseExistingServer: !process.env.CI,
  },
})
```

---

## Playwright — Page Object Model

```ts
// e2e/pages/OrderPage.ts
import { Page, Locator, expect } from '@playwright/test'

export class OrderPage {
  readonly quantityInput: Locator
  readonly submitButton: Locator
  readonly successMessage: Locator
  readonly errorMessage: Locator

  constructor(private readonly page: Page) {
    this.quantityInput  = page.getByLabel(/quantity/i)
    this.submitButton   = page.getByRole('button', { name: /place order/i })
    this.successMessage = page.getByRole('status', { name: /success/i })
    this.errorMessage   = page.getByRole('alert')
  }

  async goto(): Promise<void> {
    await this.page.goto('/orders/new')
  }

  async fillAndSubmit(quantity: number): Promise<void> {
    await this.quantityInput.fill(String(quantity))
    await this.submitButton.click()
  }

  async expectSuccess(): Promise<void> {
    await expect(this.successMessage).toBeVisible()
  }

  async expectError(text: string | RegExp): Promise<void> {
    await expect(this.errorMessage).toContainText(text)
  }
}
```

```ts
// e2e/pages/LoginPage.ts
import { Page } from '@playwright/test'

export class LoginPage {
  constructor(private readonly page: Page) {}

  async loginAs(email: string, password: string): Promise<void> {
    await this.page.goto('/login')
    await this.page.getByLabel(/email/i).fill(email)
    await this.page.getByLabel(/password/i).fill(password)
    await this.page.getByRole('button', { name: /sign in/i }).click()
    await this.page.waitForURL('/dashboard')
  }
}
```

---

## Playwright — Fixtures (shared auth state)

```ts
// e2e/fixtures.ts
import { test as base, Page } from '@playwright/test'
import { LoginPage } from './pages/LoginPage'
import { OrderPage } from './pages/OrderPage'

type Fixtures = {
  authenticatedPage: Page
  orderPage: OrderPage
}

export const test = base.extend<Fixtures>({
  authenticatedPage: async ({ page }, use) => {
    const login = new LoginPage(page)
    await login.loginAs('user@example.com', 'password')
    await use(page)
  },

  orderPage: async ({ authenticatedPage }, use) => {
    const order = new OrderPage(authenticatedPage)
    await order.goto()
    await use(order)
  },
})

export { expect } from '@playwright/test'
```

```ts
// e2e/order.spec.ts
import { test, expect } from './fixtures'

test.describe('Create order flow', () => {
  test('places order with valid quantity', async ({ orderPage }) => {
    await orderPage.fillAndSubmit(2)
    await orderPage.expectSuccess()
  })

  test('shows error when quantity is zero', async ({ orderPage }) => {
    await orderPage.fillAndSubmit(0)
    await orderPage.expectError(/must be at least 1/i)
  })

  test('redirects unauthenticated user to login', async ({ page }) => {
    await page.goto('/orders/new')
    await expect(page).toHaveURL('/login')
  })
})
```

---

## Playwright — Network Interception

```ts
// Stub an API call to test error states
test('shows error banner when API returns 500', async ({ authenticatedPage }) => {
  await authenticatedPage.route('**/api/orders', route =>
    route.fulfill({ status: 500, body: '' }),
  )

  const orderPage = new OrderPage(authenticatedPage)
  await orderPage.goto()
  await orderPage.fillAndSubmit(1)

  await expect(authenticatedPage.getByRole('alert')).toContainText(/something went wrong/i)
})

// Assert API call was made with correct payload
test('sends correct payload to API', async ({ authenticatedPage }) => {
  let capturedBody: unknown

  await authenticatedPage.route('**/api/orders', async route => {
    capturedBody = JSON.parse(route.request().postData() ?? '{}')
    await route.continue()
  })

  const orderPage = new OrderPage(authenticatedPage)
  await orderPage.goto()
  await orderPage.fillAndSubmit(3)

  expect(capturedBody).toMatchObject({ quantity: 3 })
})
```

---

## k6 — Load Test Script

```js
// load-tests/order-create.js
import http from 'k6/http'
import { check, sleep } from 'k6'
import { Rate, Trend } from 'k6/metrics'

const errorRate    = new Rate('errors')
const orderLatency = new Trend('order_create_latency', true)

export const options = {
  stages: [
    { duration: '30s', target: 10  },   // ramp-up
    { duration: '1m',  target: 50  },   // sustained load
    { duration: '30s', target: 100 },   // peak
    { duration: '30s', target: 0   },   // ramp-down
  ],
  thresholds: {
    http_req_duration:     ['p(95)<200', 'p(99)<500'],
    http_req_failed:       ['rate<0.01'],    // < 1% errors
    order_create_latency:  ['p(95)<200'],
  },
}

const BASE_URL = __ENV.BASE_URL || 'http://localhost'
const TOKEN    = __ENV.API_TOKEN

export default function () {
  const payload = JSON.stringify({ product_id: 1, quantity: 1 })
  const headers = {
    'Content-Type':  'application/json',
    'Authorization': `Bearer ${TOKEN}`,
  }

  const res = http.post(`${BASE_URL}/api/orders`, payload, { headers })
  orderLatency.add(res.timings.duration)

  const ok = check(res, {
    'status is 201':   r => r.status === 201,
    'has id in body':  r => JSON.parse(r.body).id !== undefined,
    'latency < 200ms': r => r.timings.duration < 200,
  })

  errorRate.add(!ok)
  sleep(1)
}
```

```bash
# Run load test
k6 run \
  -e BASE_URL=http://localhost \
  -e API_TOKEN=test_token_here \
  load-tests/order-create.js
```

---

## Running in CI (GitHub Actions)

```yaml
- name: Run Vitest unit tests
  run: npx vitest run --coverage

- name: Run Playwright E2E
  run: npx playwright test
  env:
    E2E_BASE_URL: http://localhost

- name: Upload Playwright report
  uses: actions/upload-artifact@v4
  if: always()
  with:
    name: playwright-report
    path: playwright-report/
    retention-days: 7

- name: Run k6 load tests
  uses: grafana/k6-action@v0.3.1
  with:
    filename: load-tests/order-create.js
  env:
    BASE_URL: http://localhost
    API_TOKEN: ${{ secrets.API_TOKEN }}
```
