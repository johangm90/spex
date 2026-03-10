# Testing PHP — spex-qa Reference

Canonical PHPUnit + Symfony test patterns for the preferred project stack (PHP/Symfony + MariaDB).

---

## PHPUnit Configuration

```xml
<!-- phpunit.xml.dist -->
<?xml version="1.0" encoding="UTF-8"?>
<phpunit xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:noNamespaceSchemaLocation="vendor/phpunit/phpunit/phpunit.xsd"
         bootstrap="tests/bootstrap.php"
         colors="true"
         stopOnFailure="false">

    <testsuites>
        <testsuite name="unit">
            <directory>tests/Unit</directory>
        </testsuite>
        <testsuite name="integration">
            <directory>tests/Integration</directory>
        </testsuite>
        <testsuite name="functional">
            <directory>tests/Functional</directory>
        </testsuite>
    </testsuites>

    <coverage>
        <include>
            <directory suffix=".php">src</directory>
        </include>
        <exclude>
            <directory>src/Migrations</directory>
            <directory>src/Kernel.php</directory>
        </exclude>
        <report>
            <html outputDirectory="coverage/html"/>
            <clover outputFile="coverage/clover.xml"/>
            <text outputFile="php://stdout" showUncoveredFiles="false"/>
        </report>
    </coverage>

    <php>
        <ini name="error_reporting" value="-1"/>
        <env name="APP_ENV" value="test"/>
        <env name="SYMFONY_DEPRECATIONS_HELPER" value="weak"/>
        <env name="KERNEL_CLASS" value="App\Kernel"/>
    </php>
</phpunit>
```

```bash
# Run commands
php bin/phpunit                          # all suites
php bin/phpunit --testsuite unit         # unit only (fast)
php bin/phpunit --coverage-html coverage # with HTML coverage
php bin/phpunit --filter OrderServiceTest # single class
```

---

## Unit Test — Domain Entity / Service (AAA pattern)

```php
<?php
// tests/Unit/Domain/Order/OrderServiceTest.php
declare(strict_types=1);

namespace App\Tests\Unit\Domain\Order;

use App\Domain\Order\Order;
use App\Domain\Order\OrderService;
use App\Domain\Order\OrderRepository;
use App\Domain\Order\Exception\InsufficientStockException;
use PHPUnit\Framework\TestCase;
use PHPUnit\Framework\MockObject\MockObject;

final class OrderServiceTest extends TestCase
{
    private OrderRepository&MockObject $repository;
    private OrderService $service;

    protected function setUp(): void
    {
        $this->repository = $this->createMock(OrderRepository::class);
        $this->service    = new OrderService($this->repository);
    }

    public function testCreateOrderPersistsAndReturnsOrder(): void
    {
        // Arrange
        $this->repository
            ->expects($this->once())
            ->method('save')
            ->with($this->isInstanceOf(Order::class));

        // Act
        $order = $this->service->create(productId: 42, quantity: 2, userId: 'user-1');

        // Assert
        $this->assertSame(42, $order->getProductId());
        $this->assertSame(2, $order->getQuantity());
        $this->assertSame('pending', $order->getStatus());
    }

    public function testCreateOrderThrowsWhenStockInsufficient(): void
    {
        // Arrange
        $this->repository
            ->method('getAvailableStock')
            ->with(42)
            ->willReturn(1);

        // Assert
        $this->expectException(InsufficientStockException::class);
        $this->expectExceptionMessage('Insufficient stock for product 42');

        // Act
        $this->service->create(productId: 42, quantity: 5, userId: 'user-1');
    }

    /** @dataProvider invalidQuantityProvider */
    public function testCreateOrderRejectsInvalidQuantity(int $quantity): void
    {
        $this->expectException(\InvalidArgumentException::class);

        $this->service->create(productId: 1, quantity: $quantity, userId: 'user-1');
    }

    public static function invalidQuantityProvider(): array
    {
        return [
            'zero'     => [0],
            'negative' => [-1],
        ];
    }
}
```

---

## Unit Test — Value Object

```php
<?php
// tests/Unit/Domain/Money/MoneyTest.php
declare(strict_types=1);

namespace App\Tests\Unit\Domain\Money;

use App\Domain\Money\Money;
use App\Domain\Money\Currency;
use PHPUnit\Framework\TestCase;

final class MoneyTest extends TestCase
{
    public function testAddSameCurrency(): void
    {
        $a = Money::of(1000, Currency::EUR);
        $b = Money::of(500, Currency::EUR);

        $result = $a->add($b);

        $this->assertSame(1500, $result->getAmount());
        $this->assertSame(Currency::EUR, $result->getCurrency());
    }

    public function testAddDifferentCurrencyThrows(): void
    {
        $this->expectException(\DomainException::class);

        Money::of(1000, Currency::EUR)->add(Money::of(500, Currency::USD));
    }

    public function testEquality(): void
    {
        $this->assertTrue(
            Money::of(100, Currency::EUR)->equals(Money::of(100, Currency::EUR))
        );
        $this->assertFalse(
            Money::of(100, Currency::EUR)->equals(Money::of(100, Currency::USD))
        );
    }
}
```

---

## Integration Test — Symfony WebTestCase (HTTP layer)

```php
<?php
// tests/Functional/Api/OrderControllerTest.php
declare(strict_types=1);

namespace App\Tests\Functional\Api;

use App\Tests\Functional\ApiTestTrait;
use Symfony\Bundle\FrameworkBundle\Test\WebTestCase;
use Doctrine\ORM\EntityManagerInterface;

final class OrderControllerTest extends WebTestCase
{
    use ApiTestTrait;

    public function testCreateOrderReturns201(): void
    {
        $client = static::createAuthenticatedClient('user@example.com');

        $client->request('POST', '/api/orders', [], [], [
            'CONTENT_TYPE' => 'application/json',
        ], json_encode([
            'product_id' => 1,
            'quantity'   => 2,
        ]));

        $this->assertResponseStatusCodeSame(201);
        $this->assertResponseHeaderSame('content-type', 'application/json');

        $body = json_decode($client->getResponse()->getContent(), true);
        $this->assertArrayHasKey('id', $body);
        $this->assertSame('pending', $body['status']);
    }

    public function testCreateOrderReturns400WhenQuantityMissing(): void
    {
        $client = static::createAuthenticatedClient('user@example.com');

        $client->request('POST', '/api/orders', [], [], [
            'CONTENT_TYPE' => 'application/json',
        ], json_encode(['product_id' => 1]));

        $this->assertResponseStatusCodeSame(400);
        $body = json_decode($client->getResponse()->getContent(), true);
        $this->assertArrayHasKey('violations', $body);
    }

    public function testCreateOrderReturns401WhenUnauthenticated(): void
    {
        $client = static::createClient();

        $client->request('POST', '/api/orders', [], [], [
            'CONTENT_TYPE' => 'application/json',
        ], json_encode(['product_id' => 1, 'quantity' => 1]));

        $this->assertResponseStatusCodeSame(401);
    }

    public function testGetOrderReturns403WhenAccessingOtherUsersResource(): void
    {
        $client = static::createAuthenticatedClient('attacker@example.com');

        // ID belongs to victim@example.com's order (loaded by fixture)
        $client->request('GET', '/api/orders/019123ab-0000-0000-0000-000000000001');

        $this->assertResponseStatusCodeSame(403);
    }
}
```

### ApiTestTrait — helper for authenticated requests

```php
<?php
// tests/Functional/ApiTestTrait.php
declare(strict_types=1);

namespace App\Tests\Functional;

use Symfony\Bundle\FrameworkBundle\KernelBrowser;
use App\Entity\User;

trait ApiTestTrait
{
    protected static function createAuthenticatedClient(string $email): KernelBrowser
    {
        $client = static::createClient();

        /** @var User $user */
        $user = static::getContainer()
            ->get('doctrine')
            ->getRepository(User::class)
            ->findOneBy(['email' => $email]);

        $client->loginUser($user);

        return $client;
    }
}
```

---

## API Platform Test — ApiTestCase

```php
<?php
// tests/Functional/Api/ProductResourceTest.php
declare(strict_types=1);

namespace App\Tests\Functional\Api;

use ApiPlatform\Symfony\Bundle\Test\ApiTestCase;
use ApiPlatform\Symfony\Bundle\Test\Client;

final class ProductResourceTest extends ApiTestCase
{
    private Client $client;

    protected function setUp(): void
    {
        $this->client = static::createClient();
    }

    public function testGetProductCollection(): void
    {
        $this->client->request('GET', '/api/products', [
            'headers' => ['Accept' => 'application/ld+json'],
        ]);

        $this->assertResponseIsSuccessful();
        $this->assertResponseHeaderSame('content-type', 'application/ld+json; charset=utf-8');
        $this->assertJsonContains([
            '@context'         => '/api/contexts/Product',
            '@type'            => 'hydra:Collection',
            'hydra:totalItems' => 5,
        ]);
    }

    public function testCreateProductWithAdminRole(): void
    {
        $token = $this->getAdminJwt();

        $this->client->request('POST', '/api/products', [
            'headers' => [
                'Content-Type'  => 'application/ld+json',
                'Authorization' => "Bearer {$token}",
            ],
            'json' => [
                'name'  => 'Widget Pro',
                'price' => 1999,
                'sku'   => 'WGT-001',
            ],
        ]);

        $this->assertResponseStatusCodeSame(201);
        $this->assertJsonContains(['name' => 'Widget Pro']);
    }

    private function getAdminJwt(): string
    {
        // Exchange credentials for JWT token
        $response = $this->client->request('POST', '/api/auth', [
            'json' => ['email' => 'admin@example.com', 'password' => 'admin_password'],
        ]);

        return $response->toArray()['token'];
    }
}
```

---

## Database Tests — DAMA Doctrine Test Bundle

Use `dama/doctrine-test-bundle` to wrap each test in a transaction and roll back automatically — no manual teardown needed.

```yaml
# config/packages/test/dama_doctrine_test.yaml
dama_doctrine_test:
    enable_static_connection: true
    enable_static_meta_data_cache: true
    enable_static_query_cache: true
```

```php
<?php
// tests/Integration/Repository/OrderRepositoryTest.php
declare(strict_types=1);

namespace App\Tests\Integration\Repository;

use App\Domain\Order\Order;
use App\Domain\Order\OrderRepository;
use Symfony\Bundle\FrameworkBundle\Test\KernelTestCase;

final class OrderRepositoryTest extends KernelTestCase
{
    private OrderRepository $repository;

    protected function setUp(): void
    {
        self::bootKernel();
        $this->repository = static::getContainer()->get(OrderRepository::class);
    }

    public function testFindPendingOrders(): void
    {
        // Arrange — DAMA wraps in a transaction; no cleanup needed
        $order = Order::create(productId: 1, quantity: 2, userId: 'user-1');
        $this->repository->save($order);

        // Act
        $pending = $this->repository->findByStatus('pending');

        // Assert
        $this->assertCount(1, $pending);
        $this->assertSame('pending', $pending[0]->getStatus());
    }
    // Each test is automatically rolled back by DAMA
}
```

---

## Fixtures — Nelmio Alice

```yaml
# fixtures/orders.yaml
App\Entity\User:
  user_victim:
    email: victim@example.com
    roles: ['ROLE_USER']
    password: '$2y$13$...'  # bcrypt hash of "password"

  user_attacker:
    email: attacker@example.com
    roles: ['ROLE_USER']
    password: '$2y$13$...'

App\Entity\Order:
  order_victim_1:
    id: '019123ab-0000-0000-0000-000000000001'
    user: '@user_victim'
    productId: 1
    quantity: 2
    status: pending
```

```php
// Load fixtures in test bootstrap or via command:
// php bin/console doctrine:fixtures:load --env=test --no-interaction
```

---

## Test Doubles Cheat Sheet

| Need | PHPUnit method | Notes |
|---|---|---|
| Replace a dependency | `createMock(Interface::class)` | Stubs all methods to return `null` |
| Partial stub (some real methods) | `createPartialMock(Class::class, ['method'])` | Only listed methods are stubbed |
| Assert method called exactly once | `->expects($this->once())->method(...)` | Fails test if 0 or 2+ calls |
| Assert method never called | `->expects($this->never())->method(...)` | |
| Return a value | `->willReturn($value)` | |
| Return different values on successive calls | `->willReturnOnConsecutiveCalls($a, $b, $c)` | |
| Throw from a mock | `->willThrowException(new \RuntimeException())` | |
| Capture argument passed to mock | `->with($this->callback(fn($arg) => $arg->getId() === 1))` | |

---

## Coverage in CI (GitHub Actions)

```yaml
- name: Run PHPUnit with coverage
  run: |
    docker compose run --rm \
      -e XDEBUG_MODE=coverage \
      app php bin/phpunit \
        --coverage-clover coverage/clover.xml \
        --coverage-text

- name: Upload to Codecov
  uses: codecov/codecov-action@v4
  with:
    files: coverage/clover.xml
    fail_ci_if_error: true
    threshold: 80
```

---

## Running Tests in Docker (Makefile targets)

```makefile
test-unit:         ## Run unit tests only (fast)
	docker compose run --rm app php bin/phpunit --testsuite unit

test-integration:  ## Run integration + functional tests
	docker compose -f docker-compose.test.yml run --rm app php bin/phpunit --testsuite integration,functional

test-coverage:     ## Run all tests with HTML coverage report
	docker compose -f docker-compose.test.yml run --rm \
	  -e XDEBUG_MODE=coverage \
	  app php bin/phpunit --coverage-html coverage/html

test:              ## Run all tests (unit + integration + functional)
	docker compose -f docker-compose.test.yml up -d --wait
	docker compose -f docker-compose.test.yml run --rm app php bin/phpunit
	docker compose -f docker-compose.test.yml down -v
```
