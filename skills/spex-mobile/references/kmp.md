# Kotlin Multiplatform (KMP) Deep Reference

## Project Structure

```
shared/
├── commonMain/         ← business logic, domain, data (pure Kotlin)
│   ├── domain/
│   │   ├── model/
│   │   └── usecase/
│   ├── data/
│   │   ├── repository/
│   │   └── remote/     ← Ktor client
│   └── Platform.kt     ← expect declarations
├── androidMain/        ← Android actual implementations
│   └── Platform.android.kt
├── iosMain/            ← iOS actual implementations
│   └── Platform.ios.kt
androidApp/             ← Android host app
iosApp/                 ← iOS host app (Xcode project)
```

### What goes where
| Layer | Location |
|---|---|
| Models, business rules, use cases | `commonMain` |
| Ktor networking | `commonMain` |
| SQLDelight database | `commonMain` schema + generated code |
| Platform APIs (file system, GPS, biometrics) | `expect`/`actual` |
| UI (Compose / SwiftUI) | Platform-specific apps |

---

## expect / actual

```kotlin
// commonMain/Platform.kt
expect class Platform() {
    val name: String
}

expect fun getCurrentTimestamp(): Long

// androidMain/Platform.android.kt
actual class Platform actual constructor() {
    actual val name: String = "Android ${android.os.Build.VERSION.SDK_INT}"
}

actual fun getCurrentTimestamp(): Long = System.currentTimeMillis()

// iosMain/Platform.ios.kt
actual class Platform actual constructor() {
    actual val name: String = UIDevice.currentDevice.systemName()
}

actual fun getCurrentTimestamp(): Long =
    (NSDate().timeIntervalSince1970 * 1000).toLong()
```

### When to use expect/actual
- Platform-specific system APIs (camera, GPS, biometrics, filesystem)
- Logging (use `expect fun log(tag: String, msg: String)`)
- Dispatchers if needed (`Dispatchers.Main` is available via `kotlinx-coroutines-core` — prefer that)
- **Do NOT** use expect/actual just to share a class name — share the full implementation in `commonMain` instead

---

## Ktor (Networking in commonMain)

```kotlin
// commonMain
val client = HttpClient {
    install(ContentNegotiation) { json(Json { ignoreUnknownKeys = true }) }
    install(HttpTimeout) { requestTimeoutMillis = 15_000 }
}

suspend fun fetchUser(id: String): User =
    client.get("$BASE_URL/users/$id").body()

// gradle - commonMain dependencies
implementation("io.ktor:ktor-client-core:$ktorVersion")
implementation("io.ktor:ktor-client-content-negotiation:$ktorVersion")
implementation("io.ktor:ktor-serialization-kotlinx-json:$ktorVersion")

// androidMain
implementation("io.ktor:ktor-client-okhttp:$ktorVersion")

// iosMain
implementation("io.ktor:ktor-client-darwin:$ktorVersion")
```

---

## SQLDelight (Local DB in commonMain)

```sql
-- commonMain/sqldelight/com/example/User.sq
CREATE TABLE User (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL
);

selectAll:
SELECT * FROM User;

upsert:
INSERT OR REPLACE INTO User VALUES (?, ?, ?);
```

```kotlin
// Usage in repository (commonMain)
class UserRepository(private val db: AppDatabase) {
    fun observeUsers(): Flow<List<User>> = db.userQueries.selectAll().asFlow().mapToList()
    suspend fun upsert(user: User) = withContext(Dispatchers.IO) {
        db.userQueries.upsert(user.id, user.name, user.email)
    }
}

// gradle
implementation("app.cash.sqldelight:runtime:$sqlDelightVersion")
// androidMain: implementation("app.cash.sqldelight:android-driver:$sqlDelightVersion")
// iosMain: implementation("app.cash.sqldelight:native-driver:$sqlDelightVersion")
```

---

## Exposing KMP to iOS (Swift interop)

KMP compiles to a native framework for iOS. Key constraints:
- **Only classes annotated `@ObjCName` or standard Kotlin classes are cleanly visible in Swift**
- Kotlin `Flow` doesn't bridge directly — wrap with a helper or use KMP-NativeCoroutines / SKIE

### Option 1: SKIE (recommended, cleanest Swift API)
```kotlin
// Just use Flow normally in commonMain — SKIE generates Swift async sequences
// Add SKIE Gradle plugin and it handles the rest
```

### Option 2: KMP-NativeCoroutines
```kotlin
@NativeCoroutines
suspend fun getUser(id: String): User  // becomes async func in Swift
@NativeCoroutinesState
val userState: StateFlow<User>         // becomes AsyncStream in Swift
```

### Option 3: Manual wrapper (no library)
```kotlin
// iosMain — wrap Flow in a callback-based class Swift can call
class UserStateWrapper(private val viewModel: UserViewModel) {
    fun subscribe(onEach: (User) -> Unit, onError: (Throwable) -> Unit): Cancellable {
        val job = CoroutineScope(Dispatchers.Main).launch {
            viewModel.userState.collect { onEach(it) }
        }
        return Cancellable { job.cancel() }
    }
}
```

---

## Gradle Setup (libs.versions.toml)

```toml
[versions]
kotlin = "2.0.x"
ktor = "2.x.x"
sqldelight = "2.x.x"
coroutines = "1.8.x"

[libraries]
ktor-core = { module = "io.ktor:ktor-client-core", version.ref = "ktor" }
sqldelight-runtime = { module = "app.cash.sqldelight:runtime", version.ref = "sqldelight" }
coroutines-core = { module = "org.jetbrains.kotlinx:kotlinx-coroutines-core", version.ref = "coroutines" }

[plugins]
kotlin-multiplatform = { id = "org.jetbrains.kotlin.multiplatform", version.ref = "kotlin" }
sqldelight = { id = "app.cash.sqldelight", version.ref = "sqldelight" }
```

---

## Common KMP Gotchas

🔴 **Freezing (legacy — Kotlin/Native pre-1.7.20)**: Old memory model required shared objects to be frozen. New memory model (default since 1.7.20) removes this — but check if dependencies still use the old model.

🟠 **`Dispatchers.Main` on iOS**: Requires `kotlinx-coroutines-core` with the native-mt variant OR use the new memory model (≥1.7.20). Verify it's included.

🟠 **iOS framework size**: Use `--mode=Release` for production. Debug frameworks are huge.

🟠 **Xcode integration**: Use the Kotlin Gradle plugin's `embedAndSignAppleFrameworkForXcode` task — don't manually copy the framework.

🔵 **Naming conflicts**: Kotlin `data class` generates `copy()` — can conflict with Swift's `NSCopying`. Use `@ObjCName` to rename if needed.

---

## Compose Multiplatform (CMP) — Shared UI

```
shared/
├── commonMain/
│   └── ui/               ← Shared @Composable components (CMP)
│       ├── components/
│       └── screens/
├── androidMain/
│   └── ui/               ← Android-specific previews, platform UI
└── iosMain/
    └── ui/               ← iOS entry point for Compose rendering
```

```kotlin
// commonMain — shared Composable (works on Android + iOS + Desktop + Web)
@Composable
fun GreetingCard(name: String, modifier: Modifier = Modifier) {
    Card(modifier = modifier.padding(16.dp)) {
        Text("Hello, $name!", style = MaterialTheme.typography.headlineSmall)
    }
}

// androidMain — preview (Android only)
@Preview @Composable
fun GreetingCardPreview() = GreetingCard("World")

// iosMain — iOS entry point
fun MainViewController() = ComposeUIViewController { App() }
// In Xcode: ComposeView().makeUIViewController()
```

---

## CMP resources API

```kotlin
// commonMain/composeResources/values/strings.xml (standard XML)
// commonMain/composeResources/drawable/logo.png

// Usage in commonMain Composable
import org.jetbrains.compose.resources.*

@Composable fun LogoImage() {
    Image(painterResource(Res.drawable.logo), contentDescription = "Logo")
}

Text(stringResource(Res.string.app_name))
```

---

## CMP vs native UI — decision table

| Criterion | CMP Shared UI | Native (SwiftUI/Compose) |
|-----------|--------------|--------------------------|
| Code sharing | Maximum (one codebase) | Minimal |
| Platform look & feel | Custom / Material3 only | Full native widgets |
| iOS animation fidelity | Good (Compose animations) | Excellent (UIKit-backed) |
| Xcode previews | ❌ | ✅ |
| Access to platform-specific UI APIs | Via `expect`/`actual` wrappers | Direct |
| Best for | Internal tools, B2B, startups | Consumer apps with high native fidelity requirement |

---

## iOS Integration: CocoaPods vs Swift Package Manager

```kotlin
// CocoaPods (traditional)
cocoapods {
    summary = "Shared KMP module"
    homepage = "https://example.com"
    ios.deploymentTarget = "16.0"
    framework { baseName = "shared" }
    // In iosApp/Podfile: pod 'shared', :path => '../'
}

// Swift Package Manager (modern, Kotlin 2.0+)
// Add to iosApp via: File → Add Package Dependencies → local path
// Uses XCFramework artifact from Gradle
```

```kotlin
// Gradle task to produce XCFramework for SPM
kotlin {
    listOf(iosX64(), iosArm64(), iosSimulatorArm64()).forEach {
        it.binaries.framework { baseName = "shared"; isStatic = true }
    }
}
// Run: ./gradlew assembleSharedXCFramework
// Then reference from Package.swift or drag into Xcode
```

---

## Testing in KMP

```kotlin
// commonTest — runs on all platforms
class UserRepositoryTest {
    @Test
    fun `upsert then fetch returns user`() = runTest {  // kotlinx-coroutines-test
        val repo = UserRepository(InMemoryDatabase())
        repo.upsert(User("1", "Alice"))
        assertEquals("Alice", repo.getUser("1")?.name)
    }
}

// androidTest — uses JUnit4 + Robolectric or device
// iosTest — runs via Kotlin/Native test runner (kotlin.test)

// Gradle
commonTest {
    dependencies {
        implementation(kotlin("test"))
        implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:$coroutinesVersion")
        implementation("app.cash.turbine:turbine:$turbineVersion")
    }
}
```

---

## SKIE detailed setup

```kotlin
// build.gradle.kts (shared module)
plugins {
    id("co.touchlab.skie") version "0.x.x"
}

// SKIE automatically:
// - Converts Kotlin Flow → Swift AsyncSequence
// - Converts suspend fun → Swift async func
// - Converts sealed class → Swift enum with associated values
// - Converts Kotlin default parameters → Swift overloads

// Example: after SKIE, Swift can do:
// for await user in viewModel.usersFlow { ... }  (no wrapper needed)
```
