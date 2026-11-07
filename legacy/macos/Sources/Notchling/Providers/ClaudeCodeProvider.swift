import SwiftUI

/// Claude Code's accounts: the saved logins `AccountStore` swaps into the Keychain. Adding is the
/// browser sign-in `SignInModel` drives, so `beginAdd` starts it and its progress shows in the page.
final class ClaudeCodeProvider: AccountProvider {
    let store: AccountStore
    let signIn: SignInModel

    init(store: AccountStore) {
        self.store = store
        signIn = SignInModel(store: store)
    }

    let id = ProviderID.claudeCode
    let title = "Claude Code"
    let capabilities: ProviderCapabilities = [.addAccount, .usage, .autoSwitch, .cost]
    let menuStyle = ProviderMenuStyle(notSignedIn: "Not signed in to Claude Code", showsLimits: true)
    let switchFailureTitle = "Couldn't switch accounts"

    var changed: (() -> Void)? {
        get { store.changed }
        set { store.changed = newValue }
    }

    var state: ProviderState {
        var adding = false
        switch signIn.phase {
        case .waiting, .saving: adding = true
        default: break
        }
        return ProviderState(
            accounts: store.accounts.map { ProviderAccount(id: $0.id, email: $0.email, plan: $0.plan, needsLogin: $0.needsLogin) },
            activeID: store.activeID, busy: store.busy, adding: adding)
    }

    func sync(force: Bool) { store.sync(force: force) }

    func usageReport(for accountID: String) -> UsageReport? { store.usage[accountID].map(UsageReport.init) }

    func switchTo(_ id: String) async throws { try await awaitStore { store.switchTo(id, done: $0) } }

    func beginAdd(done: @escaping (Error?) -> Void) {
        signIn.start()
        done(nil)
    }

    func cancelAdd() { signIn.cancel() }

    func remove(_ id: String) { store.remove(id) }

    func accountsWindowClosed() { signIn.cancel() }

    func makePage(cost: CostTracker) -> AnyView {
        AnyView(AccountsPage(accounts: store, cost: cost, signIn: signIn))
    }
}
