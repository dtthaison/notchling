import SwiftUI

/// Antigravity and Antigravity IDE, which share one Google login. A switch or an add quits and
/// reopens them.
final class AntigravityProvider: AccountProvider {
    let store: AntigravityStore

    init(store: AntigravityStore) { self.store = store }

    let id = ProviderID.antigravity
    let title = "Antigravity"
    let capabilities: ProviderCapabilities = [.addAccount, .restartsApp, .usage]
    let menuStyle = ProviderMenuStyle(notSignedIn: "Not signed in to Antigravity", showsLimits: true, planFallback: "Google",
                                      limitsPlaceholder: "Open Antigravity to read its limits.", syncsOnOpen: true, waitsInMenu: true, opens: .antigravity)
    let switchFailureTitle = "Couldn't switch Antigravity accounts"

    var isAvailable: Bool { store.isAvailable }

    var changed: (() -> Void)? {
        get { store.changed }
        set { store.changed = newValue }
    }

    var state: ProviderState {
        ProviderState(accounts: store.accounts.map { ProviderAccount(id: $0.id, email: $0.email, plan: $0.plan) },
                      activeID: store.activeID, busy: store.busy, adding: store.adding)
    }

    func sync(force: Bool) { store.sync() }

    func usageReport(for accountID: String) -> UsageReport? { store.usage[accountID].map(UsageReport.init) }

    func switchTo(_ id: String) async throws { try await awaitStore { store.switchTo(id, done: $0) } }

    func beginAdd(done: @escaping (Error?) -> Void) { store.beginAdd(done: done) }

    func cancelAdd() { store.cancelAdd() }

    func remove(_ id: String) { store.remove(id) }

    func switchConfirmation(to id: String) -> ProviderConfirmation? {
        guard AntigravityStore.isRunning else { return nil }
        return ProviderConfirmation(title: "Restart Antigravity?",
                                    message: "Antigravity quits and opens again on the other account. Agents running in it stop.",
                                    button: "Restart")
    }

    func makePage(cost: CostTracker) -> AnyView { AnyView(AntigravityPage(store: store)) }
}
