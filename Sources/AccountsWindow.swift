import AppKit
import SwiftUI

/// The Claude Accounts window: every saved account with its limits, switching, signing in to another
/// account, and token cost. Everything `claude auth login`, /status and /usage would be needed for.
final class AccountsWindowController: NSObject, NSWindowDelegate {
    private let accounts: AccountStore
    private let cost: CostTracker
    private let signIn: SignInModel
    private let tab = TabModel()
    private var window: NSWindow?

    init(accounts: AccountStore, cost: CostTracker) {
        self.accounts = accounts
        self.cost = cost
        signIn = SignInModel(store: accounts)
    }

    func show(_ page: AccountsTab = .accounts, signIn startSignIn: Bool = false) {
        tab.page = page
        if window == nil {
            let view = AccountsView(accounts: accounts, cost: cost, signIn: signIn, tab: tab)
            let window = NSWindow(contentViewController: NSHostingController(rootView: view))
            window.title = "Claude Accounts"
            window.styleMask = [.titled, .closable, .miniaturizable]
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.center()
            self.window = window
        }
        accounts.sync()
        cost.refresh()
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
        if startSignIn { signIn.start() }
    }

    func windowWillClose(_ notification: Notification) {
        signIn.cancel()
    }
}

enum AccountsTab: String, CaseIterable {
    case accounts = "Accounts", cost = "Token Cost"
}

final class TabModel: ObservableObject {
    @Published var page = AccountsTab.accounts
}

/// One sign-in at a time, from the browser back to a saved account.
final class SignInModel: ObservableObject {
    enum Phase: Equatable {
        case idle
        case waiting(URL)
        case saving
        case added(String)
        case failed(String)
    }

    @Published var phase = Phase.idle
    /// Shown under the list after a switch fails.
    @Published var switchError: String?
    private let store: AccountStore
    private var login: AccountLogin?

    init(store: AccountStore) { self.store = store }

    func start() {
        login?.cancel()
        let login = AccountLogin()
        self.login = login
        phase = .saving
        login.start(ready: { [weak self] url in
            self?.phase = .waiting(url)
            NSWorkspace.shared.open(url)
        }, done: { [weak self] result in
            guard let self, self.login === login else { return }
            switch result {
            case .failure(let error):
                self.phase = .failed(error.localizedDescription)
            case .success(let oauth):
                self.phase = .saving
                self.store.add(oauth) { added in
                    switch added {
                    case .success(let account): self.phase = .added(account.email)
                    case .failure(let error): self.phase = .failed(error.localizedDescription)
                    }
                }
            }
        })
    }

    func cancel() {
        login?.cancel()
        login = nil
        if case .waiting = phase { phase = .idle }
    }

    func switchTo(_ id: String) {
        switchError = nil
        store.switchTo(id) { [weak self] error in self?.switchError = error?.localizedDescription }
    }
}

// MARK: - Views

struct AccountsView: View {
    @ObservedObject var accounts: AccountStore
    @ObservedObject var cost: CostTracker
    @ObservedObject var signIn: SignInModel
    @ObservedObject var tab: TabModel

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $tab.page) {
                ForEach(AccountsTab.allCases, id: \.self) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 240)
            .padding(.vertical, 12)
            Divider()
            switch tab.page {
            case .accounts: AccountsPage(accounts: accounts, signIn: signIn)
            case .cost: CostPage(cost: cost)
            }
        }
        .frame(width: 460, height: 540)
    }
}

private struct AccountsPage: View {
    @ObservedObject var accounts: AccountStore
    @ObservedObject var signIn: SignInModel
    @State private var removing: ClaudeAccount?

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(spacing: 10) {
                    if accounts.accounts.isEmpty {
                        Text("No accounts yet. Add one to see its limits and switch to it.")
                            .foregroundColor(.secondary)
                            .padding(.top, 40)
                    }
                    ForEach(accounts.accounts, id: \.id) { account in
                        AccountCard(account: account, usage: accounts.usage[account.id],
                                    isActive: account.id == accounts.activeID, busy: accounts.busy,
                                    onSwitch: { signIn.switchTo(account.id) },
                                    onRemove: { removing = account })
                    }
                    if let error = signIn.switchError {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.callout)
                            .foregroundColor(.orange)
                    }
                }
                .padding(16)
            }
            Divider()
            SignInPanel(signIn: signIn)
                .padding(16)
        }
        .alert(item: $removing) { account in
            Alert(title: Text("Remove \(account.email)?"),
                  message: Text(account.id == accounts.activeID
                                ? "Notchling forgets its saved login. Claude Code stays signed in to it."
                                : "Notchling forgets its saved login. Add it again to use it."),
                  primaryButton: .destructive(Text("Remove")) { accounts.remove(account.id) },
                  secondaryButton: .cancel())
        }
    }
}

extension ClaudeAccount: Identifiable {}

private struct AccountCard: View {
    let account: ClaudeAccount
    let usage: AccountUsage?
    let isActive: Bool
    let busy: Bool
    let onSwitch: () -> Void
    let onRemove: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                ZStack {
                    Circle().fill(isActive ? Color.accentColor : Color.secondary.opacity(0.25))
                    Text(account.email.prefix(1).uppercased())
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(isActive ? .white : .primary)
                }
                .frame(width: 30, height: 30)
                VStack(alignment: .leading, spacing: 1) {
                    Text(account.email).font(.system(size: 13, weight: .semibold)).lineLimit(1).truncationMode(.middle)
                    Text(account.plan).font(.caption).foregroundColor(.secondary)
                }
                Spacer()
                if isActive {
                    Text("In Use")
                        .font(.caption.weight(.semibold))
                        .foregroundColor(.accentColor)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .background(Capsule().fill(Color.accentColor.opacity(0.15)))
                } else {
                    Button("Switch", action: onSwitch).disabled(busy)
                }
                Menu {
                    Button("Remove…", action: onRemove)
                } label: {
                    Image(systemName: "ellipsis")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            }
            if account.needsLogin {
                Label("Signed out. Add this account again to sign in.", systemImage: "exclamationmark.circle")
                    .font(.caption).foregroundColor(.orange)
            } else if let usage {
                VStack(spacing: 6) {
                    if let s = usage.session { LimitRow(label: "5 hours", window: s, short: true) }
                    if let w = usage.week { LimitRow(label: "Week", window: w, short: false) }
                    ForEach(usage.models, id: \.name) { LimitRow(label: $0.name, window: $0.window, short: false) }
                }
            } else {
                Text(isActive ? "Checking limits…" : "Limits show once the saved login is checked.")
                    .font(.caption).foregroundColor(.secondary)
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 10)
            .stroke(isActive ? Color.accentColor.opacity(0.6) : Color.secondary.opacity(0.2), lineWidth: 1))
    }
}

private struct LimitRow: View {
    let label: String
    let window: UsageWindow
    let short: Bool

    var body: some View {
        HStack(spacing: 8) {
            Text(label).font(.caption).foregroundColor(.secondary).frame(width: 52, alignment: .leading)
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.secondary.opacity(0.18))
                    Capsule().fill(Color(nsColor: AccountRowView.color(window.percent)))
                        .frame(width: max(6, geo.size.width * min(window.percent / 100, 1)))
                }
            }
            .frame(height: 6)
            Text("\(Int(window.percent.rounded()))%")
                .font(.caption.monospacedDigit().weight(.medium))
                .frame(width: 36, alignment: .trailing)
            Text(window.resetsAt.map { "resets " + AccountRowView.resetText($0, short: short) } ?? "")
                .font(.caption).foregroundColor(.secondary)
                .frame(width: 118, alignment: .trailing)
        }
    }
}

private struct SignInPanel: View {
    @ObservedObject var signIn: SignInModel

    var body: some View {
        switch signIn.phase {
        case .idle:
            HStack {
                Text("Sign in to another Claude account to switch to it from here.")
                    .font(.callout).foregroundColor(.secondary)
                Spacer()
                Button("Add Account") { signIn.start() }.keyboardShortcut(.defaultAction)
            }
        case .waiting(let url):
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Sign in with the account to add in your browser…").font(.callout.weight(.medium))
                }
                Text("Browser already signed in to a different account? Copy the link into a private window.")
                    .font(.caption).foregroundColor(.secondary)
                HStack {
                    Button("Open Again") { NSWorkspace.shared.open(url) }
                    Button("Copy Link") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(url.absoluteString, forType: .string)
                    }
                    Spacer()
                    Button("Cancel") { signIn.cancel() }
                }
            }
        case .saving:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Saving the account…").font(.callout)
                Spacer()
            }
        case .added(let email):
            HStack {
                Label("Added \(email).", systemImage: "checkmark.circle.fill").foregroundColor(.green).font(.callout)
                Spacer()
                Button("Add Another") { signIn.start() }
                Button("Done") { signIn.phase = .idle }
            }
        case .failed(let message):
            HStack {
                Label(message, systemImage: "exclamationmark.triangle.fill").foregroundColor(.orange).font(.callout)
                Spacer()
                Button("Try Again") { signIn.start() }
                Button("Cancel") { signIn.phase = .idle }
            }
        }
    }
}

private struct CostPage: View {
    @ObservedObject var cost: CostTracker

    var body: some View {
        let s = cost.summary
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack {
                    Stat(title: "Today", value: s.today)
                    Stat(title: "7 Days", value: s.week)
                    Stat(title: "30 Days", value: s.month)
                }
                if cost.scanned == nil {
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Reading transcripts…").font(.callout).foregroundColor(.secondary)
                    }
                }
                DayBars(days: s.days)
                Breakdown(title: "By Model, 30 Days", rows: Array(s.models.prefix(6)))
                Breakdown(title: "By Project, 30 Days", rows: Array(s.projects.prefix(8)))
                Text("What Claude Code's tokens would cost at API prices, from its transcripts. Your plan isn't billed this way.")
                    .font(.caption).foregroundColor(.secondary)
            }
            .padding(20)
        }
    }
}

private struct Stat: View {
    let title: String
    let value: Double

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.caption).foregroundColor(.secondary)
            Text(CostChartView.dollars(value)).font(.system(size: 20, weight: .semibold)).monospacedDigit()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct DayBars: View {
    let days: [(day: Date, cost: Double)]

    var body: some View {
        let peak = max(days.map(\.cost).max() ?? 0, 0.01)
        let letter: DateFormatter = {
            let f = DateFormatter()
            f.setLocalizedDateFormatFromTemplate("EEEEE")
            return f
        }()
        VStack(alignment: .leading, spacing: 4) {
            Text("Last 14 Days").font(.caption.weight(.semibold)).foregroundColor(.secondary)
            HStack(alignment: .bottom, spacing: 6) {
                ForEach(Array(days.enumerated()), id: \.offset) { i, day in
                    VStack(spacing: 3) {
                        RoundedRectangle(cornerRadius: 3)
                            .fill(i == days.count - 1 ? Color.accentColor : Color.secondary.opacity(0.4))
                            .frame(height: max(2, CGFloat(day.cost / peak) * 90))
                            .help("\(day.day.formatted(date: .abbreviated, time: .omitted)): \(CostChartView.dollars(day.cost))")
                        Text(letter.string(from: day.day)).font(.system(size: 9)).foregroundColor(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .frame(height: 110, alignment: .bottom)
        }
    }
}

private struct Breakdown: View {
    let title: String
    let rows: [(name: String, cost: Double)]

    var body: some View {
        if !rows.isEmpty {
            let top = rows.first?.cost ?? 1
            VStack(alignment: .leading, spacing: 6) {
                Text(title).font(.caption.weight(.semibold)).foregroundColor(.secondary)
                ForEach(rows, id: \.name) { row in
                    HStack(spacing: 8) {
                        Text(row.name).font(.callout).lineLimit(1).frame(width: 130, alignment: .leading)
                        GeometryReader { geo in
                            Capsule().fill(Color.accentColor.opacity(0.5))
                                .frame(width: max(3, geo.size.width * row.cost / top))
                        }
                        .frame(height: 6)
                        Text(CostChartView.dollars(row.cost)).font(.callout.monospacedDigit()).frame(width: 64, alignment: .trailing)
                    }
                }
            }
        }
    }
}
