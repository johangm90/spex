# PHP / Symfony 7 / API Platform 3 — Deep Reference

## Project Structure

```
src/
├── Domain/
│   ├── Model/              ← Entities, Value Objects, Aggregates
│   ├── Repository/         ← Repository interfaces
│   ├── Event/              ← Domain events
│   └── Service/            ← Domain services
├── Application/
│   ├── UseCase/            ← Application services / use cases
│   ├── DTO/                ← Input/Output DTOs
│   └── EventHandler/       ← Domain event handlers
├── Infrastructure/
│   ├── Persistence/        ← Doctrine repository implementations
│   ├── Messenger/          ← Message handlers, transports
│   └── Security/           ← Voters, JWT configurators
├── ApiPlatform/
│   ├── Resource/           ← ApiResource classes (API layer)
│   ├── Processor/          ← State processors (write operations)
│   └── Provider/           ← State providers (read operations)
└── Kernel.php
```

---

## Entity + Doctrine ORM

```php
<?php
// src/Domain/Model/Order.php
namespace App\Domain\Model;

use Doctrine\ORM\Mapping as ORM;
use Doctrine\Common\Collections\ArrayCollection;
use Doctrine\Common\Collections\Collection;

#[ORM\Entity]
#[ORM\Table(name: 'orders')]
#[ORM\HasLifecycleCallbacks]
class Order
{
    #[ORM\Id]
    #[ORM\Column(type: 'uuid', unique: true)]
    private string $id;

    #[ORM\Column(type: 'string', enumType: OrderStatus::class)]
    private OrderStatus $status;

    // DECIMAL for money — never float
    #[ORM\Column(type: 'decimal', precision: 19, scale: 4)]
    private string $totalAmount;

    #[ORM\Column(type: 'string', length: 3)]
    private string $currency;

    #[ORM\OneToMany(targetEntity: OrderLine::class, mappedBy: 'order',
        cascade: ['persist', 'remove'], orphanRemoval: true)]
    private Collection $lines;

    #[ORM\Column]
    private \DateTimeImmutable $createdAt;

    #[ORM\Column(nullable: true)]
    private ?\DateTimeImmutable $updatedAt = null;

    public function __construct(string $id, string $currency)
    {
        $this->id = $id;
        $this->status = OrderStatus::PENDING;
        $this->totalAmount = '0.0000';
        $this->currency = $currency;
        $this->lines = new ArrayCollection();
        $this->createdAt = new \DateTimeImmutable();
    }

    #[ORM\PreUpdate]
    public function onPreUpdate(): void
    {
        $this->updatedAt = new \DateTimeImmutable();
    }

    public function cancel(): void
    {
        if ($this->status !== OrderStatus::PENDING) {
            throw new \DomainException('Only PENDING orders can be cancelled.');
        }
        $this->status = OrderStatus::CANCELLED;
    }

    // Getters only — no public setters on domain state
    public function getId(): string { return $this->id; }
    public function getStatus(): OrderStatus { return $this->status; }
}
```

### Value Object example
```php
<?php
namespace App\Domain\Model;

final readonly class Money
{
    public function __construct(
        public readonly int $amountCents,  // store as integer cents
        public readonly string $currency,
    ) {
        if ($amountCents < 0) {
            throw new \InvalidArgumentException('Amount cannot be negative.');
        }
        if (!in_array($currency, ['EUR', 'USD', 'GBP'], true)) {
            throw new \InvalidArgumentException("Unsupported currency: $currency");
        }
    }

    public function add(self $other): self
    {
        if ($this->currency !== $other->currency) {
            throw new \LogicException('Cannot add different currencies.');
        }
        return new self($this->amountCents + $other->amountCents, $this->currency);
    }
}
```

---

## Repository Pattern

```php
<?php
// Domain interface
namespace App\Domain\Repository;

interface OrderRepositoryInterface
{
    public function findById(string $id): ?\App\Domain\Model\Order;
    public function save(\App\Domain\Model\Order $order): void;
    public function findPendingByUserId(string $userId): array;
}

// Infrastructure implementation
namespace App\Infrastructure\Persistence;

use App\Domain\Model\Order;
use App\Domain\Repository\OrderRepositoryInterface;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;
use Doctrine\Persistence\ManagerRegistry;

class DoctrineOrderRepository extends ServiceEntityRepository
    implements OrderRepositoryInterface
{
    public function __construct(ManagerRegistry $registry)
    {
        parent::__construct($registry, Order::class);
    }

    public function findById(string $id): ?Order
    {
        return $this->find($id);
    }

    public function save(Order $order): void
    {
        $this->getEntityManager()->persist($order);
        // flush is handled at the use-case boundary or in a Doctrine event listener
    }

    public function findPendingByUserId(string $userId): array
    {
        return $this->createQueryBuilder('o')
            ->where('o.userId = :userId')
            ->andWhere('o.status = :status')
            ->setParameter('userId', $userId)
            ->setParameter('status', OrderStatus::PENDING)
            ->orderBy('o.createdAt', 'DESC')
            ->getQuery()
            ->getResult();
    }
}
```

---

## API Platform 3 — Resource Configuration

```php
<?php
// src/ApiPlatform/Resource/OrderResource.php
namespace App\ApiPlatform\Resource;

use ApiPlatform\Metadata\ApiResource;
use ApiPlatform\Metadata\Get;
use ApiPlatform\Metadata\GetCollection;
use ApiPlatform\Metadata\Post;
use ApiPlatform\Metadata\Patch;
use App\ApiPlatform\Processor\CreateOrderProcessor;
use App\ApiPlatform\Processor\CancelOrderProcessor;
use App\ApiPlatform\Provider\OrderProvider;
use App\ApiPlatform\Provider\OrderCollectionProvider;
use Symfony\Component\Validator\Constraints as Assert;
use Symfony\Component\Serializer\Attribute\Groups;

#[ApiResource(
    shortName: 'Order',
    operations: [
        new GetCollection(
            provider: OrderCollectionProvider::class,
            normalizationContext: ['groups' => ['order:list']],
        ),
        new Get(
            provider: OrderProvider::class,
            normalizationContext: ['groups' => ['order:read']],
        ),
        new Post(
            processor: CreateOrderProcessor::class,
            denormalizationContext: ['groups' => ['order:create']],
            normalizationContext: ['groups' => ['order:read']],
            validationContext: ['groups' => ['order:create']],
        ),
        new Patch(
            uriTemplate: '/orders/{id}/cancellation',
            processor: CancelOrderProcessor::class,
            normalizationContext: ['groups' => ['order:read']],
        ),
    ],
)]
class OrderResource
{
    #[Groups(['order:read', 'order:list'])]
    public ?string $id = null;

    #[Groups(['order:read', 'order:list'])]
    public ?string $status = null;

    #[Assert\NotBlank(groups: ['order:create'])]
    #[Assert\Currency(groups: ['order:create'])]
    #[Groups(['order:create', 'order:read'])]
    public ?string $currency = null;

    #[Groups(['order:read'])]
    public ?\DateTimeImmutable $createdAt = null;
}
```

---

## State Processor (Write Operations)

```php
<?php
// src/ApiPlatform/Processor/CreateOrderProcessor.php
namespace App\ApiPlatform\Processor;

use ApiPlatform\Metadata\Operation;
use ApiPlatform\State\ProcessorInterface;
use App\Application\UseCase\CreateOrder\CreateOrderCommand;
use App\Application\UseCase\CreateOrder\CreateOrderHandler;
use App\ApiPlatform\Resource\OrderResource;
use Symfony\Component\HttpKernel\Exception\ConflictHttpException;

final class CreateOrderProcessor implements ProcessorInterface
{
    public function __construct(
        private readonly CreateOrderHandler $handler,
    ) {}

    public function process(mixed $data, Operation $operation, array $uriVariables = [],
        array $context = []): OrderResource
    {
        /** @var OrderResource $data */
        $idempotencyKey = $context['request']->headers->get('Idempotency-Key')
            ?? throw new \InvalidArgumentException('Idempotency-Key header required.');

        try {
            $order = $this->handler->handle(new CreateOrderCommand(
                idempotencyKey: $idempotencyKey,
                currency: $data->currency,
                userId: $this->security->getUser()->getUserIdentifier(),
            ));
        } catch (\App\Domain\Exception\DuplicateIdempotencyKeyException $e) {
            throw new ConflictHttpException($e->getMessage());
        }

        return $this->toResource($order);
    }

    private function toResource(\App\Domain\Model\Order $order): OrderResource
    {
        $resource = new OrderResource();
        $resource->id = $order->getId();
        $resource->status = $order->getStatus()->value;
        $resource->createdAt = $order->getCreatedAt();
        return $resource;
    }
}
```

## State Provider (Read Operations)

```php
<?php
// src/ApiPlatform/Provider/OrderProvider.php
namespace App\ApiPlatform\Provider;

use ApiPlatform\Metadata\Operation;
use ApiPlatform\State\ProviderInterface;
use App\Domain\Repository\OrderRepositoryInterface;
use App\ApiPlatform\Resource\OrderResource;
use Symfony\Component\HttpKernel\Exception\NotFoundHttpException;

final class OrderProvider implements ProviderInterface
{
    public function __construct(
        private readonly OrderRepositoryInterface $orderRepository,
    ) {}

    public function provide(Operation $operation, array $uriVariables = [],
        array $context = []): OrderResource
    {
        $order = $this->orderRepository->findById($uriVariables['id'])
            ?? throw new NotFoundHttpException('Order not found.');

        $resource = new OrderResource();
        $resource->id = $order->getId();
        $resource->status = $order->getStatus()->value;
        $resource->createdAt = $order->getCreatedAt();
        return $resource;
    }
}
```

---

## JWT Authentication (LexikJWTAuthenticationBundle)

```yaml
# config/packages/lexik_jwt_authentication.yaml
lexik_jwt_authentication:
    secret_key: '%env(JWT_SECRET_KEY)%'
    public_key: '%env(JWT_PUBLIC_KEY)%'
    pass_phrase: '%env(JWT_PASSPHRASE)%'
    token_ttl: 900  # 15 minutes

# config/packages/security.yaml
security:
    firewalls:
        login:
            pattern: ^/api/auth/token
            stateless: true
            json_login:
                check_path: /api/auth/token
                success_handler: lexik_jwt_authentication.handler.authentication_success
                failure_handler: lexik_jwt_authentication.handler.authentication_failure
        api:
            pattern: ^/api
            stateless: true
            jwt: ~
    access_control:
        - { path: ^/api/auth, roles: PUBLIC_ACCESS }
        - { path: ^/api, roles: IS_AUTHENTICATED_FULLY }
```

### Custom JWT payload
```php
<?php
namespace App\Infrastructure\Security;

use Lexik\Bundle\JWTAuthenticationBundle\Event\JWTCreatedEvent;

class JWTCreatedSubscriber
{
    public function onJWTCreated(JWTCreatedEvent $event): void
    {
        $payload = $event->getData();
        $user = $event->getUser();
        $payload['userId'] = $user->getId();
        $payload['roles'] = $user->getRoles();
        $event->setData($payload);
    }
}
```

---

## Custom Security Voter

```php
<?php
namespace App\Infrastructure\Security;

use App\Domain\Model\Order;
use Symfony\Component\Security\Core\Authentication\Token\TokenInterface;
use Symfony\Component\Security\Core\Authorization\Voter\Voter;

class OrderVoter extends Voter
{
    const VIEW = 'ORDER_VIEW';
    const CANCEL = 'ORDER_CANCEL';

    protected function supports(string $attribute, mixed $subject): bool
    {
        return in_array($attribute, [self::VIEW, self::CANCEL])
            && $subject instanceof Order;
    }

    protected function voteOnAttribute(string $attribute, mixed $subject,
        TokenInterface $token): bool
    {
        $user = $token->getUser();
        /** @var Order $order */
        $order = $subject;

        return match($attribute) {
            self::VIEW   => $order->getUserId() === $user->getId()
                            || in_array('ROLE_ADMIN', $user->getRoles()),
            self::CANCEL => $order->getUserId() === $user->getId(),
            default      => false,
        };
    }
}
```

---

## Symfony Messenger (Async Jobs)

```php
<?php
// Message
namespace App\Application\Message;

final readonly class SendOrderConfirmationEmail
{
    public function __construct(public readonly string $orderId) {}
}

// Handler
namespace App\Application\MessageHandler;

use App\Application\Message\SendOrderConfirmationEmail;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;

#[AsMessageHandler]
final class SendOrderConfirmationEmailHandler
{
    public function __construct(private readonly MailerInterface $mailer) {}

    public function __invoke(SendOrderConfirmationEmail $message): void
    {
        // send email for $message->orderId
    }
}
```

```yaml
# config/packages/messenger.yaml
framework:
    messenger:
        transports:
            async:
                dsn: '%env(MESSENGER_TRANSPORT_DSN)%'
                retry_strategy:
                    max_retries: 3
                    delay: 1000
                    multiplier: 2
        routing:
            'App\Application\Message\SendOrderConfirmationEmail': async
```

---

## Testing — PHPUnit + ApiTestCase

```php
<?php
// tests/Api/OrderTest.php
namespace App\Tests\Api;

use ApiPlatform\Symfony\Bundle\Test\ApiTestCase;
use App\Tests\Factory\UserFactory;
use App\Tests\Factory\OrderFactory;
use Zenstruck\Foundry\Test\Factories;
use Zenstruck\Foundry\Test\ResetDatabase;

class OrderTest extends ApiTestCase
{
    use Factories;
    use ResetDatabase;

    public function testCreateOrder(): void
    {
        $client = static::createClient();
        $user = UserFactory::createOne();
        $token = $this->getJwtToken($client, $user);

        $client->request('POST', '/api/orders', [
            'json' => ['currency' => 'EUR'],
            'headers' => [
                'Authorization' => "Bearer $token",
                'Idempotency-Key' => 'test-idem-key-1',
            ],
        ]);

        $this->assertResponseStatusCodeSame(201);
        $this->assertJsonContains(['status' => 'PENDING', 'currency' => 'EUR']);
    }

    public function testCreateOrderIdempotency(): void
    {
        $client = static::createClient();
        $user = UserFactory::createOne();
        $token = $this->getJwtToken($client, $user);

        // First request
        $client->request('POST', '/api/orders', [
            'json' => ['currency' => 'EUR'],
            'headers' => ['Authorization' => "Bearer $token", 'Idempotency-Key' => 'same-key'],
        ]);
        $this->assertResponseStatusCodeSame(201);

        // Replay — must return same 201, not create a second order
        $client->request('POST', '/api/orders', [
            'json' => ['currency' => 'EUR'],
            'headers' => ['Authorization' => "Bearer $token", 'Idempotency-Key' => 'same-key'],
        ]);
        $this->assertResponseStatusCodeSame(201);
        // Assert only 1 order exists
        $this->assertCount(1, OrderFactory::repository()->findAll());
    }

    public function testCancelOrderForbiddenForOtherUser(): void
    {
        $owner = UserFactory::createOne();
        $other = UserFactory::createOne();
        $order = OrderFactory::createOne(['user' => $owner]);

        $client = static::createClient();
        $token = $this->getJwtToken($client, $other);

        $client->request('PATCH', "/api/orders/{$order->getId()}/cancellation", [
            'headers' => ['Authorization' => "Bearer $token"],
            'json' => [],
        ]);

        $this->assertResponseStatusCodeSame(403);
    }
}
```

### Unit test (domain logic, no framework)
```php
<?php
namespace App\Tests\Unit\Domain;

use App\Domain\Model\Order;
use App\Domain\Model\OrderStatus;
use PHPUnit\Framework\TestCase;

class OrderTest extends TestCase
{
    public function testCancelPendingOrder(): void
    {
        $order = new Order('uuid-1', 'EUR');
        $order->cancel();
        $this->assertSame(OrderStatus::CANCELLED, $order->getStatus());
    }

    public function testCannotCancelAlreadyCancelledOrder(): void
    {
        $this->expectException(\DomainException::class);
        $order = new Order('uuid-1', 'EUR');
        $order->cancel();
        $order->cancel(); // second cancel throws
    }
}
```

---

## Common Gotchas

🔴 **Doctrine lazy loading in serialization** — accessing a lazy-loaded collection inside a serializer can trigger N+1 queries. Use `JOIN FETCH` in DQL or `fetch: EAGER` explicitly, but prefer DTOs that only carry what's needed.

🔴 **Circular reference in serializer** — use `#[Groups]` carefully; never put the same group on both sides of a bidirectional relation. Use `max_depth` or a dedicated output DTO.

🟠 **Entity mutated outside aggregate** — Doctrine tracks all managed entities. If you modify an entity retrieved from the repo without going through the aggregate root, you bypass invariant checks. Always mutate through the root.

🟠 **Flushing inside a repository** — `flush()` in the repository breaks the unit-of-work pattern and makes it impossible to batch multiple changes in one transaction. Flush in the use case or a Doctrine lifecycle subscriber.

🔵 **API Platform and custom controllers** — prefer State Processors/Providers over custom controllers; they integrate with API Platform's event system, pagination, and serialization. Custom controllers are an escape hatch, not the default.

🔵 **Symfony Messenger and failed messages** — always configure a `failure_transport`; without it, failed messages are silently discarded after exhausting retries.
