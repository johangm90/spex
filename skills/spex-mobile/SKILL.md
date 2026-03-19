---
name: spex-mobile
description: >
  Use this skill when asked to build a native Android app (Kotlin, Jetpack Compose,
  MVVM/MVI, Coroutines, Hilt, Room), a native iOS app (Swift, SwiftUI, async/await,
  Combine, SwiftData), or a Kotlin Multiplatform / Compose Multiplatform (KMP/CMP)
  shared codebase. Also use for writing Kotlin or Swift native modules, designing
  shared KMP business logic, deciding between MVVM and MVI, implementing offline-first
  patterns on mobile, setting up Hilt DI, Room/SQLDelight databases, or integrating
  Ktor for cross-platform networking. Triggers: Android, iOS, Kotlin, Swift, Compose,
  SwiftUI, KMP, KMM, CMP, Jetpack, Hilt, Room, Ktor, SQLDelight, mobile architecture.
---

# Skill: spex-mobile

You are a senior mobile engineer and architect with deep expertise in:
- **Android**: Kotlin, Jetpack Compose, MVVM/MVI, Coroutines, Hilt, Room, Gradle
- **iOS**: Swift, SwiftUI, async/await, Combine, Xcode toolchain
- **KMP/CMP**: Kotlin Multiplatform + Compose Multiplatform, shared business logic, `expect`/`actual`, Ktor, SQLDelight

## Platform Reference Files

| File | Contents |
|------|----------|
| [references/android.md](references/android.md) | Compose patterns, Hilt DI, ViewModel/Coroutines, Room, Navigation, animations, WorkManager, testing |
| [references/ios.md](references/ios.md) | SwiftUI patterns, async/await, navigation, SwiftData/Core Data, Combine, testing, Instruments |
| [references/kmp.md](references/kmp.md) | Project structure, expect/actual, Ktor, SQLDelight, iOS interop (SKIE/KMP-NativeCoroutines), CMP shared UI, testing |
| [references/mcp-protocol.md](references/mcp-protocol.md) | MCP integration for this project (state check, artifact_register, handoff envelope) |

---

## Architecture Decision Framework

### MVVM vs MVI — when to use which

| Signal | Use MVVM | Use MVI |
|--------|----------|---------|
| Screen complexity | Simple–medium | Complex, many states |
| Team familiarity | Default choice | Already using Redux/Flux |
| Undo/redo needed | ✗ | ✓ |
| State reproducibility tests | Less critical | Critical |
| Android Jetpack integration | `ViewModel` + `StateFlow` + `collectAsStateWithLifecycle` | Same foundation, stricter intent routing |

**Rule of thumb**: Start with MVVM. Migrate to MVI if you're writing many `when (state)` branches across multiple features and finding state bugs hard to reproduce.

---

## MVVM — Canonical Implementation

### Android (Kotlin + Compose)

```kotlin
// 1. UI State — immutable data class
data class ProfileUiState(
    val profile: Profile? = null,
    val isLoading: Boolean = false,
    val error: String? = null
)

// 2. ViewModel
@HiltViewModel
class ProfileViewModel @Inject constructor(
    private val profileRepo: ProfileRepository
) : ViewModel() {
    private val _state = MutableStateFlow(ProfileUiState())
    val state: StateFlow<ProfileUiState> = _state.asStateFlow()

    // One-shot events: navigation, snackbars
    private val _events = Channel<ProfileEvent>(Channel.BUFFERED)
    val events = _events.receiveAsFlow()

    fun load(userId: String) = viewModelScope.launch {
        _state.update { it.copy(isLoading = true) }
        profileRepo.getProfile(userId)
            .onSuccess { p -> _state.update { it.copy(profile = p, isLoading = false) } }
            .onFailure { e -> _state.update { it.copy(error = e.message, isLoading = false) } }
    }
}

// 3. Composable — collect state with lifecycle awareness
@Composable
fun ProfileScreen(
    viewModel: ProfileViewModel = hiltViewModel(),
    onNavigateBack: () -> Unit
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val lifecycleOwner = LocalLifecycleOwner.current

    LaunchedEffect(lifecycleOwner) {
        viewModel.events.flowWithLifecycle(lifecycleOwner.lifecycle)
            .collect { event ->
                when (event) {
                    ProfileEvent.NavigateBack -> onNavigateBack()
                }
            }
    }

    when {
        state.isLoading -> CircularProgressIndicator()
        state.error != null -> ErrorState(state.error!!, onRetry = { viewModel.load(userId) })
        state.profile != null -> ProfileContent(state.profile!!)
    }
}
```

### iOS (Swift + SwiftUI)

```swift
// 1. ViewModel (iOS 17+ @Observable)
@Observable
class ProfileViewModel {
    var profile: Profile?
    var isLoading = false
    var error: String?

    private let repo: ProfileRepository

    init(repo: ProfileRepository = .live) {
        self.repo = repo
    }

    func load(userId: String) async {
        isLoading = true
        defer { isLoading = false }
        do {
            profile = try await repo.fetchProfile(userId)
        } catch {
            self.error = error.localizedDescription
        }
    }
}

// 2. View
struct ProfileView: View {
    @State private var vm = ProfileViewModel()
    let userId: String

    var body: some View {
        Group {
            if vm.isLoading {
                ProgressView()
            } else if let profile = vm.profile {
                ProfileContent(profile: profile)
            } else if let error = vm.error {
                ErrorView(message: error, onRetry: { Task { await vm.load(userId: userId) } })
            }
        }
        .task { await vm.load(userId: userId) }
        .navigationTitle("Profile")
    }
}
```

---

## MVI — Canonical Implementation

### Android (Kotlin + Compose)

```kotlin
// 1. Contract
sealed interface ProfileIntent {
    data class Load(val userId: String) : ProfileIntent
    object Retry : ProfileIntent
}

data class ProfileState(
    val profile: Profile? = null,
    val isLoading: Boolean = false,
    val error: String? = null
)

sealed interface ProfileEffect {
    data class ShowSnackbar(val message: String) : ProfileEffect
    object NavigateBack : ProfileEffect
}

// 2. ViewModel as Intent processor
@HiltViewModel
class ProfileViewModel @Inject constructor(
    private val repo: ProfileRepository
) : ViewModel() {
    private val _state = MutableStateFlow(ProfileState())
    val state = _state.asStateFlow()

    private val _effects = Channel<ProfileEffect>(Channel.BUFFERED)
    val effects = _effects.receiveAsFlow()

    fun dispatch(intent: ProfileIntent) {
        when (intent) {
            is ProfileIntent.Load -> loadProfile(intent.userId)
            ProfileIntent.Retry -> _state.value.run { loadProfile(/* last userId */) }
        }
    }

    private fun loadProfile(userId: String) = viewModelScope.launch {
        _state.update { it.copy(isLoading = true, error = null) }
        repo.getProfile(userId)
            .onSuccess { p -> _state.update { it.copy(profile = p, isLoading = false) } }
            .onFailure { e ->
                _state.update { it.copy(error = e.message, isLoading = false) }
                _effects.send(ProfileEffect.ShowSnackbar("Failed: ${e.message}"))
            }
    }
}

// 3. Composable
@Composable
fun ProfileScreen(vm: ProfileViewModel = hiltViewModel(), userId: String) {
    val state by vm.state.collectAsStateWithLifecycle()
    // collect effects similarly to MVVM events pattern
    LaunchedEffect(Unit) { vm.dispatch(ProfileIntent.Load(userId)) }
    // render state...
}
```

---

## Clean Architecture Layers

```
┌─────────────────────────────────────────────┐
│  Presentation Layer                         │
│  Compose UI / SwiftUI views                 │
│  ViewModel / @Observable                    │
│  UiState, Intent, Effect/Event sealed types │
├─────────────────────────────────────────────┤
│  Domain Layer  (no Android/iOS imports)     │
│  Use cases (single-responsibility)          │
│  Domain models                              │
│  Repository interfaces                      │
├─────────────────────────────────────────────┤
│  Data Layer                                 │
│  Repository implementations                 │
│  Remote data sources (Retrofit / Ktor)      │
│  Local data sources (Room / SQLDelight)     │
│  DTOs + mappers                             │
└─────────────────────────────────────────────┘
```

**Rules:**
- Domain layer has **zero** platform imports — pure Kotlin/Swift
- Presentation depends on Domain; Data depends on Domain — never the reverse
- Mappers live at the boundary: DTO→Domain in Data, Domain→UiState in Presentation
- Use cases are optional for simple CRUD but required for orchestrating multiple repos

---

## Jetpack Compose Deep Patterns

### Recomposition control

```kotlin
// ❌ Lambda captures cause unnecessary recompositions
LazyColumn {
    items(list) { item ->
        ItemRow(item, onClick = { viewModel.select(item.id) }) // new lambda each compose
    }
}

// ✅ Stable key + remembered lambda
LazyColumn {
    items(list, key = { it.id }) { item ->
        val onClick = remember(item.id) { { viewModel.select(item.id) } }
        ItemRow(item, onClick = onClick)
    }
}

// ✅ @Stable on custom classes used as Compose parameters
@Stable
data class UserCardState(val name: String, val avatarUrl: String)
```

### Side effects cheat-sheet

| Effect | Use when |
|--------|----------|
| `LaunchedEffect(key)` | Run suspend code; re-run when key changes |
| `DisposableEffect(key)` | Register/unregister listeners; cleanup via `onDispose` |
| `SideEffect` | Sync Compose state → non-Compose system (e.g. analytics) |
| `rememberCoroutineScope()` | Trigger coroutines from callbacks (button click) |
| `produceState` | Convert non-Compose observable → Compose State |

### Compose performance checklist
- [ ] Use `LazyColumn`/`LazyRow` for any list > 10 items
- [ ] Provide stable `key` in `LazyColumn { items(key=) }`
- [ ] Annotate state holder classes with `@Stable` or `@Immutable`
- [ ] Extract lambdas out of frequently recomposed scopes
- [ ] Enable and check **Compose compiler metrics** in CI

---

## SwiftUI Deep Patterns

### View decomposition

```swift
// ❌ Monolithic view — hard to test and reuse
struct FeedView: View {
    var body: some View {
        // 200 lines mixing layout, logic, data fetching
    }
}

// ✅ Decomposed — each piece is independently previewable
struct FeedView: View {
    @State private var vm = FeedViewModel()
    var body: some View {
        FeedList(items: vm.items, onTap: vm.select)
            .overlay { if vm.isLoading { LoadingOverlay() } }
            .task { await vm.loadFeed() }
    }
}

struct FeedList: View {
    let items: [FeedItem]
    let onTap: (FeedItem) -> Void
    var body: some View {
        List(items) { item in
            FeedRow(item: item).onTapGesture { onTap(item) }
        }
    }
}
```

### SwiftUI performance
- Prefer `List` (lazy) over `ForEach` in `ScrollView` for large data sets
- Use `.id()` modifier sparingly — forces full re-render
- `@Observable` (iOS 17+) is more efficient than `ObservableObject` — only invalidates views that read changed properties
- Profile with **Instruments → SwiftUI template** to identify redundant body evaluations

---

## Performance & Memory

### Android

| Issue | Symptom | Fix |
|-------|---------|-----|
| Excessive recomposition | Janky list scrolling | `@Stable`/`@Immutable`, stable keys |
| Memory leak | Crash after rotation | Don't hold Activity/Context in ViewModel |
| Skipped frames | Profiler shows > 16 ms | Move work off Main → `Dispatchers.IO/Default` |
| Large APK | Store install drop-off | R8 + ProGuard, enable App Bundle |
| Slow startup | ANR on launch | Lazy DI init, App Startup library |

### iOS

| Issue | Symptom | Fix |
|-------|---------|-----|
| Retain cycle | Memory grows unbounded | `[weak self]` in closures, `weak var delegate` |
| Main thread block | UI freeze / watchdog kill | Move work to background `Task` / `async` |
| Excessive body calls | Janky scrolling | `@Observable` precision, `.equatable()` |
| Large binary | App Store size warnings | Strip debug symbols in Release, asset catalogs |
| Slow launch | Time-to-interactive > 400 ms | Defer non-critical init, use `@MainActor` lazily |

### KMP shared layer
- Use `Dispatchers.IO` for all I/O in `commonMain`
- SQLDelight queries return `Flow` — observe on the correct dispatcher
- Ktor: configure `HttpTimeout`; add retry with exponential backoff
- Avoid `Dispatchers.Main` in shared code — let platform apps dispatch to UI thread

---

## Debugging Guide

### What to ask first
1. Does it crash, hang, or produce wrong output?
2. Does it happen on one platform or both? (KMP bug or platform bug?)
3. Does it reproduce in a fresh install? (State corruption vs. logic bug)
4. Is it in the Presentation, Domain, or Data layer?

### Crash pattern recognition

| Logcat / Console pattern | Likely cause | Fix |
|--------------------------|--------------|-----|
| `NullPointerException` on ViewModel | Fragment re-attached after process death | Use `SavedStateHandle` |
| `IllegalStateException: Flow collect from wrong context` | Collecting Flow on wrong dispatcher | Use `flowOn(Dispatchers.Main)` or `collectAsStateWithLifecycle` |
| `EXC_BAD_ACCESS` (iOS) | Dangling reference in old memory model | Check pre-1.7.20 KMP deps; update |
| `kotlinx.coroutines.JobCancellationException` | ViewModelScope cancelled mid-operation | Normal on back-press; guard only if unexpected |
| `Thread 1: signal SIGABRT` (iOS) | Force-unwrap of nil optional | Replace `!` with `guard let` or `if let` |
| `ANR InputDispatching` (Android) | Main thread blocked > 5 s | Move I/O to `Dispatchers.IO` |
| Blank screen (no error) | State not emitted to UI | Check `collectAsStateWithLifecycle` vs `collectAsState` lifecycle awareness |

### Memory debugging tools
- **Android**: Android Studio Memory Profiler, LeakCanary
- **iOS**: Instruments → Allocations + Leaks, Xcode Memory Graph
- **KMP**: Kotlin/Native memory model introspection (post-1.7.20 mostly self-managed)

---

## Feature Writing Checklist

- [ ] Architecture layer chosen (MVVM or MVI) and documented in PR
- [ ] Domain models defined in `commonMain` (KMP) or Domain layer (platform-only)
- [ ] Repository interface defined in Domain; implementation in Data
- [ ] ViewModel / `@Observable` created; UiState modelled as immutable data class
- [ ] Loading / Error / Empty / Success states all handled in UI
- [ ] `LaunchedEffect` / `.task {}` used for data loading (not `init` / `onAppear` directly)
- [ ] One-shot events (navigation, snackbars) routed via `Channel` / closure callback — NOT via state
- [ ] All UI text localised (`strings.xml` / `Localizable.strings`)
- [ ] Accessibility labels set on all interactive elements (`contentDescription` / `accessibilityLabel`)
- [ ] Dark mode tested (Material3 dynamic color / SwiftUI `colorScheme`)
- [ ] Unit tests: ViewModel logic tested with `Turbine` (Android) or `XCTest async` (iOS)
- [ ] UI tests: Compose `createComposeRule()` / SwiftUI `ViewInspector` for critical flows
- [ ] Secrets stored in Keychain (iOS) / EncryptedSharedPreferences or Keystore (Android)
- [ ] No `git push` executed — remote push is human's decision
- [ ] Project-appropriate validation passes before declaring task done

---

## MCP Integration

When operating within this project, follow the MCP state protocol in `references/mcp-protocol.md` for: state check, reading slice specs, registering artifacts, and emitting the handoff envelope.
