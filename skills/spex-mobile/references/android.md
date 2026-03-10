# Android Deep Reference

## Jetpack Compose

### Key rules
- Composables are **pure functions of state** — no side effects in the body
- Side effects go in `LaunchedEffect`, `SideEffect`, or `DisposableEffect`
- Use `remember` for local state, `rememberSaveable` to survive recomposition + rotation
- Hoisting: lift state up to the lowest common ancestor that needs it
- `@Stable` / `@Immutable` annotations help Compose skip unnecessary recompositions

### Performance
- Avoid creating lambdas inline in frequently-recomposed composables → extract or `remember { }`
- Use `LazyColumn` / `LazyRow` instead of `Column` + `forEach` for lists
- Use `key {}` in lazy lists when items can reorder
- Profile with **Compose compiler metrics** (`freeCompilerArgs += ["-P", "plugin:androidx.compose.compiler.plugins.kotlin:reportsDestination=..."]`)

### Common Composable patterns
```kotlin
// Stateless composable (easy to test, preview, reuse)
@Composable
fun UserCard(user: User, onTap: () -> Unit, modifier: Modifier = Modifier) { ... }

// State hoisting
@Composable
fun SearchBar(query: String, onQueryChange: (String) -> Unit) { ... }

// LaunchedEffect — run suspend code tied to a key
LaunchedEffect(userId) {
    viewModel.loadUser(userId)
}

// DisposableEffect — cleanup on leave
DisposableEffect(Unit) {
    val listener = ...
    onDispose { listener.unregister() }
}
```

---

## ViewModel + Coroutines

```kotlin
@HiltViewModel
class DetailViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val repo: DetailRepository
) : ViewModel() {

    // Prefer StateFlow over LiveData for new code
    private val _state = MutableStateFlow(DetailUiState())
    val state = _state.asStateFlow()

    // One-shot events (navigation, snackbars)
    private val _events = Channel<DetailEvent>()
    val events = _events.receiveAsFlow()

    init {
        val id = savedStateHandle.get<String>("id") ?: return
        loadDetail(id)
    }

    private fun loadDetail(id: String) = viewModelScope.launch {
        _state.update { it.copy(isLoading = true) }
        repo.getDetail(id)
            .onSuccess { detail -> _state.update { it.copy(detail = detail, isLoading = false) } }
            .onFailure { e -> _state.update { it.copy(error = e.message, isLoading = false) } }
    }
}
```

### Coroutine dispatcher rules
- `Dispatchers.Main` — UI updates only
- `Dispatchers.IO` — network, disk
- `Dispatchers.Default` — CPU-heavy work
- Never use `GlobalScope` in production — it leaks and ignores lifecycle

---

## Gradle (Kotlin DSL)

```kotlin
// build.gradle.kts (app)
android {
    compileSdk = 35
    defaultConfig {
        minSdk = 26
        targetSdk = 35
    }
    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.x" }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.xx.xx"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose")
    implementation("androidx.hilt:hilt-navigation-compose:1.x.x")
}
```

---

## Navigation (Compose)

```kotlin
// Type-safe routes (Navigation 2.8+)
@Serializable object HomeRoute
@Serializable data class DetailRoute(val id: String)

NavHost(navController, startDestination = HomeRoute) {
    composable<HomeRoute> { HomeScreen(onItemClick = { id -> navController.navigate(DetailRoute(id)) }) }
    composable<DetailRoute> { backStackEntry ->
        val route: DetailRoute = backStackEntry.toRoute()
        DetailScreen(id = route.id)
    }
}
```

---

## Room (Local Database)

```kotlin
@Entity
data class UserEntity(@PrimaryKey val id: String, val name: String)

@Dao
interface UserDao {
    @Query("SELECT * FROM userentity WHERE id = :id")
    fun observeUser(id: String): Flow<UserEntity?>   // Flow = reactive updates

    @Upsert suspend fun upsert(user: UserEntity)
    @Delete suspend fun delete(user: UserEntity)
}

// Always expose Flow from DAO, not suspend fun for queries
// Use @Transaction for multi-table reads
```

---

## Security
- Sensitive data: use **EncryptedSharedPreferences** or Android Keystore
- API keys: never in source — use `local.properties` + `BuildConfig`
- Network: enforce TLS; use certificate pinning for high-security apps
- Biometric: `BiometricPrompt` API — don't roll your own

---

## Material3 Theming

```kotlin
// Theme setup
@Composable
fun AppTheme(darkTheme: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val colorScheme = when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> darkColorScheme()
        else -> lightColorScheme()
    }
    MaterialTheme(colorScheme = colorScheme, typography = AppTypography, content = content)
}

// Use MaterialTheme tokens everywhere — never hardcode colors
Text("Hello", color = MaterialTheme.colorScheme.primary)
```

---

## Hilt DI — full setup

```kotlin
// 1. App-level
@HiltAndroidApp
class MyApp : Application()

// 2. Module
@Module
@InstallIn(SingletonComponent::class)
object NetworkModule {
    @Provides @Singleton
    fun provideHttpClient(): HttpClient = HttpClient(OkHttp) { /* config */ }

    @Provides @Singleton
    fun provideUserApi(client: HttpClient): UserApi = UserApiImpl(client)
}

// 3. Repository binding
@Module
@InstallIn(SingletonComponent::class)
abstract class RepositoryModule {
    @Binds @Singleton
    abstract fun bindUserRepo(impl: UserRepositoryImpl): UserRepository
}

// 4. Scopes
// @Singleton — app lifetime
// @ActivityRetainedScoped — ViewModel lifetime
// @ViewModelScoped — single ViewModel instance
// @ActivityScoped — Activity lifetime

// 5. ViewModel injection (no factory needed)
@HiltViewModel
class MyViewModel @Inject constructor(private val repo: UserRepository) : ViewModel()

// 6. Activity / Fragment
@AndroidEntryPoint
class MainActivity : ComponentActivity() { ... }
```

---

## Compose Animations

```kotlin
// AnimatedVisibility
AnimatedVisibility(
    visible = showBanner,
    enter = fadeIn() + slideInVertically(),
    exit = fadeOut() + slideOutVertically()
) {
    BannerView()
}

// animate*AsState — for simple value animations
val elevation by animateDpAsState(if (scrolled) 8.dp else 0.dp, label = "elevation")
val alpha by animateFloatAsState(if (enabled) 1f else 0.4f, label = "alpha")

// Crossfade — swap content with a fade
Crossfade(targetState = currentScreen, label = "screen") { screen ->
    when (screen) {
        Screen.Home -> HomeContent()
        Screen.Profile -> ProfileContent()
    }
}

// updateTransition — coordinate multiple animations
val transition = updateTransition(isExpanded, label = "expand")
val height by transition.animateDp(label = "height") { if (it) 200.dp else 60.dp }
val alpha by transition.animateFloat(label = "alpha") { if (it) 1f else 0f }
```

---

## WorkManager (background tasks)

```kotlin
// 1. Define worker
@HiltWorker
class SyncWorker @AssistedInject constructor(
    @Assisted context: Context,
    @Assisted params: WorkerParameters,
    private val repo: SyncRepository
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        return try {
            repo.sync()
            Result.success()
        } catch (e: Exception) {
            if (runAttemptCount < 3) Result.retry() else Result.failure()
        }
    }
}

// 2. Schedule
val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
    .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
    .build()

WorkManager.getInstance(context).enqueueUniquePeriodicWork(
    "sync", ExistingPeriodicWorkPolicy.KEEP, request
)
```

---

## Navigation Deep Linking

```kotlin
// AndroidManifest.xml
// <intent-filter android:autoVerify="true">
//   <action android:name="android.intent.action.VIEW"/>
//   <category android:name="android.intent.category.DEFAULT"/>
//   <category android:name="android.intent.category.BROWSABLE"/>
//   <data android:scheme="https" android:host="example.com" android:pathPrefix="/user"/>
// </intent-filter>

// NavHost setup
composable<DetailRoute>(
    deepLinks = listOf(navDeepLink<DetailRoute>(basePath = "https://example.com/user"))
) { backStack ->
    DetailScreen(route = backStack.toRoute())
}
```

---

## Testing

```kotlin
// Compose UI test
class ProfileScreenTest {
    @get:Rule val composeRule = createComposeRule()

    @Test fun showsLoadingThenProfile() {
        val fakeVm = FakeProfileViewModel()
        composeRule.setContent { ProfileScreen(viewModel = fakeVm) }
        composeRule.onNodeWithTag("loading_indicator").assertIsDisplayed()
        fakeVm.emitProfile(testProfile)
        composeRule.onNodeWithText(testProfile.name).assertIsDisplayed()
    }
}

// ViewModel unit test with Turbine
@Test fun loadProfile_updatesState() = runTest {
    val vm = ProfileViewModel(FakeProfileRepo())
    vm.state.test {
        assertEquals(ProfileUiState(), awaitItem())  // initial
        vm.load("user-1")
        assertEquals(true, awaitItem().isLoading)
        val success = awaitItem()
        assertEquals("Alice", success.profile?.name)
    }
}

// MockK for dependencies
val mockRepo = mockk<ProfileRepository>()
coEvery { mockRepo.getProfile("user-1") } returns Result.success(testProfile)
```
