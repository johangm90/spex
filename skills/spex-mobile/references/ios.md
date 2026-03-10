# iOS Deep Reference

## SwiftUI

### Key rules
- Views are **value types** — structs, not classes
- State drives UI; never mutate view state directly from outside
- Use `@State` for local, `@Binding` for passed-down, `@StateObject`/`@Observable` for ViewModels
- Prefer `@Observable` (Swift 5.9 / iOS 17+) over `ObservableObject` for new code
- `.task {}` modifier for async work tied to view lifecycle (auto-cancels on disappear)

### State ownership hierarchy
```
@State (owns)  →  @Binding (borrows)
@StateObject (owns)  →  @ObservedObject (borrows)
@Observable + @State (iOS 17+, owns)
@EnvironmentObject / @Environment  (ambient/injected)
```

### Common patterns
```swift
// MVVM with @Observable (iOS 17+)
@Observable
class ProfileViewModel {
    var profile: Profile?
    var error: String?
    var isLoading = false

    func load(id: String) async {
        isLoading = true
        defer { isLoading = false }
        do {
            profile = try await profileService.fetch(id: id)
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct ProfileView: View {
    @State private var vm = ProfileViewModel()
    let userId: String

    var body: some View {
        Group {
            if vm.isLoading { ProgressView() }
            else if let profile = vm.profile { ProfileDetail(profile: profile) }
            else if let error = vm.error { Text(error).foregroundStyle(.red) }
        }
        .task { await vm.load(id: userId) }
    }
}
```

### Performance
- Use `List` (lazy) instead of `ForEach` inside `ScrollView` for large datasets
- Avoid expensive computations in `var body` — move to ViewModel or use `let`
- Use `.equatable()` modifier to skip unnecessary re-renders
- Profile with **Instruments → SwiftUI** template

---

## Swift Concurrency (async/await)

```swift
// Basic async function
func fetchUser(id: String) async throws -> User {
    let (data, _) = try await URLSession.shared.data(from: url)
    return try JSONDecoder().decode(User.self, from: data)
}

// @MainActor — ensures code runs on main thread
@MainActor
class MyViewModel: ObservableObject {
    @Published var items: [Item] = []

    func loadItems() async {
        items = try await repo.fetchItems()  // safe — @MainActor
    }
}

// Parallel execution
async let user = fetchUser(id: id)
async let posts = fetchPosts(userId: id)
let (u, p) = try await (user, posts)

// Task lifecycle in SwiftUI — prefer .task over Task {}
.task {
    await viewModel.load()  // auto-cancelled when view disappears
}

// Actors for shared mutable state
actor Cache {
    private var storage: [String: Data] = [:]
    func get(_ key: String) -> Data? { storage[key] }
    func set(_ key: String, value: Data) { storage[key] = value }
}
```

### Rules
- Mark all UI updates `@MainActor`
- Prefer `async/await` over `DispatchQueue` for new code
- Use `Task.detached` sparingly — it doesn't inherit actor context
- `async let` for parallel work; `TaskGroup` for dynamic parallelism

---

## Navigation (NavigationStack, iOS 16+)

```swift
// Type-safe navigation with NavigationStack
enum Route: Hashable {
    case detail(id: String)
    case settings
}

struct RootView: View {
    @State private var path: [Route] = []

    var body: some View {
        NavigationStack(path: $path) {
            HomeView(onItemTap: { id in path.append(.detail(id: id)) })
                .navigationDestination(for: Route.self) { route in
                    switch route {
                    case .detail(let id): DetailView(id: id)
                    case .settings: SettingsView()
                    }
                }
        }
    }
}
```

---

## Data Persistence

```swift
// SwiftData (iOS 17+) — preferred for new projects
@Model class Task {
    var title: String
    var isDone: Bool = false
    init(title: String) { self.title = title }
}

// In SwiftUI
@Query private var tasks: [Task]  // live, reactive query

// UserDefaults (simple key-value)
@AppStorage("hasOnboarded") private var hasOnboarded = false

// Keychain — use for sensitive data (tokens, passwords), never UserDefaults
import Security  // or use a wrapper like KeychainAccess
```

---

## Security
- **Never** store tokens in `UserDefaults` — use Keychain
- App Transport Security (ATS): don't disable `NSAllowsArbitraryLoads` in production
- Certificate pinning: `URLSession` delegate `urlSession(_:didReceive:completionHandler:)`
- Biometrics: `LAContext.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, ...)`
- Sensitive UI: set `view.textContentType` appropriately; use `.privacySensitive()` in SwiftUI

---

## @Environment and Dependency Injection

```swift
// Custom environment key for DI
struct ProfileRepoKey: EnvironmentKey {
    static let defaultValue: ProfileRepository = LiveProfileRepository()
}

extension EnvironmentValues {
    var profileRepo: ProfileRepository {
        get { self[ProfileRepoKey.self] }
        set { self[ProfileRepoKey.self] = newValue }
    }
}

// Inject at root
ContentView()
    .environment(\.profileRepo, MockProfileRepository())  // swap for testing

// Consume in view or ViewModel
struct ProfileView: View {
    @Environment(\.profileRepo) private var repo
}
```

---

## Combine (iOS 13/14 compatibility)

```swift
// Publisher chain
cancellables.store(in: &bag)
NotificationCenter.default.publisher(for: UIApplication.didBecomeActiveNotification)
    .sink { [weak self] _ in self?.refresh() }
    .store(in: &cancellables)

// ObservableObject + @Published (pre-iOS 17)
class LegacyViewModel: ObservableObject {
    @Published var items: [Item] = []
    private var cancellables = Set<AnyCancellable>()

    func load() {
        repo.itemsPublisher()
            .receive(on: DispatchQueue.main)
            .sink(
                receiveCompletion: { _ in },
                receiveValue: { [weak self] in self?.items = $0 }
            )
            .store(in: &cancellables)
    }
}
```

---

## Sheet / fullScreenCover / popover

```swift
struct ContentView: View {
    @State private var showSheet = false
    @State private var showFullScreen = false
    @State private var showPopover = false

    var body: some View {
        VStack {
            Button("Sheet") { showSheet = true }
                .sheet(isPresented: $showSheet) { SheetContent() }

            Button("Full Screen") { showFullScreen = true }
                .fullScreenCover(isPresented: $showFullScreen) { FullScreenContent() }

            Button("Popover") { showPopover = true }
                .popover(isPresented: $showPopover) { PopoverContent() }
        }
    }
}

// Dismissal from within presented view
struct SheetContent: View {
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        Button("Done") { dismiss() }
    }
}
```

---

## Core Data (pre-iOS 17)

```swift
// Stack setup
class CoreDataStack {
    static let shared = CoreDataStack()
    lazy var container: NSPersistentContainer = {
        let c = NSPersistentContainer(name: "AppModel")
        c.loadPersistentStores { _, error in
            if let error { fatalError("Core Data failed: \(error)") }
        }
        c.viewContext.automaticallyMergesChangesFromParent = true
        return c
    }()
}

// Fetch in SwiftUI
struct TaskListView: View {
    @FetchRequest(sortDescriptors: [SortDescriptor(\.createdAt, order: .reverse)])
    private var tasks: FetchedResults<TaskEntity>

    var body: some View {
        List(tasks) { task in Text(task.title ?? "") }
    }
}
```

---

## SwiftData Relationships and Queries (iOS 17+)

```swift
@Model class Project {
    var name: String
    @Relationship(deleteRule: .cascade) var tasks: [ProjectTask] = []
    init(name: String) { self.name = name }
}

@Model class ProjectTask {
    var title: String
    var isDone: Bool = false
    var project: Project?
    init(title: String) { self.title = title }
}

// Filtered query with predicate
@Query(filter: #Predicate<ProjectTask> { !$0.isDone }, sort: \.title)
private var openTasks: [ProjectTask]
```

---

## Testing (XCTest + async)

```swift
// Async test
final class ProfileViewModelTests: XCTestCase {
    func testLoadSetsProfile() async throws {
        let vm = ProfileViewModel(repo: MockProfileRepository())
        await vm.load(userId: "user-1")
        XCTAssertEqual(vm.profile?.name, "Alice")
        XCTAssertFalse(vm.isLoading)
    }

    // MainActor test
    @MainActor
    func testErrorStateOnFailure() async {
        let vm = ProfileViewModel(repo: FailingProfileRepository())
        await vm.load(userId: "bad-id")
        XCTAssertNotNil(vm.error)
    }
}

// Combine publisher test
func testPublisherEmitsItems() {
    let expectation = expectation(description: "items emitted")
    var received: [Item] = []
    vm.itemsPublisher
        .sink { received = $0; expectation.fulfill() }
        .store(in: &cancellables)
    vm.load()
    wait(for: [expectation], timeout: 2.0)
    XCTAssertFalse(received.isEmpty)
}
```

---

## Instruments Profiling
- **Time Profiler**: Find CPU hotspots; look for `body` calls in SwiftUI call stack
- **Allocations**: Track object lifetimes; watch for unbounded growth
- **Leaks**: Detects reference cycles; run after navigating through all flows
- **SwiftUI template**: Shows view identity, body evaluation count, and layout time
- Workflow: Product → Profile (⌘I) → choose template → record 30 s of typical use → sort by "Self" time
