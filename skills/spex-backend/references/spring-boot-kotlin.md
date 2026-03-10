# Kotlin / Spring Boot 3 — Deep Reference

## Project Structure (Gradle Kotlin DSL)

```
src/
├── main/kotlin/com/example/app/
│   ├── domain/
│   │   ├── model/          ← Data classes (entities, value objects)
│   │   ├── repository/     ← Repository interfaces
│   │   ├── event/          ← Domain events
│   │   └── service/        ← Domain services
│   ├── application/
│   │   ├── usecase/        ← Application services / use cases
│   │   └── dto/            ← Request/Response DTOs
│   ├── infrastructure/
│   │   ├── persistence/    ← Spring Data JPA repository implementations
│   │   ├── messaging/      ← Event publishers, listeners
│   │   └── security/       ← JWT filter, UserDetailsService
│   └── api/
│       ├── controller/     ← REST controllers
│       ├── request/        ← Request body classes
│       ├── response/       ← Response body classes
│       └── advice/         ← @ControllerAdvice (error handling)
└── test/kotlin/...
```

```kotlin
// build.gradle.kts
plugins {
    kotlin("jvm") version "2.0.x"
    kotlin("plugin.spring") version "2.0.x"
    kotlin("plugin.jpa") version "2.0.x"
    id("org.springframework.boot") version "3.x.x"
    id("io.spring.dependency-management") version "1.x.x"
}

dependencies {
    implementation("org.springframework.boot:spring-boot-starter-web")
    implementation("org.springframework.boot:spring-boot-starter-data-jpa")
    implementation("org.springframework.boot:spring-boot-starter-security")
    implementation("org.springframework.boot:spring-boot-starter-validation")
    implementation("io.jsonwebtoken:jjwt-api:0.12.x")
    runtimeOnly("io.jsonwebtoken:jjwt-impl:0.12.x")
    runtimeOnly("io.jsonwebtoken:jjwt-jackson:0.12.x")
    runtimeOnly("org.postgresql:postgresql")
    testImplementation("org.springframework.boot:spring-boot-starter-test")
    testImplementation("io.kotest:kotest-runner-junit5:5.x.x")
    testImplementation("io.kotest:kotest-assertions-core:5.x.x")
    testImplementation("io.mockk:mockk:1.x.x")
    testImplementation("org.testcontainers:postgresql:1.x.x")
}

tasks.withType<Test> { useJUnitPlatform() }

// Required for JPA entities with Kotlin (generates no-arg constructor)
allOpen {
    annotation("jakarta.persistence.Entity")
    annotation("jakarta.persistence.MappedSuperclass")
}
```

---

## Domain Model — Entities + Value Objects

```kotlin
// Value Object — immutable, equality by value
@Embeddable
data class Money(
    @Column(nullable = false, precision = 19, scale = 4)
    val amount: BigDecimal,
    @Column(nullable = false, length = 3)
    val currency: String
) {
    init {
        require(amount >= BigDecimal.ZERO) { "Amount cannot be negative" }
        require(currency.length == 3) { "Currency must be ISO 4217 3-letter code" }
    }

    fun add(other: Money): Money {
        require(currency == other.currency) { "Cannot add different currencies" }
        return copy(amount = amount + other.amount)
    }
}

// Entity
@Entity
@Table(name = "orders")
class Order(
    @Id
    @Column(columnDefinition = "uuid")
    val id: UUID = UUID.randomUUID(),

    @Column(nullable = false, columnDefinition = "uuid")
    val userId: UUID,

    @Enumerated(EnumType.STRING)
    @Column(nullable = false)
    var status: OrderStatus = OrderStatus.PENDING,

    @Embedded
    var total: Money = Money(BigDecimal.ZERO, "EUR"),

    @Column(nullable = false, updatable = false)
    val createdAt: Instant = Instant.now(),

    @Column
    var updatedAt: Instant? = null
) {
    // Domain behaviour encapsulated in entity
    fun cancel() {
        check(status == OrderStatus.PENDING) {
            "Only PENDING orders can be cancelled, current status: $status"
        }
        status = OrderStatus.CANCELLED
        updatedAt = Instant.now()
    }
}

enum class OrderStatus { PENDING, CONFIRMED, CANCELLED }
```

**Kotlin + JPA gotchas:**
- JPA entities need a no-arg constructor → use `kotlin("plugin.jpa")` + `allOpen`
- `data class` generates `copy()` which bypasses invariants — prefer regular `class` for aggregates
- `@Column` on `val` is fine; Spring Data JPA maps Kotlin `val` correctly

---

## Repository Pattern

```kotlin
// Domain interface (no Spring imports)
interface OrderRepository {
    fun findById(id: UUID): Order?
    fun findByUserId(userId: UUID): List<Order>
    fun save(order: Order): Order
}

// Spring Data JPA implementation (infrastructure)
interface JpaOrderRepository : JpaRepository<Order, UUID> {
    fun findByUserId(userId: UUID): List<Order>
}

@Repository
class OrderRepositoryImpl(
    private val jpa: JpaOrderRepository
) : OrderRepository {
    override fun findById(id: UUID): Order? = jpa.findById(id).orElse(null)
    override fun findByUserId(userId: UUID): List<Order> = jpa.findByUserId(userId)
    override fun save(order: Order): Order = jpa.save(order)
}
```

---

## @Transactional — Rules and Gotchas

```kotlin
@Service
class CreateOrderUseCase(
    private val orderRepository: OrderRepository,
    private val idempotencyRepository: IdempotencyRepository,
    private val eventPublisher: ApplicationEventPublisher,
) {
    @Transactional  // wraps the entire use case in one transaction
    fun execute(command: CreateOrderCommand): Order {
        // 1. Check idempotency key
        idempotencyRepository.findByKey(command.idempotencyKey)?.let {
            return orderRepository.findById(it.resourceId)!!  // return cached result
        }

        // 2. Create aggregate
        val order = Order(userId = command.userId, total = Money(BigDecimal.ZERO, command.currency))

        // 3. Persist
        val saved = orderRepository.save(order)

        // 4. Record idempotency key
        idempotencyRepository.save(IdempotencyKey(command.idempotencyKey, saved.id))

        // 5. Publish domain event (transactional outbox or Spring event)
        eventPublisher.publishEvent(OrderCreated(saved.id, saved.userId))

        return saved
    }
}
```

**@Transactional rules:**
- Apply at the **use-case / application service** level, not in repositories
- Default propagation is `REQUIRED` (join existing or create new) — correct for most cases
- `@Transactional` on `private` methods does **nothing** — Spring proxy can't intercept them
- Self-invocation (`this.method()`) bypasses the proxy — inject `self` or extract to a separate bean
- Use `@Transactional(readOnly = true)` for queries — enables Hibernate read optimisations

---

## Spring Security + JWT

```kotlin
// Security config
@Configuration
@EnableWebSecurity
class SecurityConfig(private val jwtFilter: JwtAuthFilter) {

    @Bean
    fun securityFilterChain(http: HttpSecurity): SecurityFilterChain = http
        .csrf { it.disable() }
        .sessionManagement { it.sessionCreationPolicy(SessionCreationPolicy.STATELESS) }
        .authorizeHttpRequests {
            it.requestMatchers("/api/auth/**").permitAll()
            it.anyRequest().authenticated()
        }
        .addFilterBefore(jwtFilter, UsernamePasswordAuthenticationFilter::class.java)
        .build()

    @Bean
    fun passwordEncoder(): PasswordEncoder = BCryptPasswordEncoder()
}

// JWT filter
@Component
class JwtAuthFilter(private val jwtService: JwtService,
                    private val userDetailsService: UserDetailsService) :
    OncePerRequestFilter() {

    override fun doFilterInternal(request: HttpServletRequest,
                                  response: HttpServletResponse,
                                  chain: FilterChain) {
        val header = request.getHeader("Authorization")
        if (header == null || !header.startsWith("Bearer ")) {
            chain.doFilter(request, response); return
        }
        val token = header.substring(7)
        val username = jwtService.extractUsername(token)
        if (username != null && SecurityContextHolder.getContext().authentication == null) {
            val userDetails = userDetailsService.loadUserByUsername(username)
            if (jwtService.isTokenValid(token, userDetails)) {
                val authToken = UsernamePasswordAuthenticationToken(
                    userDetails, null, userDetails.authorities)
                authToken.details = WebAuthenticationDetailsSource().buildDetails(request)
                SecurityContextHolder.getContext().authentication = authToken
            }
        }
        chain.doFilter(request, response)
    }
}

// JWT service
@Service
class JwtService(@Value("\${jwt.secret}") private val secret: String) {
    private val key by lazy {
        Keys.hmacShaKeyFor(Decoders.BASE64.decode(secret))
    }

    fun generateToken(username: String, extraClaims: Map<String, Any> = emptyMap()): String =
        Jwts.builder()
            .claims(extraClaims)
            .subject(username)
            .issuedAt(Date())
            .expiration(Date(System.currentTimeMillis() + 900_000)) // 15 min
            .signWith(key)
            .compact()

    fun extractUsername(token: String): String? = runCatching {
        Jwts.parser().verifyWith(key).build().parseSignedClaims(token).payload.subject
    }.getOrNull()

    fun isTokenValid(token: String, userDetails: UserDetails): Boolean =
        extractUsername(token) == userDetails.username && !isTokenExpired(token)

    private fun isTokenExpired(token: String): Boolean =
        Jwts.parser().verifyWith(key).build()
            .parseSignedClaims(token).payload.expiration.before(Date())
}
```

---

## Global Exception Handling

```kotlin
@RestControllerAdvice
class GlobalExceptionHandler {

    @ExceptionHandler(EntityNotFoundException::class)
    fun handleNotFound(e: EntityNotFoundException): ResponseEntity<ErrorResponse> =
        ResponseEntity.status(404).body(ErrorResponse("NOT_FOUND", e.message ?: ""))

    @ExceptionHandler(IllegalStateException::class)
    fun handleDomainError(e: IllegalStateException): ResponseEntity<ErrorResponse> =
        ResponseEntity.status(422).body(ErrorResponse("BUSINESS_RULE_VIOLATION", e.message ?: ""))

    @ExceptionHandler(MethodArgumentNotValidException::class)
    fun handleValidation(e: MethodArgumentNotValidException): ResponseEntity<ErrorResponse> {
        val details = e.bindingResult.fieldErrors.map {
            ErrorDetail(it.field, it.defaultMessage ?: "invalid")
        }
        return ResponseEntity.status(400)
            .body(ErrorResponse("VALIDATION_ERROR", "Request validation failed", details))
    }

    @ExceptionHandler(Exception::class)
    fun handleGeneric(e: Exception): ResponseEntity<ErrorResponse> {
        // Never leak stack trace — log it, return generic message
        return ResponseEntity.status(500).body(ErrorResponse("INTERNAL_ERROR", "An error occurred"))
    }
}

data class ErrorResponse(
    val code: String,
    val message: String,
    val details: List<ErrorDetail> = emptyList()
)
data class ErrorDetail(val field: String, val issue: String)
```

---

## Coroutines with Spring MVC

```kotlin
// Spring MVC 6 supports suspend functions natively
@RestController
@RequestMapping("/api/orders")
class OrderController(private val useCase: CreateOrderUseCase) {

    @PostMapping
    suspend fun create(
        @RequestBody @Valid request: CreateOrderRequest,
        @RequestHeader("Idempotency-Key") idempotencyKey: String,
        authentication: Authentication
    ): ResponseEntity<OrderResponse> = withContext(Dispatchers.IO) {
        val order = useCase.execute(CreateOrderCommand(
            idempotencyKey = idempotencyKey,
            currency = request.currency,
            userId = UUID.fromString(authentication.name)
        ))
        ResponseEntity.status(201).body(OrderResponse.from(order))
    }
}
```

---

## Testing — Kotest + MockK

```kotlin
// Unit test (pure domain, no Spring context)
class OrderTest : FunSpec({
    test("cancel a PENDING order") {
        val order = Order(userId = UUID.randomUUID(), total = Money(BigDecimal.TEN, "EUR"))
        order.cancel()
        order.status shouldBe OrderStatus.CANCELLED
    }

    test("cancelling a CANCELLED order throws") {
        val order = Order(userId = UUID.randomUUID(), total = Money(BigDecimal.TEN, "EUR"))
        order.cancel()
        shouldThrow<IllegalStateException> { order.cancel() }
    }
})

// Application layer test with MockK
class CreateOrderUseCaseTest : FunSpec({
    val orderRepo = mockk<OrderRepository>()
    val idempotencyRepo = mockk<IdempotencyRepository>()
    val eventPublisher = mockk<ApplicationEventPublisher>(relaxed = true)
    val useCase = CreateOrderUseCase(orderRepo, idempotencyRepo, eventPublisher)

    test("creates and saves a new order") {
        val command = CreateOrderCommand("idem-key-1", "EUR", UUID.randomUUID())
        every { idempotencyRepo.findByKey("idem-key-1") } returns null
        every { orderRepo.save(any()) } answers { firstArg() }
        every { idempotencyRepo.save(any()) } answers { firstArg() }

        val result = useCase.execute(command)

        result.status shouldBe OrderStatus.PENDING
        verify(exactly = 1) { orderRepo.save(any()) }
        verify(exactly = 1) { eventPublisher.publishEvent(any<OrderCreated>()) }
    }
})
```

---

## Integration Test with Testcontainers

```kotlin
@SpringBootTest(webEnvironment = SpringBootTest.WebEnvironment.RANDOM_PORT)
@Testcontainers
class OrderIntegrationTest(@Autowired val restTemplate: TestRestTemplate,
                           @Autowired val orderRepository: JpaOrderRepository) {

    companion object {
        @Container
        @JvmStatic
        val postgres = PostgreSQLContainer<Nothing>("postgres:16").apply {
            withDatabaseName("testdb")
        }

        @DynamicPropertySource
        @JvmStatic
        fun props(registry: DynamicPropertyRegistry) {
            registry.add("spring.datasource.url") { postgres.jdbcUrl }
            registry.add("spring.datasource.username") { postgres.username }
            registry.add("spring.datasource.password") { postgres.password }
        }
    }

    @Test
    fun `POST orders creates order and returns 201`() {
        val token = getAuthToken() // helper to get JWT
        val headers = HttpHeaders().apply {
            setBearerAuth(token)
            set("Idempotency-Key", UUID.randomUUID().toString())
        }
        val request = HttpEntity(mapOf("currency" to "EUR"), headers)
        val response = restTemplate.postForEntity("/api/orders", request, Map::class.java)

        assertEquals(201, response.statusCode.value())
        assertEquals("PENDING", response.body!!["status"])
        assertEquals(1, orderRepository.count())
    }
}
```

---

## Common Gotchas

🔴 **`data class` for JPA entities** — `data class` generates `equals()`/`hashCode()` based on all fields, which breaks Hibernate's entity identity tracking. Use plain `class` for entities; `data class` is fine for DTOs and value objects.

🔴 **`@Transactional` on `private` / `final` methods** — Spring AOP proxy can't intercept these. Make transactional methods `open` (or use `allOpen` in build config) and `public`.

🟠 **Lazy loading outside a transaction** — accessing a lazy-loaded relation after the transaction closes throws `LazyInitializationException`. Use `@EntityGraph`, `JOIN FETCH`, or projections to eagerly load what you need.

🟠 **Coroutines + `@Transactional`** — `@Transactional` uses thread-local storage; Kotlin coroutines can switch threads. Use `Transactional` with `Dispatchers.IO` carefully, or use Spring's `TransactionalOperator` for reactive-style transaction management.

🔵 **Kotlin null safety vs JPA** — JPA can return `null` from `find()` but Kotlin non-null types will crash. Always use `Optional<T>` or call `.orElse(null)` and handle the nullable result.
