# Architecture review and restructuring plan

November 2026. The review covered all 35 source files (about 15,000 lines). The guidance comes from the SD knowledge base notebooks ("SD · 1 Architecture Fundamentals", "SD · 3 Testing & Quality", "SD · 5 Implementation").

## Where it stands

**Already good:**
- A lot of logic is written as static functions with no state: `LimitLogic`, `MarkdownParser`, the transcript parsers, `PageLibrary.find`, the base58 decoding. They can be tested as they are.
- `CLAUDE.md` records the hard-won rules (accounts, Keychain, lanes, hooks).
- The design kit (`ChatKit`) depends on almost nothing else.

**What makes it hard to change and test:**

| Problem | Where |
|---|---|
| No module boundaries: one `swiftc` call over everything, so any file can reach any other. | whole app |
| Side effects are called directly, not passed in: `FileManager.default` ×95, `UserDefaults.standard` ×47, plus `Process`, `URLSession` with semaphores, and `/usr/bin/security`. | everywhere |
| God objects | `AppDelegate` (885 lines: wiring, polling, agent-list policy, the whole menu, about 10 alerts, `claude rm`)<br>`ChatModel` (610 lines inside `ChatWindow.swift`, 1,910 lines)<br>`AccountStore` (about 560 lines: persistence, Keychain, HTTP, JSON mapping, switching, lanes)<br>`PetView` (about 870 lines inside `NotchView.swift`) |
| Dependency cycles | `HookBridge` ↔ `PageLibrary` / `OwnerPublish`<br>`AccountStore` ↔ `LaneStore`<br>`AgentMonitor` ↔ `AgentActions`<br>`LaneStore` → `ReplySender` → `AgentMonitor` → `LaneStore`<br>`ChatWindow` ↔ `ChatParts` |
| Adding a provider means editing core code: the `Provider` enum, `switch` statements, window init parameters, `sync` calls, menu sections. | Accounts, AppDelegate |
| Duplicated code | three `Process` runners; two semaphore-based HTTP clients; four copies of JSON-file read-modify-write; ISO date parsing ×4; `readJSON` ×2; `parentPID` ×2; `activeEmail` ×2; the `DesktopPage` and `AntigravityPage` views; the two menu row views |
| Values passed around as strings | about 20 UserDefaults keys across 6 files (the `migrateFromAwake` list must be kept in step by hand); `~/.claude` hard-coded in 5 files; the account id `"uuid:org"` built and split in 5 places; the hook protocol as `[String: Any]`, built in 4 places |
| The hook process links AppKit and SwiftUI, and reaches `AccountStore` through `OwnerPublish`. | `--hook` |

## Target structure

Dependencies point inward: UI → features → core, and the adapters implement protocols that core defines.

```
NotchlingCore        pure Foundation, no AppKit/SwiftUI, no disk/process/network
                     models (Agent, Snapshot, ChatItem, ClaudeAccount, AccountUsage, PageEntry…)
                     parsers (transcripts, conversation, usage JSON, jobs/sessions, front matter)
                     policies (LimitLogic, agent-list reducer, peek rules, notch layout math,
                     pet simulation with an injected clock and random source)
                     hook protocol as Codable HookRequest/HookResponse
                     ports: FileSystem, Settings, ProcessRunner, HTTPClient, Keychain, Clock

NotchlingSystem      adapters for the ports: lane-aware ClaudePaths, typed SettingsStore,
                     SecurityCLIKeychain, ProcessRunner, async HTTPClient, JSONFileEditor,
                     pty attach, AppLifecycle (quit/open/isRunning), file IPC for the hook

Providers            AccountProvider protocol + ClaudeCode, ClaudeDesktop, Antigravity
                     (and later Codex, Gemini), registered at launch

NotchlingUI          DesignKit, SharedUI (EditingWindow, PagePreview, alerts),
                     Notch, Pet, Chat, Pages, Search, Accounts. Views draw; view models hold state

Notchling (app)      composition root: builds the adapters and passes them down
                     --hook path uses Core + System only (no AppKit)
```

The SD guidance this follows:
- **Functional core, imperative shell.** Logic takes values and returns values. Only adapters touch disk, Keychain, processes or the network.
- **Constructor injection with defaults:** `init(fs: FileSystem = LocalFileSystem())`. This adds a seam without touching any caller.
- **Open/closed for providers.** A new provider is a new file that conforms to `AccountProvider` and is registered in one place. Nothing else changes.
- **Humble views.** SwiftUI views hold no I/O. State changes live in `@MainActor` view models that tests can drive.
- **Size targets:** files under about 400 lines, functions under about 40, at most 4 parameters (otherwise pass a parameter struct).
- **Strangler fig.** Each step is small and leaves the app shippable. No rewrite on a separate branch.

## Phases

### Phase 0: safety net (done)
- Moved the flat folder into `Sources/Notchling/<Feature>/`: App, Notch, Agents, Hook, Accounts, Limits, Lanes, Chat, DesignKit, Pages, Search. `build.sh` and `Demo/make-demo.sh` compile that tree.
- Added `Package.swift` and `Tests/NotchlingTests`, using `@testable import Notchling`. The 30 characterization tests cover:
  - `LimitLogic`
  - the usage JSON mapping, including the `limits` list
  - the conversation and transcript parsers (fixtures)
  - limit-hit detection
  - paste collapsing
  - the hook's output for each action
  - page lookup by link, id and words
  - base58 → UUID
  - markdown blocks
- CI runs `swift test` before building.

### Performance work (done)
Measured with `sample` on the installed app:
- **Transcript lookup.** `TranscriptReader.path(for:)` scanned all ~480 project folders every 3 s for each agent whose transcript it couldn't find. It now checks the cwd's project folder first, rescans at most every 30 s, and caches the `/clear` successor until the folder changes. Its cost went from ~550 samples per 15 s to 7.
- **Incremental transcript reading.** Each transcript keeps a cursor: a byte offset plus the parser's state. A refresh parses only the lines appended since the last one. On a 5.6 MB transcript that is 24.8 ms → 0.39 ms per refresh (`ZBenchmarkTests`).
- **One shared reader.** `TranscriptCursors<Parser, Output>` (`Agents/TranscriptCursor.swift`) owns the cursors. The notch (`TranscriptReader.SnapshotParser`) and the Chat window (`ConversationReader.ConversationParser`) both read through it. On a 5.6 MB transcript the Chat window went from ~35 ms to ~0.08 ms per refresh. With Chat open, 30 s samples dropped from 249 to 6 for `ChatModel.refresh`, and from 114 to 27 for the notch's snapshots.
- **Bounded state.** Long sessions drop old items and counted message ids while the cursor is kept, so memory doesn't grow with the session.
- **Cost tracking driven by file events.** `CostTracker` walked all 5,600 transcripts (15,000 entries) every minute and re-encoded its 3.4 MB cache after every scan that found something. That was half the app's CPU when the disk was busy. Now:
  - `System/FileEvents.swift` (FSEvents) tells it which files changed. The full walk runs only at launch, after dropped events, and every 30 minutes.
  - The cache is saved at most every 5 minutes and on quit.
  - Summaries are rebuilt only when something changed or the day rolled over.

  70 s sample: 12.7% → 1.3% of a core, and `CostTracker` 4,389 → 42 samples.

**Still to do:**
- `limitHit`, `sleepCut` and `toolFinished` still read small tails (200–500 KB) on their own schedules. They could become fields of `SnapshotParser`.
- The Antigravity quota check runs `pgrep`, `ps` and `lsof` every time.

**File events for the notch (deferred).** With cursors, polling costs one `stat` per agent per tick, so `FileEvents` there would mostly cut latency (the 3 s wait).
- `AccountStore.sync` (Keychain through `/usr/bin/security`) is now the largest periodic cost, but it is small.

### Phase 1: seams (no behavior change)
1. Add a lane-aware `ClaudePaths`. This removes the 5 hard-coded `~/.claude` paths, and `TranscriptReader`, `SlashCommands` and `desktopSessionID` then see lane folders too.
2. Add a typed `SettingsStore`: one enum of keys, with the migration list built from it. This replaces the 47 calls to `UserDefaults.standard`.
3. Write one `ProcessRunner` and one async `HTTPClient` to replace the three runners and the semaphores. Add a `JSONFileEditor` that keeps file permissions and a backup.
4. Add an `AccountID` value type in place of `"uuid:org"` strings.
5. Make the hook protocol `Codable`, shared by `NotchModel` and `HookBridge`.

### Phase 2: the `NotchlingCore` target
Move the pure models, parsers and policies listed above into a library target with no AppKit import. Make them `public` and move their tests to `NotchlingCoreTests`. This breaks the cycles, because `PageLibrary`, `OwnerPublish` and `HookBridge` then share Core types and no longer depend on each other.

### Phase 3: break up the god objects
| Today | Becomes |
|---|---|
| `AppDelegate` | `AppCoordinator` (polling, wiring)<br>`StatusMenuBuilder`<br>`AgentListReducer` (pure)<br>`Alerts` |
| `ChatModel` | `ChatViewModel` (selection, drafts, send)<br>`SessionAccountsCoordinator` (move, switch, new session)<br>`DropIntake`<br>`ClaudeCLI` adapter |
| `AccountStore` | `AccountRepository`<br>`UsageAPI`<br>`ClaudeCodeSwitcher`<br>`LaneSwitcher` (ends the Lanes cycle) |
| `ConversationReader.parse` (172 lines) | per-type handlers over a `ParseState` struct, plus a `ToolCallFactory` |
| `NotchView.swift` | one file per card; `PetView` goes to a Pet folder with an injected clock and random source |
| `LimitWatcher` | one switch flow for the main login and for lanes; the notifier and banner move to UI |

### Phase 4: provider registry
```swift
protocol AccountProvider: AnyObject {
    var id: String { get }
    var title: String { get }
    var capabilities: ProviderCapabilities { get }   // add flow, needs restart, usage, auto-switch, cost
    var state: ProviderState { get }                  // accounts, activeID, busy
    func sync(force: Bool)
    func switchTo(_ id: AccountID) async throws
    func beginAdd() throws
    func cancelAdd()
    func remove(_ id: AccountID)
}
```
- Usage becomes a generic `UsageReport`: groups of bars, each bar with a kind (`session`, `week`, `other`). The cards and `LimitLogic` read that.
- The Accounts window, the menu sections and the limit watcher iterate the registry.
- Desktop and Antigravity share one `RestartingAppProvider` base class: quit, open, watch for a new login, cancel.

### Phase 5: a hook with no AppKit
Once Core and System are separate targets, the `--hook` path links only those two, so the hook starts faster and pulls in less.

## Review findings, checked
The review's real bugs are fixed. These turned out not to be bugs:
- Usage freshness `15*60` (in `LimitWatcher`, not Accounts) against `LimitLogic.freshFor` (10 minutes): they gate different things. `freshFor` is for an account Notchling would switch to, `inUseFreshFor` for the account already in use, which is refreshed every 5 minutes and skipped while its token is expired. The two literals are now one named constant.
- `ConversationReader.cache` having no size limit: its `TranscriptCursors` was already capped at 60 files, though by clearing them all. It now drops only the file read longest ago.
- `AccountStore.snapshot()` and `DispatchQueue.main.sync`: every caller (and the Codex and Antigravity `capture()`s) is already inside its own serial `queue.async`, so none can run on the main thread.
- `NotchController` observers never removed: it is created once by `AppDelegate` and lives as long as the process, and the closure observers hold `self` weakly.

## Rules for new code
- No `FileManager.default`, `UserDefaults.standard`, `Process` or `URLSession` outside the adapters. Take the dependency in `init` instead.
- Every new parser or policy ships with a test. Parsers of Claude Code's formats get a fixture in `Tests/NotchlingTests/Fixtures`.
- A new account provider must not need an edit outside its own file and its registration.
- Keep files under about 400 lines. Split by feature, not by layer.
