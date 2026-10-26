import AppKit
import SwiftUI

/// Notchling's window: every saved account with its limits, switching, signing in to another account,
/// token cost, and every setting from the menu. Everything `claude auth login`, /status and /usage were
/// needed for.
final class AccountsWindowController: NSObject, NSWindowDelegate {
    private let accounts: AccountStore
    private let antigravity: AntigravityStore
    private let desktop: ClaudeDesktopStore
    private let cost: CostTracker
    private let settings: SettingsModel
    private let signIn: SignInModel
    private let tab = TabModel()
    private var window: NSWindow?

    init(accounts: AccountStore, antigravity: AntigravityStore, desktop: ClaudeDesktopStore, cost: CostTracker, settings: SettingsModel) {
        self.accounts = accounts
        self.antigravity = antigravity
        self.desktop = desktop
        self.cost = cost
        self.settings = settings
        signIn = SignInModel(store: accounts)
    }

    func show(_ page: AccountsTab = .accounts, provider: Provider? = nil, signIn startSignIn: Bool = false) {
        tab.page = page
        if let provider { tab.provider = provider }
        if window == nil {
            let view = AccountsView(accounts: accounts, antigravity: antigravity, desktop: desktop, cost: cost,
                                    settings: settings, signIn: signIn, tab: tab)
            let window = NSWindow(contentViewController: NSHostingController(rootView: view))
            window.title = "Notchling"
            window.styleMask = [.titled, .closable, .miniaturizable]
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.center()
            self.window = window
        }
        accounts.sync()
        antigravity.sync()
        desktop.sync()
        cost.refresh()
        settings.reload?()
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
        if startSignIn { signIn.start() }
    }

    var isVisible: Bool { window?.isVisible == true }

    func windowWillClose(_ notification: Notification) {
        signIn.cancel()
    }
}

enum AccountsTab: String, CaseIterable {
    case accounts = "Accounts", cost = "Token Cost", settings = "Settings"
}

/// The menu's settings for the Settings tab. AppDelegate owns the real values: it fills these in
/// through `reload` and carries out each change through `apply`.
final class SettingsModel: ObservableObject {
    enum Change {
        case awakeMode(AwakeMode), keepDisplayOn(Bool), showNotch(Bool), notchWidth(NotchWidth)
        case animatePet(Bool), answerFromNotch(Bool), launchAtLogin(Bool)
    }

    @Published var awakeMode = AwakeMode.off { didSet { push(.awakeMode(awakeMode)) } }
    @Published var keepDisplayOn = false { didSet { push(.keepDisplayOn(keepDisplayOn)) } }
    @Published var showNotch = true { didSet { push(.showNotch(showNotch)) } }
    @Published var notchWidth = NotchWidth.medium { didSet { push(.notchWidth(notchWidth)) } }
    @Published var animatePet = true { didSet { push(.animatePet(animatePet)) } }
    @Published var answerFromNotch = true { didSet { push(.answerFromNotch(answerFromNotch)) } }
    @Published var launchAtLogin = false { didSet { push(.launchAtLogin(launchAtLogin)) } }

    var apply: ((Change) -> Void)?
    var reload: (() -> Void)?
    private var loading = false

    /// Sets values without sending them back as changes.
    func load(_ fill: (SettingsModel) -> Void) {
        loading = true
        fill(self)
        loading = false
    }

    private func push(_ change: Change) {
        guard !loading else { return }
        apply?(change)
        // A change can fail (Launch at Login outside /Applications); show what really happened.
        DispatchQueue.main.async { self.reload?() }
    }
}

enum Provider: String, CaseIterable {
    case claudeCode = "Claude Code", claudeDesktop = "Claude Desktop", antigravity = "Antigravity"
}

final class TabModel: ObservableObject {
    @Published var page = AccountsTab.accounts
    @Published var provider = Provider.claudeCode
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
    @ObservedObject var antigravity: AntigravityStore
    @ObservedObject var desktop: ClaudeDesktopStore
    @ObservedObject var cost: CostTracker
    @ObservedObject var settings: SettingsModel
    @ObservedObject var signIn: SignInModel
    @ObservedObject var tab: TabModel

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                Picker("", selection: $tab.page) {
                    ForEach(AccountsTab.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 330)
                HStack {
                    Spacer()
                    Button(action: refresh) {
                        Image(systemName: "arrow.clockwise")
                    }
                    .buttonStyle(.borderless)
                    .help("Refresh accounts, limits and cost")
                    .padding(.trailing, 16)
                }
            }
            .padding(.vertical, 12)
            Divider()
            switch tab.page {
            case .accounts:
                Picker("", selection: $tab.provider) {
                    ForEach(Provider.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 340)
                .padding(.top, 12)
                switch tab.provider {
                case .claudeCode: AccountsPage(accounts: accounts, cost: cost, signIn: signIn)
                case .claudeDesktop: DesktopPage(accounts: accounts, desktop: desktop)
                case .antigravity: AntigravityPage(store: antigravity)
                }
            case .cost: CostPage(cost: cost, accounts: accounts)
            case .settings: SettingsPage(settings: settings)
            }
        }
        .frame(width: 460, height: 540)
    }

    /// Re-reads both logins and rescans cost, for when something changed outside Notchling.
    private func refresh() {
        accounts.sync(force: true)
        antigravity.sync()
        cost.refresh()
        settings.reload?()
    }
}

private struct AccountsPage: View {
    @ObservedObject var accounts: AccountStore
    @ObservedObject var cost: CostTracker
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
                        AccountCard(account: account, usage: accounts.usage[account.id], cost: cost.byAccount[account.id],
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
    let cost: CostTracker.Summary?
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
            if let cost, cost.month > 0 {
                HStack(spacing: 4) {
                    Image(systemName: "dollarsign.circle").foregroundColor(.secondary)
                    Text("Today \(CostChartView.dollars(cost.today)) · 7 days \(CostChartView.dollars(cost.week)) · "
                         + "30 days \(CostChartView.dollars(cost.month))")
                }
                .font(.caption)
                .foregroundColor(.secondary)
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 10)
            .stroke(isActive ? Color.accentColor.opacity(0.6) : Color.secondary.opacity(0.2), lineWidth: 1))
    }
}

// MARK: - Claude Desktop

/// The same accounts as Claude Code, but switching the Claude desktop app. Switching and Add Account
/// restart the app, so both ask first while it's open.
private struct DesktopPage: View {
    @ObservedObject var accounts: AccountStore
    @ObservedObject var desktop: ClaudeDesktopStore
    @State private var error: String?
    @State private var confirming: Pending?
    @State private var removing: ClaudeAccount?

    enum Pending: Identifiable {
        case switchTo(String), add
        var id: String { if case .switchTo(let u) = self { return u }; return "add" }
    }

    private func uuid(_ a: ClaudeAccount) -> String { String(a.id.prefix { $0 != ":" }) }

    /// Only accounts that are actually in Claude Desktop — the one it's on, plus any Notchling has
    /// captured. Claude Code accounts not signed in to Desktop aren't listed here.
    private var inDesktop: [ClaudeAccount] {
        accounts.accounts.filter { desktop.activeUuid == uuid($0) || desktop.saved.contains(uuid($0)) }
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(spacing: 10) {
                    if inDesktop.isEmpty {
                        Text("No accounts in Claude Desktop yet. Use Add Account to sign in to one.")
                            .foregroundColor(.secondary).padding(.top, 40)
                    }
                    ForEach(inDesktop, id: \.id) { account in
                        DesktopCard(account: account, active: desktop.activeUuid == uuid(account),
                                    switchable: desktop.saved.contains(uuid(account)),
                                    busy: desktop.busy || desktop.adding,
                                    onSwitch: { ask(.switchTo(uuid(account))) },
                                    onRemove: { removing = account })
                    }
                    if let error {
                        Label(error, systemImage: "exclamationmark.triangle.fill").font(.callout).foregroundColor(.orange)
                    }
                }
                .padding(16)
            }
            Divider()
            addPanel.padding(16)
        }
        .alert(item: $confirming) { pending in
            Alert(title: Text(title(pending)),
                  message: Text("Claude Desktop quits and opens again. Save anything open there first."),
                  primaryButton: .default(Text("Continue")) { run(pending) },
                  secondaryButton: .cancel())
        }
        .background(EmptyView().alert(item: $removing) { account in
            Alert(title: Text("Remove \(account.email) from Desktop list?"),
                  message: Text("Notchling forgets its saved Desktop session. Claude Desktop stays signed in if "
                                + "it's the account in use; add it again by signing in to it in Desktop."),
                  primaryButton: .destructive(Text("Remove")) { desktop.forget(uuid(account)) },
                  secondaryButton: .cancel())
        })
    }

    @ViewBuilder private var addPanel: some View {
        if desktop.adding {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Sign in to the other account in Claude Desktop…").font(.callout.weight(.medium))
                    Text("Notchling saves it as soon as you're signed in.").font(.caption).foregroundColor(.secondary)
                }
                Spacer()
                Button("Cancel") { desktop.cancelAdd() }
            }
        } else {
            HStack {
                Text("Claude Desktop opens signed out so you can sign in to another account.")
                    .font(.callout).foregroundColor(.secondary)
                Spacer()
                Button("Add Account") { ask(.add) }.keyboardShortcut(.defaultAction).disabled(desktop.busy)
            }
        }
    }

    private func title(_ p: Pending) -> String {
        if case .add = p { return "Open Claude Desktop signed out?" }
        return "Restart Claude Desktop?"
    }

    private func ask(_ pending: Pending) {
        if ClaudeDesktopStore.isRunning { confirming = pending } else { run(pending) }
    }

    private func run(_ pending: Pending) {
        error = nil
        let done: (Error?) -> Void = { error = $0?.localizedDescription }
        switch pending {
        case .switchTo(let uuid): desktop.switchTo(uuid, done: done)
        case .add: desktop.beginAdd(done: done)
        }
    }
}

private struct DesktopCard: View {
    let account: ClaudeAccount
    let active: Bool
    let switchable: Bool
    let busy: Bool
    let onSwitch: () -> Void
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                Circle().fill(active ? Color.accentColor : Color.secondary.opacity(0.25))
                Text(account.email.prefix(1).uppercased())
                    .font(.system(size: 14, weight: .semibold)).foregroundColor(active ? .white : .primary)
            }
            .frame(width: 30, height: 30)
            VStack(alignment: .leading, spacing: 1) {
                Text(account.email).font(.system(size: 13, weight: .semibold)).lineLimit(1).truncationMode(.middle)
                Text(account.plan).font(.caption).foregroundColor(.secondary)
            }
            Spacer()
            if active {
                Text("In Use").font(.caption.weight(.semibold)).foregroundColor(.accentColor)
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(Capsule().fill(Color.accentColor.opacity(0.15)))
            } else if switchable {
                Button("Switch", action: onSwitch).disabled(busy)
            }
            Menu {
                Button("Remove from List…", action: onRemove)
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 10)
            .stroke(active ? Color.accentColor.opacity(0.6) : Color.secondary.opacity(0.2), lineWidth: 1))
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
    @ObservedObject var accounts: AccountStore
    /// "" is All Accounts; otherwise an account id, or "-" for cost from before accounts were tracked.
    @State private var filter = ""

    private func name(_ id: String) -> String {
        id.isEmpty ? "Before Tracking" : accounts.accounts.first { $0.id == id }?.email ?? "Removed Account"
    }

    /// Lets cost from before tracking count toward one account, or stay on its own.
    @ViewBuilder private var untrackedChoice: some View {
        if cost.summary.accounts.contains(where: { $0.id.isEmpty }) || cost.untrackedOwner != nil {
            HStack {
                Text("Cost from before tracking counts toward").font(.caption).foregroundColor(.secondary)
                Picker("", selection: Binding(get: { cost.untrackedOwner ?? "" },
                                              set: { cost.untrackedOwner = $0.isEmpty ? nil : $0 })) {
                    Text("No Account").tag("")
                    ForEach(accounts.accounts) { Text($0.email).tag($0.id) }
                }
                .labelsHidden()
                .fixedSize()
            }
        }
    }

    var body: some View {
        let s = filter.isEmpty ? cost.summary : cost.byAccount[filter == "-" ? "" : filter] ?? CostTracker.Summary()
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Picker("Account", selection: $filter) {
                    Text("All Accounts").tag("")
                    ForEach(cost.summary.accounts, id: \.id) { row in
                        Text(name(row.id)).tag(row.id.isEmpty ? "-" : row.id)
                    }
                }
                .frame(maxWidth: 320)
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
                if filter.isEmpty {
                    Breakdown(title: "By Account, 30 Days", rows: s.accounts.map { (name($0.id), $0.cost) })
                    untrackedChoice
                }
                Breakdown(title: "By Model, 30 Days", rows: Array(s.models.prefix(6)))
                Breakdown(title: "By Project, 30 Days", rows: Array(s.projects.prefix(8)))
                Text("What Claude Code's tokens would cost at API prices, from its transcripts. Your plan isn't billed "
                     + "this way. Each message counts toward the account in use at the time; Before Tracking is cost from "
                     + "before Notchling followed which account was in use.")
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

private struct SettingsPage: View {
    @ObservedObject var settings: SettingsModel

    var body: some View {
        Form {
            Section("Keep Mac Awake") {
                Picker("Keep awake", selection: $settings.awakeMode) {
                    ForEach(AwakeMode.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                Toggle("Keep display on", isOn: $settings.keepDisplayOn)
            }
            Section("Notch") {
                Toggle("Show agents in the notch", isOn: $settings.showNotch)
                Picker("Width", selection: $settings.notchWidth) {
                    ForEach(NotchWidth.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                Toggle("Animate pet", isOn: $settings.animatePet)
            }
            Section {
                Toggle("Answer prompts from the notch", isOn: $settings.answerFromNotch)
            } header: {
                Text("Claude Code")
            } footer: {
                Text("Adds a hook to ~/.claude/settings.json so questions and permission prompts can be answered here.")
                    .font(.caption).foregroundColor(.secondary)
            }
            Section("General") {
                Toggle("Launch at login", isOn: $settings.launchAtLogin)
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - Antigravity

/// Antigravity's saved Google accounts. Switching and adding restart Antigravity, so both ask first
/// while it's open.
private struct AntigravityPage: View {
    @ObservedObject var store: AntigravityStore
    @State private var removing: AntigravityAccount?
    @State private var confirming: Pending?
    @State private var error: String?

    enum Pending: Identifiable {
        case switchTo(AntigravityAccount), add
        var id: String {
            if case .switchTo(let account) = self { return account.id }
            return "add"
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(spacing: 10) {
                    if store.accounts.isEmpty {
                        Text("Sign in to Antigravity and Notchling saves the account here.")
                            .foregroundColor(.secondary)
                            .padding(.top, 40)
                    }
                    ForEach(store.accounts) { account in
                        AntigravityCard(account: account, usage: store.usage[account.id],
                                        isActive: account.id == store.activeID,
                                        busy: store.busy || store.adding,
                                        onSwitch: { ask(.switchTo(account)) }, onRemove: { removing = account })
                    }
                    if let error {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.callout)
                            .foregroundColor(.orange)
                    }
                }
                .padding(16)
            }
            Divider()
            addPanel.padding(16)
        }
        .alert(item: $confirming) { pending in
            Alert(title: Text("Restart Antigravity?"),
                  message: Text("Antigravity quits and opens again on the other account. Agents running in it stop."),
                  primaryButton: .default(Text("Restart")) { run(pending) },
                  secondaryButton: .cancel())
        }
        .background(EmptyView().alert(item: $removing) { account in
            Alert(title: Text("Remove \(account.email)?"),
                  message: Text(account.id == store.activeID
                                ? "Notchling forgets its saved login. Antigravity stays signed in to it."
                                : "Notchling forgets its saved login. Sign in to it in Antigravity to add it again."),
                  primaryButton: .destructive(Text("Remove")) { store.remove(account.id) },
                  secondaryButton: .cancel())
        })
    }

    @ViewBuilder private var addPanel: some View {
        if store.adding {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Sign in to the other account in Antigravity…").font(.callout.weight(.medium))
                    Text("Notchling saves it as soon as you're signed in.").font(.caption).foregroundColor(.secondary)
                }
                Spacer()
                Button("Cancel") { store.cancelAdd() }
            }
        } else {
            HStack {
                Text("Antigravity opens signed out so you can sign in to another Google account.")
                    .font(.callout).foregroundColor(.secondary)
                Spacer()
                Button("Add Account") { ask(.add) }.keyboardShortcut(.defaultAction).disabled(store.busy)
            }
        }
    }

    private func ask(_ pending: Pending) {
        if AntigravityStore.isRunning { confirming = pending } else { run(pending) }
    }

    private func run(_ pending: Pending) {
        error = nil
        let done: (Error?) -> Void = { error = $0?.localizedDescription }
        switch pending {
        case .switchTo(let account): store.switchTo(account.id, done: done)
        case .add: store.beginAdd(done: done)
        }
    }
}

private struct AntigravityCard: View {
    let account: AntigravityAccount
    let usage: AntigravityUsage?
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
                    Text(account.plan ?? "Google Account").font(.caption).foregroundColor(.secondary)
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
            if let usage {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(usage.groups.indices, id: \.self) { i in
                        let group = usage.groups[i]
                        VStack(alignment: .leading, spacing: 6) {
                            Text(group.name).font(.caption.weight(.medium)).foregroundColor(.secondary)
                            ForEach(group.bars.indices, id: \.self) { j in
                                let short = group.bars[j].label == "5h"
                                LimitRow(label: short ? "5 hours" : group.bars[j].label,
                                         window: group.bars[j].window, short: short)
                            }
                        }
                    }
                }
            } else if isActive {
                Text("Open Antigravity to read its limits.").font(.caption).foregroundColor(.secondary)
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 10)
            .stroke(isActive ? Color.accentColor.opacity(0.6) : Color.secondary.opacity(0.2), lineWidth: 1))
    }
}
