import SwiftUI

/// The Codex CLI's ChatGPT accounts. Adding runs `codex login` in a scratch home and leaves the live
/// login alone, so the menu sends Add Account… to the Accounts window, where its sign-in link shows.
final class CodexProvider: AccountProvider {
    let store: CodexStore

    init(store: CodexStore) { self.store = store }

    let id = ProviderID.codex
    let title = "Codex"
    let capabilities: ProviderCapabilities = [.addAccount, .usage, .autoSwitch]
    let menuStyle = ProviderMenuStyle(notSignedIn: "Not signed in to Codex", showsLimits: true, planFallback: "ChatGPT",
                                      limitsPlaceholder: "Reading limits…", opens: .codex)
    let switchFailureTitle = "Couldn't switch Codex accounts"

    var isAvailable: Bool { store.isAvailable }

    var changed: (() -> Void)? {
        get { store.changed }
        set { store.changed = newValue }
    }

    var state: ProviderState {
        ProviderState(accounts: store.accounts.map { ProviderAccount(id: $0.id, email: $0.email, plan: $0.plan) },
                      activeID: store.activeID, busy: store.busy, adding: store.adding)
    }

    func sync(force: Bool) { store.sync(force: force) }

    func usageReport(for accountID: String) -> UsageReport? { store.usage[accountID].map(UsageReport.init) }

    func switchTo(_ id: String) async throws { try await awaitStore { store.switchTo(id, done: $0) } }

    func beginAdd(done: @escaping (Error?) -> Void) { store.beginAdd(done: done) }

    func cancelAdd() { store.cancelAdd() }

    func remove(_ id: String) { store.remove(id) }

    func switchConfirmation(to id: String) -> ProviderConfirmation? {
        guard let warning = CodexStore.switchWarning(CodexStore.runningWork()) else { return nil }
        return ProviderConfirmation(title: "Switch Codex Account?", message: warning, button: "Switch")
    }

    func makePage(cost: CostTracker) -> AnyView { AnyView(CodexPage(store: store)) }
}
