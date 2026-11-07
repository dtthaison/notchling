import AppKit
import SwiftUI

/// What a new session runs on: a Claude Code account (nil is the main login; another gets a lane),
/// a Codex account (Codex has one login, so a non-active one switches Codex first), or an Antigravity
/// account for Antigravity CLI (the app's login, which `agy` shares; nil: whatever `agy` is signed in
/// to, when Notchling has none saved).
/// Add a case per tool.
enum SessionChoice: Hashable {
    case claude(accountID: String?)
    case codex(accountID: String)
    case antigravity(accountID: String?)

    /// Remembered between sheets.
    var key: String {
        switch self {
        case .claude(let id): return "claude:" + (id ?? "")
        case .codex(let id): return "codex:" + id
        case .antigravity(let id): return "antigravity:" + (id ?? "")
        }
    }

    init?(key: String) {
        if key.hasPrefix("claude:") {
            let id = String(key.dropFirst(7))
            self = .claude(accountID: id.isEmpty ? nil : id)
        } else if key.hasPrefix("codex:") {
            self = .codex(accountID: String(key.dropFirst(6)))
        } else if key.hasPrefix("antigravity:") {
            let id = String(key.dropFirst(12))
            self = .antigravity(accountID: id.isEmpty ? nil : id)
        } else {
            return nil
        }
    }
}

/// New Session: pick the account (every Claude Code and Codex account, each tool with Add Account),
/// then the folder, then Start. Adding an account happens in place and comes back here.
struct NewSessionSheet: View {
    @ObservedObject var model: ChatModel
    @State private var choice: SessionChoice?
    @State private var folder: String?
    @State private var extraFolders: [String] = []
    /// Adding an account in place of the lists.
    @State private var adding: ChatModel.AccountTab?
    @State private var confirmSwitch: CodexAccount?
    @State private var working = false
    @State private var error: String?
    /// Claude Code refused the folder until it's trusted: Start becomes Trust Folder and Start.
    @State private var untrusted = false

    private static let lastChoiceKey = "newSessionChoice"

    var body: some View {
        VStack(spacing: 0) {
            header
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            if let adding {
                addPane(adding)
            } else {
                HStack(spacing: 0) {
                    accountColumn.frame(width: 300)
                    Rectangle().fill(KitInk.hairline).frame(width: 1)
                    folderColumn.frame(maxWidth: .infinity)
                }
                Rectangle().fill(KitInk.hairline).frame(height: 1)
                footer
            }
        }
        .frame(width: 660, height: 480)
        .background(KitInk.window)
        .onAppear(perform: pickDefaults)
        .onChange(of: folder) { _ in untrusted = false; error = nil }
        .onChange(of: choice) { _ in untrusted = false; error = nil }
        .alert(item: $confirmSwitch) { account in
            Alert(title: Text("Switch Codex to \(account.email)?"),
                  message: Text(CodexStore.switchWarning(CodexStore.runningWork())
                                ?? "Codex has one login, so every Codex session uses this account from now on."),
                  primaryButton: .default(Text("Switch and Start")) { start(switchingCodex: true) },
                  secondaryButton: .cancel())
        }
        .part("New Session Sheet")
    }

    // MARK: Parts

    private var header: some View {
        HStack(spacing: 10) {
            if adding != nil {
                Button { leaveAdding() } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "chevron.left").font(.system(size: 10, weight: .semibold))
                        Text("New Session")
                    }
                }
                .buttonStyle(.kit(.ghost))
                .part("Back to New Session Button", .bottom)
                Text(adding == .codex ? "Add Codex Account" : adding == .antigravity ? "Add Antigravity Account" : "Add Claude Code Account")
                    .kitType(.heading).foregroundColor(KitInk.text)
            } else {
                Image(systemName: "plus.bubble").foregroundColor(KitInk.textMuted)
                Text("New Session").kitType(.heading).foregroundColor(KitInk.text)
            }
            Spacer()
            Button("Cancel") { model.newSessionSheet = false }
                .buttonStyle(.kit(.secondary))
                .keyboardShortcut(.cancelAction)
        }
        .padding(.horizontal, 16)
        .frame(height: KitMetric.titleBar + 6)
        .background(KitInk.card)
    }

    private var accountColumn: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                columnTitle("Account")
                group("Claude Code", icon: "sparkle") {
                    ForEach(model.sessionTargets, id: \.title) { target in
                        option(.claude(accountID: target.accountID), title: target.title,
                               detail: claudeDetail(target))
                    }
                    addRow("Add Claude Code Account") { enterAdding(.claude) }
                }
                if let codex = model.codex {
                    group("Codex", icon: "chevron.left.forwardslash.chevron.right") {
                        ForEach(codex.accounts) { account in
                            option(.codex(accountID: account.id), title: account.email,
                                   detail: codexDetail(account, active: account.id == codex.activeID))
                        }
                        if codex.accounts.isEmpty {
                            Text(CodexStore.binary() == nil ? "Install the Codex CLI to use it here." : "No Codex account yet.")
                                .kitType(.meta).foregroundColor(KitInk.textFaint).padding(.horizontal, 10)
                        }
                        if CodexStore.binary() != nil {
                            addRow("Add Codex Account") { enterAdding(.codex) }
                        }
                    }
                }
                if AntigravityRunner.binary() != nil {
                    group("Antigravity CLI", icon: "paperplane") {
                        if let store = model.antigravity, !store.accounts.isEmpty {
                            ForEach(store.accounts) { account in
                                option(.antigravity(accountID: account.id), title: account.email,
                                       detail: antigravityDetail(account, active: account.id == store.activeID))
                            }
                        } else {
                            option(.antigravity(accountID: nil), title: "Antigravity CLI", detail: "Uses agy's current sign-in")
                        }
                        if model.antigravity != nil {
                            addRow("Add Antigravity Account") { enterAdding(.antigravity) }
                        }
                    }
                }
            }
            .padding(12)
        }
        .part("Account Column", .trailing)
    }

    private var folderColumn: some View {
        let folders = allFolders
        return ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                columnTitle("Folder")
                ForEach(folders, id: \.self) { path in
                    Button { folder = path } label: {
                        HStack(spacing: 8) {
                            Image(systemName: "folder").font(.system(size: 12)).foregroundColor(KitInk.textMuted)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(ChatModel.folderTitle(path, among: folders)).kitType(.label).foregroundColor(KitInk.text).lineLimit(1)
                                Text((path as NSString).deletingLastPathComponent.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                                    .kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1).truncationMode(.head)
                            }
                        }
                    }
                    .buttonStyle(ChoiceRowStyle(selected: folder == path))
                    .help(path)
                }
                Button(action: chooseFolder) {
                    HStack(spacing: 8) {
                        Image(systemName: "folder.badge.plus").font(.system(size: 12))
                        Text("Choose Folder…").kitType(.label)
                    }
                    .foregroundColor(KitInk.accentText)
                }
                .buttonStyle(ChoiceRowStyle(selected: false))
                .part("Choose Folder Button", .top)
            }
            .padding(12)
        }
        .part("Folder Column", .leading)
    }

    private var footer: some View {
        HStack(spacing: 10) {
            if let error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .kitType(.meta).foregroundColor(.orange).lineLimit(2)
            } else {
                Text(summary).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(2)
            }
            Spacer()
            if working { ProgressView().controlSize(.small) }
            Button(untrusted ? "Trust Folder and Start" : "Start Session") { start(switchingCodex: false) }
                .buttonStyle(.kit(.primary))
                .keyboardShortcut(.defaultAction)
                .disabled(choice == nil || folder == nil || working)
                .help(untrusted ? "Accepts Claude Code's “Do you trust this folder?” prompt for it, as running claude there would, then starts the session." : "")
                .part("Start Session Button", .top)
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
    }

    @ViewBuilder private func addPane(_ tab: ChatModel.AccountTab) -> some View {
        switch tab {
        case .claude: ClaudeSignInPane(model: model)
        case .codex: if let codex = model.codex { CodexAccountsPane(store: codex) }
        case .antigravity: if let store = model.antigravity { AntigravityPage(store: store) }
        }
    }

    // MARK: Pieces

    private func columnTitle(_ text: String) -> some View {
        Text(text.uppercased()).kitType(.section).foregroundColor(KitInk.textFaint)
            .padding(.horizontal, 10).padding(.bottom, 4)
    }

    private func group<Content: View>(_ title: String, icon: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 10, weight: .semibold))
                Text(title).kitType(.label)
            }
            .foregroundColor(KitInk.textMuted)
            .padding(.horizontal, 10).padding(.top, 8).padding(.bottom, 2)
            content()
        }
    }

    private func option(_ value: SessionChoice, title: String, detail: String) -> some View {
        Button { choice = value; error = nil } label: {
            HStack(spacing: 8) {
                Image(systemName: choice == value ? "largecircle.fill.circle" : "circle")
                    .font(.system(size: 12))
                    .foregroundColor(choice == value ? KitInk.accent : KitInk.textFaint)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).kitType(.label).foregroundColor(KitInk.text).lineLimit(1).truncationMode(.middle)
                    if !detail.isEmpty {
                        Text(detail).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1)
                    }
                }
            }
        }
        .buttonStyle(ChoiceRowStyle(selected: choice == value))
    }

    private func addRow(_ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: "plus.circle").font(.system(size: 12))
                Text(title).kitType(.label)
            }
            .foregroundColor(KitInk.accentText)
        }
        .buttonStyle(ChoiceRowStyle(selected: false))
        .part(title + " Row", .trailing)
    }

    private func claudeDetail(_ target: ChatModel.SessionTarget) -> String {
        guard let accounts = model.accounts else { return "" }
        let id = target.accountID ?? accounts.activeID
        var parts: [String] = []
        if let plan = accounts.accounts.first(where: { $0.id == id })?.plan { parts.append(plan) }
        if let usage = id.flatMap({ accounts.usage[$0] }) {
            if let s = usage.session { parts.append("5h \(Int(s.percent.rounded()))%") }
            if let w = usage.week { parts.append("Week \(Int(w.percent.rounded()))%") }
        }
        if target.accountID == nil { parts.append("Main login") }
        return parts.joined(separator: " · ")
    }

    private func codexDetail(_ account: CodexAccount, active: Bool) -> String {
        var parts = [account.plan ?? "ChatGPT"]
        if let bars = model.codex?.usage[account.id]?.groups.first?.bars {
            parts += bars.map { "\($0.label) \(Int($0.window.percent.rounded()))%" }
        }
        parts.append(active ? "In use" : "Switches Codex")
        return parts.joined(separator: " · ")
    }

    private func antigravityDetail(_ account: AntigravityAccount, active: Bool) -> String {
        var parts = [account.plan ?? "Google Account"]
        if let bars = model.antigravity?.usage[account.id]?.groups.first?.bars {
            parts += bars.map { "\($0.label) \(Int($0.window.percent.rounded()))%" }
        }
        parts.append(active ? "In use" : "Switches Antigravity")
        return parts.joined(separator: " · ")
    }


    private var allFolders: [String] {
        var out = extraFolders
        for f in model.projectFolders where !out.contains(f) { out.append(f) }
        return out
    }

    private var summary: String {
        guard let choice, let folder else { return "Pick an account and a folder." }
        let name = (folder as NSString).lastPathComponent
        switch choice {
        case .claude: return "A background Claude Code session in \(name). It opens here, ready for your first message."
        case .codex: return "A Codex session in \(name). Your first message starts it."
        case .antigravity: return "An Antigravity CLI session in \(name). Your first message starts it."
        }
    }

    // MARK: Actions

    private func pickDefaults() {
        model.codex?.sync()
        model.antigravity?.sync()
        if choice == nil {
            let saved = UserDefaults.standard.string(forKey: Self.lastChoiceKey).flatMap(SessionChoice.init(key:))
            if let saved, isAvailable(saved) { choice = saved } else if !model.sessionTargets.isEmpty { choice = .claude(accountID: nil) }
        }
        if folder == nil { folder = model.selected?.cwd.nilIfEmpty.flatMap { allFolders.contains($0) ? $0 : nil } ?? allFolders.first }
    }

    private func isAvailable(_ choice: SessionChoice) -> Bool {
        switch choice {
        case .claude(let id): return model.sessionTargets.contains { $0.accountID == id }
        case .codex(let id): return model.codex?.accounts.contains { $0.id == id } == true
        case .antigravity(let id):
            guard AntigravityRunner.binary() != nil else { return false }
            return id == nil ? (model.antigravity?.accounts.isEmpty ?? true) : model.antigravity?.accounts.contains { $0.id == id } == true
        }
    }

    private func enterAdding(_ tab: ChatModel.AccountTab) {
        adding = tab
        switch tab {
        case .claude: model.startClaudeSignIn()
        case .codex: model.codex?.sync(force: true)
        case .antigravity: model.antigravity?.sync()
        }
    }

    private func leaveAdding() {
        let tab = adding
        adding = nil
        model.signIn = nil
        // Pick what was just added.
        if tab == .codex, let codex = model.codex, let newest = codex.accounts.last, !isPicked(.codex(accountID: newest.id)) {
            choice = .codex(accountID: newest.id)
        }
        if tab == .antigravity, let store = model.antigravity, let newest = store.accounts.last,
           !isPicked(.antigravity(accountID: newest.id)) {
            choice = .antigravity(accountID: newest.id)
        }
    }

    private func isPicked(_ c: SessionChoice) -> Bool { choice == c }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.prompt = "Choose"
        panel.message = "The project folder for the new session."
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        model.addProjectFolder(path)
        extraFolders.removeAll { $0 == path }
        extraFolders.insert(path, at: 0)
        folder = path
    }

    private func confirmAntigravitySwitch(to account: AntigravityAccount?) -> Bool {
        let alert = NSAlert()
        alert.messageText = "Switch Antigravity to \(account?.email ?? "This Account")?"
        alert.informativeText = "Antigravity CLI and the Antigravity app share one login. The app quits and opens again on this account, and Antigravity sessions open in Chat restart. agy running in a terminal keeps the old login until you restart it."
        alert.addButton(withTitle: "Switch and Start")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }

    private func start(switchingCodex: Bool, switchingAntigravity: Bool = false) {
        guard let choice, let folder else { return }
        error = nil
        UserDefaults.standard.set(choice.key, forKey: Self.lastChoiceKey)
        model.addProjectFolder(folder)
        switch choice {
        case .claude(let accountID):
            // Stays open until it has started, so a failure shows here instead of behind it.
            working = true
            let trusting = untrusted
            untrusted = false
            model.newBackgroundSession(in: folder, on: accountID, trust: trusting) { failure in
                working = false
                guard let failure else { model.newSessionSheet = false; return }
                if ReplySender.isUntrusted(failure) {
                    untrusted = true
                    error = "Claude Code hasn't trusted this folder yet."
                } else {
                    error = failure
                }
            }
        case .codex(let accountID):
            guard let codex = model.codex else { return }
            if accountID == codex.activeID {
                model.newSessionSheet = false
                model.newCodexSession(in: folder)
            } else if !switchingCodex {
                confirmSwitch = codex.accounts.first { $0.id == accountID }
            } else {
                working = true
                codex.switchTo(accountID) { failure in
                    working = false
                    if let failure {
                        error = failure.localizedDescription
                    } else {
                        model.newSessionSheet = false
                        model.newCodexSession(in: folder)
                    }
                }
            }
        case .antigravity(let accountID):
            guard let store = model.antigravity, let accountID, accountID != store.activeID else {
                model.newSessionSheet = false
                model.newAntigravitySession(in: folder)
                return
            }
            // An NSAlert, not a second SwiftUI .alert: one on an EmptyView never showed, so Start did nothing.
            guard switchingAntigravity || confirmAntigravitySwitch(to: store.accounts.first { $0.id == accountID }) else { return }
            // agy may renew its login while it runs and write it back; none may run across the switch.
            AntigravityRunner.shared.closeAll()
            working = true
            store.switchTo(accountID) { failure in
                working = false
                if let failure {
                    error = failure.localizedDescription
                } else {
                    model.newSessionSheet = false
                    model.newAntigravitySession(in: folder)
                }
            }
        }
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}

/// A pickable row, shorter than a session row so every tool's accounts fit without scrolling.
private struct ChoiceRowStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        RowBody(configuration: configuration, selected: selected)
    }

    private struct RowBody: View {
        let configuration: Configuration
        let selected: Bool
        @State private var hovering = false

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            configuration.label
                .padding(.horizontal, 10).padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(shape.fill(selected ? KitInk.selection
                                       : configuration.isPressed ? KitInk.raised
                                       : hovering ? KitInk.raised.opacity(0.6) : .clear))
                .overlay(shape.strokeBorder(KitInk.accent.opacity(selected ? 0.35 : 0), lineWidth: 1))
                .contentShape(shape)
                .onHover { hovering = $0 }
                .pointerCursor()
                .animation(KitMotion.hover, value: hovering)
        }
    }
}
