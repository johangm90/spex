# Node.js / TypeScript / NestJS — Deep Reference

## Project Structure

```
src/
├── domain/
│   ├── model/              ← Classes / interfaces (entities, value objects)
│   ├── repository/         ← Repository interfaces
│   ├── event/              ← Domain event types
│   └── service/            ← Domain services
├── application/
│   ├── use-case/           ← Application services / use cases
│   └── dto/                ← Input/Output DTOs (Zod schemas or class-validator)
├── infrastructure/
│   ├── persistence/        ← Prisma repository implementations
│   ├── queue/              ← BullMQ producers / consumers
│   └── security/           ← JWT strategy, guards
├── api/
│   ├── controller/         ← NestJS controllers
│   ├── pipe/               ← Validation pipes
│   ├── guard/              ← Auth guards
│   └── filter/             ← Exception filters
├── app.module.ts
└── main.ts
```

---

## NestJS Module Architecture

```typescript
// src/orders/orders.module.ts
import { Module } from '@nestjs/common';
import { OrdersController } from '../api/controller/orders.controller';
import { CreateOrderUseCase } from '../application/use-case/create-order.use-case';
import { PrismaOrderRepository } from '../infrastructure/persistence/prisma-order.repository';
import { OrderRepository } from '../domain/repository/order.repository';
import { BullModule } from '@nestjs/bullmq';

@Module({
  imports: [
    BullModule.registerQueue({ name: 'notifications' }),
  ],
  controllers: [OrdersController],
  providers: [
    CreateOrderUseCase,
    { provide: OrderRepository, useClass: PrismaOrderRepository },
  ],
  exports: [CreateOrderUseCase],
})
export class OrdersModule {}
```

---

## Domain Model

```typescript
// Value Object
export class Money {
  constructor(
    public readonly amountCents: number,  // store as integer cents
    public readonly currency: string,
  ) {
    if (amountCents < 0) throw new Error('Amount cannot be negative');
    if (!/^[A-Z]{3}$/.test(currency)) throw new Error('Invalid currency code');
  }

  add(other: Money): Money {
    if (this.currency !== other.currency)
      throw new Error('Cannot add different currencies');
    return new Money(this.amountCents + other.amountCents, this.currency);
  }
}

// Entity
export class Order {
  constructor(
    public readonly id: string,
    public readonly userId: string,
    private _status: OrderStatus = OrderStatus.PENDING,
    private _total: Money = new Money(0, 'EUR'),
    public readonly createdAt: Date = new Date(),
  ) {}

  get status(): OrderStatus { return this._status; }
  get total(): Money { return this._total; }

  cancel(): void {
    if (this._status !== OrderStatus.PENDING) {
      throw new Error(`Only PENDING orders can be cancelled. Current: ${this._status}`);
    }
    this._status = OrderStatus.CANCELLED;
  }
}

export enum OrderStatus { PENDING = 'PENDING', CONFIRMED = 'CONFIRMED', CANCELLED = 'CANCELLED' }
```

---

## Prisma Schema + Repository

```prisma
// prisma/schema.prisma
generator client {
  provider = "prisma-client-js"
}

datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

model Order {
  id             String      @id @default(uuid())
  userId         String      @map("user_id")
  status         OrderStatus @default(PENDING)
  totalCents     Int         @default(0) @map("total_cents")  // integer cents — no float
  currency       String      @default("EUR") @db.Char(3)
  createdAt      DateTime    @default(now()) @map("created_at")
  updatedAt      DateTime?   @updatedAt @map("updated_at")

  @@map("orders")
  @@index([userId])
}

enum OrderStatus { PENDING CONFIRMED CANCELLED }
```

```typescript
// src/infrastructure/persistence/prisma-order.repository.ts
import { Injectable } from '@nestjs/common';
import { PrismaService } from './prisma.service';
import { OrderRepository } from '../../domain/repository/order.repository';
import { Order, OrderStatus, Money } from '../../domain/model/order';

@Injectable()
export class PrismaOrderRepository implements OrderRepository {
  constructor(private readonly prisma: PrismaService) {}

  async findById(id: string): Promise<Order | null> {
    const row = await this.prisma.order.findUnique({ where: { id } });
    return row ? this.toDomain(row) : null;
  }

  async save(order: Order): Promise<Order> {
    const row = await this.prisma.order.upsert({
      where: { id: order.id },
      create: {
        id: order.id,
        userId: order.userId,
        status: order.status,
        totalCents: order.total.amountCents,
        currency: order.total.currency,
      },
      update: {
        status: order.status,
        totalCents: order.total.amountCents,
      },
    });
    return this.toDomain(row);
  }

  private toDomain(row: any): Order {
    return new Order(row.id, row.userId, row.status as OrderStatus,
      new Money(row.totalCents, row.currency), row.createdAt);
  }
}
```

---

## Input Validation with Zod

```typescript
// src/api/pipe/zod-validation.pipe.ts
import { PipeTransform, BadRequestException } from '@nestjs/common';
import { ZodSchema } from 'zod';

export class ZodValidationPipe implements PipeTransform {
  constructor(private schema: ZodSchema) {}

  transform(value: unknown) {
    const result = this.schema.safeParse(value);
    if (!result.success) {
      throw new BadRequestException({
        code: 'VALIDATION_ERROR',
        message: 'Request validation failed',
        details: result.error.issues.map(i => ({
          field: i.path.join('.'),
          issue: i.message,
        })),
      });
    }
    return result.data;
  }
}

// Schema definition
import { z } from 'zod';

export const CreateOrderSchema = z.object({
  currency: z.string().length(3).toUpperCase(),
});
export type CreateOrderInput = z.infer<typeof CreateOrderSchema>;

// In controller
@Post()
async create(
  @Body(new ZodValidationPipe(CreateOrderSchema)) body: CreateOrderInput,
  @Headers('idempotency-key') idempotencyKey: string,
  @Req() req: Request,
) { ... }
```

---

## JWT Auth (Passport + NestJS)

```typescript
// src/infrastructure/security/jwt.strategy.ts
import { Injectable, UnauthorizedException } from '@nestjs/common';
import { PassportStrategy } from '@nestjs/passport';
import { ExtractJwt, Strategy } from 'passport-jwt';
import { ConfigService } from '@nestjs/config';

@Injectable()
export class JwtStrategy extends PassportStrategy(Strategy) {
  constructor(config: ConfigService) {
    super({
      jwtFromRequest: ExtractJwt.fromAuthHeaderAsBearerToken(),
      ignoreExpiration: false,
      secretOrKey: config.get<string>('JWT_SECRET')!,
    });
  }

  async validate(payload: { sub: string; email: string }) {
    return { userId: payload.sub, email: payload.email };
  }
}

// src/infrastructure/security/jwt-auth.guard.ts
import { Injectable, ExecutionContext, UnauthorizedException } from '@nestjs/common';
import { AuthGuard } from '@nestjs/passport';
import { Reflector } from '@nestjs/core';
import { IS_PUBLIC_KEY } from './public.decorator';

@Injectable()
export class JwtAuthGuard extends AuthGuard('jwt') {
  constructor(private reflector: Reflector) { super(); }

  canActivate(context: ExecutionContext) {
    const isPublic = this.reflector.getAllAndOverride<boolean>(IS_PUBLIC_KEY, [
      context.getHandler(), context.getClass(),
    ]);
    if (isPublic) return true;
    return super.canActivate(context);
  }

  handleRequest(err: any, user: any) {
    if (err || !user) throw new UnauthorizedException('Invalid or expired token');
    return user;
  }
}

// Apply globally in AppModule
providers: [{ provide: APP_GUARD, useClass: JwtAuthGuard }]

// Mark public endpoints with decorator
export const Public = () => SetMetadata(IS_PUBLIC_KEY, true);
```

---

## BullMQ (Async Queues)

```typescript
// Producer — in use case or service
import { InjectQueue } from '@nestjs/bullmq';
import { Queue } from 'bullmq';

@Injectable()
export class CreateOrderUseCase {
  constructor(
    private readonly orderRepository: OrderRepository,
    @InjectQueue('notifications') private readonly notifQueue: Queue,
  ) {}

  async execute(command: CreateOrderCommand): Promise<Order> {
    const order = new Order(randomUUID(), command.userId);
    await this.orderRepository.save(order);
    await this.notifQueue.add('order-confirmation', { orderId: order.id }, {
      attempts: 3,
      backoff: { type: 'exponential', delay: 1000 },
    });
    return order;
  }
}

// Consumer
import { Processor, WorkerHost } from '@nestjs/bullmq';
import { Job } from 'bullmq';

@Processor('notifications')
export class NotificationsConsumer extends WorkerHost {
  constructor(private readonly mailer: MailService) { super(); }

  async process(job: Job): Promise<void> {
    if (job.name === 'order-confirmation') {
      await this.mailer.sendOrderConfirmation(job.data.orderId);
    }
  }
}
```

---

## Global Exception Filter

```typescript
import { ExceptionFilter, Catch, ArgumentsHost, HttpException, HttpStatus } from '@nestjs/common';
import { Response } from 'express';

@Catch()
export class GlobalExceptionFilter implements ExceptionFilter {
  catch(exception: unknown, host: ArgumentsHost) {
    const ctx = host.switchToHttp();
    const response = ctx.getResponse<Response>();

    if (exception instanceof HttpException) {
      const status = exception.getStatus();
      const body = exception.getResponse();
      response.status(status).json(
        typeof body === 'string'
          ? { code: 'HTTP_ERROR', message: body }
          : body
      );
      return;
    }

    if (exception instanceof Error && exception.message.includes('Only PENDING')) {
      response.status(422).json({ code: 'BUSINESS_RULE_VIOLATION', message: exception.message });
      return;
    }

    // Never leak stack traces in production
    console.error(exception);
    response.status(500).json({ code: 'INTERNAL_ERROR', message: 'An error occurred' });
  }
}
```

---

## Testing — Jest + Supertest

```typescript
// Unit test — pure domain
describe('Order', () => {
  it('cancels a PENDING order', () => {
    const order = new Order(randomUUID(), 'user-1');
    order.cancel();
    expect(order.status).toBe(OrderStatus.CANCELLED);
  });

  it('throws when cancelling a non-PENDING order', () => {
    const order = new Order(randomUUID(), 'user-1');
    order.cancel();
    expect(() => order.cancel()).toThrow('Only PENDING orders can be cancelled');
  });
});

// Application layer — mock repository
describe('CreateOrderUseCase', () => {
  let useCase: CreateOrderUseCase;
  let repo: jest.Mocked<OrderRepository>;

  beforeEach(() => {
    repo = { findById: jest.fn(), save: jest.fn(), findByUserId: jest.fn() };
    useCase = new CreateOrderUseCase(repo, notifQueueMock);
  });

  it('saves a new order and returns it', async () => {
    repo.save.mockImplementation(async (o) => o);
    const result = await useCase.execute({ userId: 'user-1', currency: 'EUR',
      idempotencyKey: 'key-1' });
    expect(result.status).toBe(OrderStatus.PENDING);
    expect(repo.save).toHaveBeenCalledTimes(1);
  });
});

// Integration test — full HTTP round-trip
describe('POST /api/orders', () => {
  let app: INestApplication;

  beforeAll(async () => {
    const module = await Test.createTestingModule({
      imports: [AppModule],
    }).compile();
    app = module.createNestApplication();
    app.useGlobalFilters(new GlobalExceptionFilter());
    app.useGlobalPipes(new ValidationPipe());
    await app.init();
  });

  afterAll(() => app.close());

  it('creates an order and returns 201', async () => {
    const token = await getTestJwt(app);
    return request(app.getHttpServer())
      .post('/api/orders')
      .set('Authorization', `Bearer ${token}`)
      .set('Idempotency-Key', randomUUID())
      .send({ currency: 'EUR' })
      .expect(201)
      .expect(res => {
        expect(res.body.status).toBe('PENDING');
        expect(res.body.currency).toBe('EUR');
      });
  });
});
```

---

## TypeScript Strict Patterns

```typescript
// tsconfig.json — always use strict mode
{
  "compilerOptions": {
    "strict": true,
    "noImplicitAny": true,
    "strictNullChecks": true,
    "noUncheckedIndexedAccess": true
  }
}

// ❌ Never use `any` without justification
const data: any = response.body;  // unsafe

// ✅ Use unknown + type guard
const data: unknown = response.body;
if (isOrderResponse(data)) { ... }
function isOrderResponse(v: unknown): v is OrderResponse {
  return typeof v === 'object' && v !== null && 'id' in v && 'status' in v;
}

// ✅ Utility types for partials / picks
type UpdateOrderDto = Partial<Pick<Order, 'status' | 'total'>>;

// ✅ Discriminated union for Result type
type Result<T, E = Error> =
  | { ok: true; value: T }
  | { ok: false; error: E };
```

---

## Common Gotchas

🔴 **Circular dependency between modules** — NestJS throws at startup. Use `forwardRef()` as a temporary fix, but the real fix is to restructure: extract the shared dependency into a shared module.

🔴 **Prisma client not initialised in tests** — create a `PrismaService` with `onModuleInit` → `prisma.$connect()` and `onModuleDestroy` → `prisma.$disconnect()`. In tests, override with a `TestingModule` that uses a test database.

🟠 **BullMQ jobs lost on Redis restart** — configure `defaultJobOptions: { removeOnComplete: false, removeOnFail: false }` in production so you can inspect and replay failed jobs.

🟠 **`async/await` in NestJS lifecycle hooks** — `onModuleInit()` and `onModuleDestroy()` support `async`; always `await` async operations in these hooks or they silently fail.

🔵 **`ConfigService` not available in decorators** — decorator factories run at class definition time, before DI is initialised. Use `ConfigService` in constructor injection, not in `@Column()` or similar metadata decorators.
