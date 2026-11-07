import SwiftUI

/// The Claude desktop app. It lists the Claude Code accounts (matched by account UUID) that it has a
/// session for, since a Desktop session can only exist after signing in to it there once.
final class ClaudeDesktopProvider: AccountProvider {
    let store: ClaudeDesktopStore
    private let claude: AccountStore

    init(store: ClaudeDesktopStore, claude: AccountStore) {
        self.store = store
        self.claude = claude
    }

    let id = ProviderID.claudeDesktop
    let title = "Claude Desktop"
    let capabilities: ProviderCapabilities = [.addAccount, .addFromMenu, .restartsApp]
    // Its active row opens the Claude Code tab, as it always has.
    let menuStyle = ProviderMenuStyle(notSignedIn: "Not signed in to Claude Desktop", showsLimits: false, syncsOnOpen: true, waitsInMenu: true, opens: .claudeCode)
    let switchFailureTitle = "Couldn't switch Claude Desktop"

    var isAvailable: Bool { store.isAvailable }

    var changed: (() -> Void)? {
        get { store.changed }
        set { store.changed = newValue }
    }

    private static func uuid(_ accountID: String) -> String { String(accountID.prefix { $0 != ":" }) }

    var state: ProviderState {
        // Only accounts actually in Desktop: the active one, or ones Notchling has captured.
        let accounts = claude.accounts.compactMap { account -> ProviderAccount? in
            let uuid = Self.uuid(account.id)
            let active = store.activeUuid == uuid
            guard active || store.saved.contains(uuid) else { return nil }
            return ProviderAccount(id: uuid, email: account.email, plan: nil, switchable: store.saved.contains(uuid))
        }
        return ProviderState(accounts: accounts, activeID: store.activeUuid, busy: store.busy, adding: store.adding,
                             unlistedActiveTitle: "Signed in")
    }

    func sync(force: Bool) { store.sync() }

    func switchTo(_ id: String) async throws { try await awaitStore { store.switchTo(id, done: $0) } }

    func beginAdd(done: @escaping (Error?) -> Void) { store.beginAdd(done: done) }

    func cancelAdd() { store.cancelAdd() }

    func remove(_ id: String) { store.forget(id) }

    func switchConfirmation(to id: String) -> ProviderConfirmation? {
        guard ClaudeDesktopStore.isRunning else { return nil }
        return ProviderConfirmation(title: "Restart Claude Desktop?",
                                    message: "Claude Desktop quits and opens again on the other account.", button: "Restart")
    }

    func addConfirmation() -> ProviderConfirmation? {
        guard ClaudeDesktopStore.isRunning else { return nil }
        return ProviderConfirmation(
            title: "Open Claude Desktop signed out?",
            message: "Claude Desktop quits and reopens at the sign-in screen. Sign in to the account "
                + "you want to add; Notchling saves it. Your current account is kept and you can switch back to it.",
            button: "Continue")
    }

    func makePage(cost: CostTracker) -> AnyView { AnyView(DesktopPage(accounts: claude, desktop: store)) }
}
