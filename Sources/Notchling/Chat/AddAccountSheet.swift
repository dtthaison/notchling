import AppKit
import SwiftUI

extension SignInModel: Identifiable {}

/// Chat's Add Account: the browser sign-in the Accounts window uses, in a sheet over the chat.
/// The new account is saved beside the others; sessions stay where they are until one is moved to
/// it (a session's account menu) or started on it (New Session).
struct AddAccountSheet: View {
    @ObservedObject var signIn: SignInModel
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "person.crop.circle.badge.plus").foregroundColor(KitInk.textMuted)
                Text("Add Account").kitType(.heading).foregroundColor(KitInk.text)
                Spacer()
                if !isDone {
                    Button("Cancel", action: close)
                        .buttonStyle(.kit(.secondary))
                        .keyboardShortcut(.cancelAction)
                        .part("Add Account Cancel Button", .bottom)
                }
            }
            .padding(.horizontal, 16)
            .frame(height: KitMetric.titleBar + 6)
            .background(KitInk.card)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            content
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
            Spacer(minLength: 0)
        }
        .frame(width: 440, height: 200)
        .background(KitInk.window)
        .part("Add Account Sheet")
    }

    private var isDone: Bool {
        if case .added = signIn.phase { return true }
        return false
    }

    @ViewBuilder private var content: some View {
        switch signIn.phase {
        case .idle:
            VStack(alignment: .leading, spacing: 12) {
                Text("Sign in to another Claude account in your browser. Your sessions keep the account they use.")
                    .kitType(.body).foregroundColor(KitInk.textMuted)
                Button("Sign In") { signIn.start() }.buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
            }
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
                HStack(spacing: 8) {
                    Button("Add Another") { signIn.start() }.buttonStyle(.kit(.secondary))
                    Button("Done", action: close).buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
                }
            }
        case .failed(let message):
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundColor(.orange)
                    Text(message).kitType(.label).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
                }
                Button("Try Again") { signIn.start() }.buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
            }
        }
    }
}
