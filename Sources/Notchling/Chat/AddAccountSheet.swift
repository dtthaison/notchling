import AppKit
import SwiftUI

/// Chat's accounts sheet, so adding an account never needs the menu bar. Claude Code: the browser
/// sign-in the Accounts window uses; the new account is saved beside the others and sessions stay
/// where they are until one is moved to it or started on it. Codex: the CLI's saved ChatGPT accounts,
/// with Add Account (`codex login` in a scratch home) and Switch.
struct AccountsSheet: View {
    @ObservedObject var model: ChatModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "person.crop.circle.badge.plus").foregroundColor(KitInk.textMuted)
                Text("Accounts").kitType(.heading).foregroundColor(KitInk.text)
                if model.codex != nil {
                    Picker("", selection: $model.accountTab) {
                        ForEach(ChatModel.AccountTab.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .frame(width: 190)
                    .part("Accounts Tabs", .bottom)
                }
                Spacer()
                Button("Done") { model.accountSheet = false }
                    .buttonStyle(.kit(.secondary))
                    .keyboardShortcut(.cancelAction)
                    .part("Accounts Done Button", .bottom)
            }
            .padding(.horizontal, 16)
            .frame(height: KitMetric.titleBar + 6)
            .background(KitInk.card)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            Group {
                switch model.accountTab {
                case .claude: ClaudeSignInPane(model: model)
                case .codex: if let codex = model.codex { CodexAccountsPane(store: codex) }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .frame(width: 480, height: 360)
        .background(KitInk.window)
        .part("Accounts Sheet")
    }
}

// MARK: - Claude Code

struct ClaudeSignInPane: View {
    @ObservedObject var model: ChatModel

    var body: some View {
        Group {
            if let signIn = model.signIn {
                ClaudeSignInSteps(signIn: signIn) { model.startClaudeSignIn() }
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Sign in to another Claude account in your browser. Your sessions keep the account they use.")
                        .kitType(.body).foregroundColor(KitInk.textMuted)
                    Button("Add Account") { model.startClaudeSignIn() }
                        .buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(20)
    }
}

private struct ClaudeSignInSteps: View {
    @ObservedObject var signIn: SignInModel
    let again: () -> Void

    var body: some View {
        switch signIn.phase {
        case .idle:
            Button("Add Account", action: again).buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
        case .waiting(let url):
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Sign in with the account to add in your browser…").kitType(.label).foregroundColor(KitInk.text)
                }
                Text("Browser already signed in to a different account? Copy the link into a private window.")
                    .kitType(.meta).foregroundColor(KitInk.textMuted)
                HStack(spacing: 8) {
                    Button("Open Again") { NSWorkspace.shared.open(url) }.buttonStyle(.kit(.secondary))
                    Button("Copy Link") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(url.absoluteString, forType: .string)
                    }
                    .buttonStyle(.kit(.secondary))
                    Button("Cancel") { signIn.cancel(); signIn.phase = .idle }.buttonStyle(.kit(.ghost))
                }
            }
        case .saving:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Saving the account…").kitType(.label).foregroundColor(KitInk.text)
            }
        case .added(let email):
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 8) {
                    Image(systemName: "checkmark.circle.fill").foregroundColor(KitInk.done)
                    Text("Added \(email).").kitType(.label).foregroundColor(KitInk.text)
                }
                Text("Move a session to it from the session's account menu, or start one on it with New Session.")
                    .kitType(.meta).foregroundColor(KitInk.textMuted)
                Button("Add Another", action: again).buttonStyle(.kit(.secondary))
            }
        case .failed(let message):
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundColor(.orange)
                    Text(message).kitType(.label).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
                }
                Button("Try Again", action: again).buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
            }
        }
    }
}

// MARK: - Codex

struct CodexAccountsPane: View {
    @ObservedObject var store: CodexStore
    @State private var confirming: Confirm?
    @State private var removing: CodexAccount?
    @State private var note: (text: String, isError: Bool)?

    struct Confirm: Identifiable {
        let account: CodexAccount
        let message: String
        var id: String { account.id }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    if store.accounts.isEmpty {
                        Text("No Codex accounts yet. Add one to switch the Codex CLI to it.")
                            .kitType(.body).foregroundColor(KitInk.textMuted)
                    }
                    ForEach(store.accounts) { account in
                        CodexAccountLine(account: account, usage: store.usage[account.id],
                                         isActive: account.id == store.activeID,
                                         busy: store.busy || store.adding,
                                         onSwitch: { ask(account) }, onRemove: { removing = account })
                    }
                }
                .padding(16)
            }
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            addBar.padding(.horizontal, 16).padding(.vertical, 12)
        }
        .alert(item: $confirming) { pending in
            Alert(title: Text("Switch Codex to \(pending.account.email)?"),
                  message: Text(pending.message),
                  primaryButton: .default(Text("Switch")) { run(pending.account) },
                  secondaryButton: .cancel())
        }
        .background(EmptyView().alert(item: $removing) { account in
            Alert(title: Text("Remove \(account.email)?"),
                  message: Text(account.id == store.activeID
                                ? "Notchling forgets its saved login. Codex stays signed in to it."
                                : "Notchling forgets its saved login. Add it again to use it."),
                  primaryButton: .destructive(Text("Remove")) { store.remove(account.id) },
                  secondaryButton: .cancel())
        })
    }

    @ViewBuilder private var addBar: some View {
        if store.adding {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Sign in to the other account in your browser…").kitType(.label).foregroundColor(KitInk.text)
                    Text("Codex keeps using the current account.").kitType(.meta).foregroundColor(KitInk.textMuted)
                }
                Spacer()
                if let url = store.signInURL {
                    Button("Open Again") { NSWorkspace.shared.open(url) }.buttonStyle(.kit(.secondary))
                }
                Button("Cancel") { store.cancelAdd() }.buttonStyle(.kit(.ghost))
            }
        } else {
            HStack(spacing: 8) {
                if let note {
                    Label(note.text, systemImage: note.isError ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                        .kitType(.meta)
                        .foregroundColor(note.isError ? .orange : KitInk.textMuted)
                        .lineLimit(2)
                } else {
                    Text("Signs in with codex login in your browser.").kitType(.meta).foregroundColor(KitInk.textMuted)
                }
                Spacer()
                Button("Add Account", action: add)
                    .buttonStyle(.kit(.primary))
                    .keyboardShortcut(.defaultAction)
                    .disabled(store.busy)
                    .part("Add Codex Account Button", .top)
            }
        }
    }

    private func add() {
        note = nil
        let before = Set(store.accounts.map(\.id))
        store.beginAdd { error in
            if let error {
                note = (error.localizedDescription, true)
            } else if let added = store.accounts.first(where: { !before.contains($0.id) }) {
                note = ("Added \(added.email).", false)
            }
        }
    }

    private func ask(_ account: CodexAccount) {
        if let warning = CodexStore.switchWarning(CodexStore.runningWork()) {
            confirming = Confirm(account: account, message: warning)
        } else {
            run(account)
        }
    }

    private func run(_ account: CodexAccount) {
        note = nil
        store.switchTo(account.id) { error in
            note = error.map { ($0.localizedDescription, true) } ?? ("Codex now uses \(account.email).", false)
        }
    }
}

private struct CodexAccountLine: View {
    let account: CodexAccount
    let usage: GroupedUsage?
    let isActive: Bool
    let busy: Bool
    let onSwitch: () -> Void
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                Circle().fill(isActive ? KitInk.accent : KitInk.raised)
                Text(account.email.prefix(1).uppercased())
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundColor(isActive ? .white : KitInk.text)
            }
            .frame(width: 26, height: 26)
            VStack(alignment: .leading, spacing: 2) {
                Text(account.email).kitType(.label).foregroundColor(KitInk.text).lineLimit(1).truncationMode(.middle)
                Text(detail).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
            }
            Spacer(minLength: 8)
            if isActive {
                Text("In Use").kitType(.meta).foregroundColor(KitInk.accent)
            } else {
                Button("Switch", action: onSwitch).buttonStyle(.kit(.secondary)).disabled(busy)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(KitInk.card))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(isActive ? KitInk.accent.opacity(0.6) : KitInk.hairline, lineWidth: 1))
        .contextMenu { Button("Remove…", action: onRemove) }
        .part("Codex Account Row")
    }

    /// "Plus · 5h 42% · Week 7%": the plan, then each Codex limit used.
    private var detail: String {
        let bars = usage?.groups.first?.bars.map { "\($0.label) \(Int($0.window.percent.rounded()))%" } ?? []
        return ([account.plan ?? "ChatGPT"] + bars).joined(separator: " · ")
    }
}
