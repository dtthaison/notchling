import AppKit
import SwiftUI

/// Chat's Secret Vault sheet: the saved credentials, and a form to add or edit one. Values go straight to
/// Keychain and are never shown again; agents reach them through `Notchling --secret` (see `SecretVault`).
struct VaultSheet: View {
    let done: () -> Void
    @State private var entries: [VaultEntry]
    /// The form while adding (nil entry) or editing.
    @State private var form: VaultForm?
    @State private var confirmRemove: VaultEntry?

    /// `entries` and `form` are for probe renders; the app passes only `done`.
    init(entries: [VaultEntry]? = nil, form: VaultForm? = nil, done: @escaping () -> Void) {
        self.done = done
        _entries = State(initialValue: entries ?? SecretVault.all())
        _form = State(initialValue: form)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "key").foregroundColor(KitInk.textMuted)
                Text(form == nil ? "Secret Vault" : form?.original == nil ? "Add Secret" : "Edit Secret")
                    .kitType(.heading).foregroundColor(KitInk.text)
                Spacer()
                if form == nil {
                    if !entries.isEmpty {
                        Button { form = VaultForm(nil) } label: { Label("Add", systemImage: "plus") }
                            .buttonStyle(.kit(.secondary))
                            .part("Add Secret Button", .bottom)
                    }
                    Button("Done", action: done)
                        .buttonStyle(.kit(.secondary))
                        .keyboardShortcut(.cancelAction)
                }
            }
            .padding(.horizontal, 16)
            .frame(height: KitMetric.titleBar + 6)
            .background(KitInk.card)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            Group {
                if let form {
                    VaultFormView(form: form) { saved in
                        self.form = nil
                        if saved { entries = SecretVault.all() }
                    }
                } else if entries.isEmpty {
                    empty
                } else {
                    list
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .frame(width: 520, height: 440)
        .background(KitInk.window)
        .part("Secret Vault Sheet")
        .alert("Remove \(confirmRemove?.name ?? "")?", isPresented: Binding(get: { confirmRemove != nil }, set: { if !$0 { confirmRemove = nil } })) {
            Button("Remove", role: .destructive) {
                if let entry = confirmRemove { try? SecretVault.remove(entry) }
                entries = SecretVault.all()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Its value is deleted from Keychain. Agents that use it will be told it's missing.")
        }
    }

    private var empty: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Save a login or a token once. Agents use it by name, and its value never goes through a chat or into a transcript.")
                .kitType(.body).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
            usage
            Button { form = VaultForm(nil) } label: { Label("Add Secret", systemImage: "plus") }
                .buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
        }
        .padding(20)
    }

    private var usage: some View {
        Text("Agents run `Notchling --secret <name> -- <command>`: the command gets the values as environment variables, and its output comes back with them hidden.")
            .kitType(.meta).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
    }

    private var list: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(entries) { entry in
                    VaultRow(entry: entry,
                             edit: { form = VaultForm(entry) },
                             remove: { confirmRemove = entry })
                }
                usage.padding(.top, 6)
            }
            .padding(16)
        }
    }
}

private struct VaultRow: View {
    let entry: VaultEntry
    let edit: () -> Void
    let remove: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: entry.kind == .login ? "person.badge.key" : "key.horizontal")
                .font(.system(size: 15)).foregroundColor(KitInk.accentText).frame(width: 22, height: 22)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(entry.name).kitType(.heading).foregroundColor(KitInk.text)
                    Text(entry.kind == .login ? "Login" : "Token").kitType(.meta).foregroundColor(KitInk.textMuted)
                }
                if let url = entry.url, !url.isEmpty {
                    Text(url).kitType(.label).foregroundColor(KitInk.accentText).lineLimit(1).truncationMode(.middle)
                        .textSelection(.enabled)
                }
                Text(entry.allVariables.map { "$" + $0 }.joined(separator: "  "))
                    .kitType(.mono).foregroundColor(KitInk.textMuted).textSelection(.enabled)
                if !entry.note.isEmpty {
                    Text(entry.note).kitType(.meta).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 8)
            Button("Edit", action: edit).buttonStyle(.kit(.ghost))
            Button(action: remove) { Image(systemName: "trash") }
                .buttonStyle(.kit(.ghost))
                .help("Remove \(entry.name)")
                .accessibilityLabel("Remove \(entry.name)")
        }
        .padding(12)
        .kitCard(radius: KitMetric.radiusMd)
        .part("Secret Row", .trailing)
    }
}

/// What the form edits. Variable names follow the name until the user changes them.
final class VaultForm: ObservableObject {
    let original: VaultEntry?
    @Published var name: String { didSet { followName() } }
    @Published var kind: VaultEntry.Kind { didSet { followName() } }
    @Published var username = ""
    @Published var secret = ""
    @Published var note: String
    @Published var url: String
    @Published var userVar: String
    @Published var secretVar: String
    @Published var error: String?
    private var customVars: Bool

    init(_ entry: VaultEntry?) {
        original = entry
        name = entry?.name ?? ""
        kind = entry?.kind ?? .token
        note = entry?.note ?? ""
        url = entry?.url ?? ""
        let vars = VaultEntry.defaultVars(name: entry?.name ?? "", kind: entry?.kind ?? .token)
        userVar = entry?.kind == .login ? entry?.userVar ?? vars.user : vars.user
        secretVar = entry?.secretVar ?? vars.secret
        customVars = entry.map { $0.variables != Self.defaults($0.name, $0.kind) } ?? false
        if let entry, let value = SecretVault.value(for: entry) { username = value.username }
    }

    private static func defaults(_ name: String, _ kind: VaultEntry.Kind) -> [String] {
        let v = VaultEntry.defaultVars(name: name, kind: kind)
        return kind == .login ? [v.user, v.secret] : [v.secret]
    }

    func editedVars() { customVars = true }

    private func followName() {
        guard !customVars else { return }
        let v = VaultEntry.defaultVars(name: name, kind: kind)
        if userVar != v.user { userVar = v.user }
        if secretVar != v.secret { secretVar = v.secret }
    }

    func save() -> Bool {
        let name = name.trimmingCharacters(in: .whitespaces)
        let url = url.trimmingCharacters(in: .whitespacesAndNewlines)
        var entry = VaultEntry(name: name, kind: kind, note: note.trimmingCharacters(in: .whitespacesAndNewlines),
                               url: url.isEmpty ? nil : url,
                               userVar: kind == .login ? userVar.trimmingCharacters(in: .whitespaces) : "",
                               secretVar: secretVar.trimmingCharacters(in: .whitespaces))
        entry.updated = Date()
        // Editing with the secret left empty keeps the stored one (and the username, unless it changed).
        var value: VaultValue? = VaultValue(username: kind == .login ? username : "", secret: secret)
        if secret.isEmpty, let original, var stored = SecretVault.value(for: original) {
            stored.username = kind == .login ? username : ""
            value = stored
        }
        do {
            try SecretVault.save(entry, value: value, replacing: original)
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }
}

private struct VaultFormView: View {
    @ObservedObject var form: VaultForm
    let close: (_ saved: Bool) -> Void
    @FocusState private var focus: Field?

    private enum Field { case name, url, username, secret, userVar, secretVar, note }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            row("Name") {
                TextField("github", text: $form.name)
                    .focused($focus, equals: .name)
                    .modifier(KitField(focused: focus == .name))
            }
            row("URL") {
                TextField("https://api.github.com", text: $form.url)
                    .focused($focus, equals: .url)
                    .modifier(KitField(focused: focus == .url))
            }
            row("Kind") {
                Picker("", selection: $form.kind) {
                    Text("Token").tag(VaultEntry.Kind.token)
                    Text("Username & Password").tag(VaultEntry.Kind.login)
                }
                .pickerStyle(.segmented).labelsHidden().frame(width: 260)
            }
            if form.kind == .login {
                row("Username") {
                    TextField("", text: $form.username)
                        .focused($focus, equals: .username)
                        .modifier(KitField(focused: focus == .username))
                }
            }
            row(form.kind == .login ? "Password" : "Token") {
                SecureField(form.original == nil ? "" : "Unchanged", text: $form.secret)
                    .focused($focus, equals: .secret)
                    .modifier(KitField(focused: focus == .secret))
            }
            row("Variables") {
                HStack(spacing: 8) {
                    if form.kind == .login {
                        TextField("", text: Binding(get: { form.userVar }, set: { form.userVar = $0; form.editedVars() }))
                            .focused($focus, equals: .userVar)
                            .modifier(KitField(focused: focus == .userVar))
                    }
                    TextField("", text: Binding(get: { form.secretVar }, set: { form.secretVar = $0; form.editedVars() }))
                        .focused($focus, equals: .secretVar)
                        .modifier(KitField(focused: focus == .secretVar))
                }
                .font(.system(size: 12, design: .monospaced))
            }
            row("Note") {
                TextField("What it's for, e.g. GitHub API for my repos", text: $form.note)
                    .focused($focus, equals: .note)
                    .modifier(KitField(focused: focus == .note))
            }
            Text("Agents see the name, URL, variables and note. The value stays in Keychain.")
                .kitType(.meta).foregroundColor(KitInk.textMuted)
                .padding(.leading, 92)
            if let error = form.error {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundColor(.orange)
                    Text(error).kitType(.label).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
                }
                .padding(.leading, 92)
            }
            Spacer(minLength: 0)
            HStack {
                Spacer()
                Button("Cancel") { close(false) }
                    .buttonStyle(.kit(.secondary)).keyboardShortcut(.cancelAction)
                Button("Save") { if form.save() { close(true) } }
                    .buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
                    .disabled(form.name.isEmpty || (form.original == nil && form.secret.isEmpty))
                    .part("Save Secret Button", .top)
            }
        }
        .padding(20)
        .onAppear { focus = form.original == nil ? .name : .secret }
        .part("Secret Form")
    }

    private func row<Content: View>(_ label: String, @ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 12) {
            Text(label).kitType(.label).foregroundColor(KitInk.textMuted).frame(width: 80, alignment: .trailing)
            content()
        }
    }
}
