import SwiftUI

/// Stable name of a provider: the Accounts window's tab, the menu section, and what other code asks for.
struct ProviderID: RawRepresentable, Hashable, ExpressibleByStringLiteral {
    var rawValue: String
    init(rawValue: String) { self.rawValue = rawValue }
    init(stringLiteral value: String) { rawValue = value }

    static let claudeCode: ProviderID = "claudeCode"
    static let claudeDesktop: ProviderID = "claudeDesktop"
    static let codex: ProviderID = "codex"
    static let antigravity: ProviderID = "antigravity"
}

struct ProviderCapabilities: OptionSet {
    let rawValue: Int
    /// Accounts can be added from Notchling.
    static let addAccount = ProviderCapabilities(rawValue: 1 << 0)
    /// The menu's Add Account… starts the flow itself; without it, the item opens the Accounts window.
    static let addFromMenu = ProviderCapabilities(rawValue: 1 << 1)
    /// A switch quits and reopens an app.
    static let restartsApp = ProviderCapabilities(rawValue: 1 << 2)
    /// Reports limits (`usageReport`).
    static let usage = ProviderCapabilities(rawValue: 1 << 3)
    /// The limit watcher may switch it when a limit is hit.
    static let autoSwitch = ProviderCapabilities(rawValue: 1 << 4)
    /// Token cost is tracked for it.
    static let cost = ProviderCapabilities(rawValue: 1 << 5)
}

struct ProviderAccount: Equatable, Identifiable {
    var id: String
    var email: String
    /// "Pro", "Max 5x"…; nil when the provider has none to show.
    var plan: String?
    /// The saved login was refused; the user has to sign in again.
    var needsLogin = false
    /// There is something saved to switch to (Desktop lists accounts it can't switch to yet).
    var switchable = true
}

struct ProviderState: Equatable {
    var accounts: [ProviderAccount] = []
    var activeID: String?
    /// Switching or adding is under way.
    var busy = false
    /// An add flow is waiting for the user (a browser sign-in, an app at its sign-in screen).
    var adding = false
    /// Names the active account when it isn't among `accounts` (Desktop on an account Notchling has no
    /// Claude Code login for).
    var unlistedActiveTitle: String?

    var active: ProviderAccount? { accounts.first { $0.id == activeID } }
    /// The active account's name for a menu row; nil when signed out.
    var activeName: String? { active?.email ?? (activeID == nil ? nil : unlistedActiveTitle) }
}

/// A question to answer before something that interrupts work.
struct ProviderConfirmation: Equatable {
    var title: String
    var message: String
    var button: String
}

/// How a provider looks in the menu bar. The Claude Code section is its own (cost, lanes, limit mode).
struct ProviderMenuStyle {
    var notSignedIn: String
    /// Shows the active account as a row with limit bars; otherwise as a plain checked item.
    var showsLimits: Bool
    var planFallback = ""
    var limitsPlaceholder = ""
    /// Reads the live login each time the menu opens (cheap for the ones that have no network call).
    var syncsOnOpen = false
    /// While an add flow waits, the section says so and offers Cancel (an app sits at its sign-in screen).
    /// Without it the section stays as it was (Codex signs in in the browser).
    var waitsInMenu = false
    /// The provider whose Accounts tab the active row and Add Account… open.
    var opens: ProviderID?
}

/// What a menu item stands for: a provider, and one of its accounts.
struct ProviderMenuTarget {
    var provider: ProviderID
    var account: String?
}

/// One source of accounts the user can switch between. Adding one means one file
/// that conforms to this and one `ProviderRegistry.register` call at launch; the Accounts window tabs
/// and the menu sections come from the registry.
///
/// The conformances wrap the existing stores and add no logic of their own: the stores keep the
/// account-switching code, which touches the user's real logins.
protocol AccountProvider: AnyObject {
    var id: ProviderID { get }
    /// The tab and menu header.
    var title: String { get }
    var capabilities: ProviderCapabilities { get }
    /// Installed or used before; otherwise the menu leaves it out.
    var isAvailable: Bool { get }
    /// Read when drawing: the stores publish the change through `changed`.
    var state: ProviderState { get }
    var menuStyle: ProviderMenuStyle { get }
    /// Called on the main thread after anything in `state` or the limits changes.
    var changed: (() -> Void)? { get set }

    /// Reads the live login and refreshes limits. Providers that have no cheaper read ignore `force`.
    func sync(force: Bool)
    func usageReport(for accountID: String) -> UsageReport?
    func switchTo(_ id: String) async throws
    /// Starts the add flow. `done` runs when the flow ends or fails to start; progress shows in `state.adding`.
    func beginAdd(done: @escaping (Error?) -> Void)
    func cancelAdd()
    func remove(_ id: String)

    /// Asked before `switchTo` / `beginAdd`: nil when nothing would be interrupted.
    func switchConfirmation(to id: String) -> ProviderConfirmation?
    func addConfirmation() -> ProviderConfirmation?
    var switchFailureTitle: String { get }

    /// The provider's page in the Accounts window, with its own cards and add panel.
    func makePage(cost: CostTracker) -> AnyView
    /// The Accounts window closed.
    func accountsWindowClosed()
}

extension AccountProvider {
    var isAvailable: Bool { true }
    func usageReport(for accountID: String) -> UsageReport? { nil }
    func switchConfirmation(to id: String) -> ProviderConfirmation? { nil }
    func addConfirmation() -> ProviderConfirmation? { nil }
    func accountsWindowClosed() {}
}

/// Wraps a store's callback-style call as `async throws`. The store calls `done` on the main thread.
func awaitStore(_ call: (@escaping (Error?) -> Void) -> Void) async throws {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        call { error in
            if let error { continuation.resume(throwing: error) } else { continuation.resume() }
        }
    }
}

/// The providers, in the order the Accounts window and the menu show them.
final class ProviderRegistry {
    private(set) var providers: [any AccountProvider] = []

    /// A second provider with the same id is ignored.
    func register(_ provider: any AccountProvider) {
        guard self[provider.id] == nil else { return }
        providers.append(provider)
    }

    subscript(id: ProviderID) -> (any AccountProvider)? { providers.first { $0.id == id } }

    func syncAll(force: Bool = false) {
        for provider in providers { provider.sync(force: force) }
    }

    func accountsWindowClosed() {
        for provider in providers { provider.accountsWindowClosed() }
    }
}
