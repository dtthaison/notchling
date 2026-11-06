import AppKit
import SwiftUI

/// The Secret Vault: the saved credentials, and a form to add, edit or remove one. Values go straight to
/// Keychain (`VaultStore`) and are never shown again; agents reach them through `Notchling --secret`.
/// Chat shows it as a sheet; `VaultPrompt` shows the form alone in a panel when an agent asks for a secret
/// (`finish` set: saving or cancelling ends it instead of going back to the list).
struct VaultSheet: View {
    let done: () -> Void
    var finish: ((VaultEntry?) -> Void)?
    @State private var entries: [VaultEntry]
    /// The form while adding (nil entry) or editing.
    @State private var form: VaultForm?
    @State private var confirmRemove: VaultEntry?
    @State private var removeError: String?

    /// `entries` are for probe renders; the app passes `form` only for an agent's request.
    init(entries: [VaultEntry]? = nil, form: VaultForm? = nil, finish: ((VaultEntry?) -> Void)? = nil, done: @escaping () -> Void) {
        self.done = done
        self.finish = finish
        _entries = State(initialValue: entries ?? SecretVault.all())
        _form = State(initialValue: form)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "key").foregroundColor(KitInk.textMuted)
                Text(form == nil ? "Secret Vault" : form?.renewing == true ? "Renew Secret" : form?.original == nil ? "Add Secret" : "Edit Secret")
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
                        if let finish { finish(saved ? form.saved : nil); return }
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
        .frame(width: 540, height: form == nil ? 520 : 640)
        .background(KitInk.window)
        .part("Secret Vault Sheet")
        .alert("Remove \(confirmRemove?.name ?? "")?", isPresented: Binding(get: { confirmRemove != nil }, set: { if !$0 { confirmRemove = nil } })) {
            Button("Remove", role: .destructive) {
                if let entry = confirmRemove {
                    do { try VaultStore.shared.remove(entry) } catch { removeError = error.localizedDescription }
                }
                entries = SecretVault.all()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Its value is deleted from Keychain. Agents that use it will be told it's missing.")
        }
        .alert("Couldn't Remove It", isPresented: Binding(get: { removeError != nil }, set: { if !$0 { removeError = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(removeError ?? "")
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
        Text("Only Notchling can read the values. Agents run `Notchling --secret <name> -- <command>`: the command gets them as environment variables, and its output comes back with them hidden. An agent that needs one you haven't saved opens this form for you.")
            .kitType(.meta).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
    }

    private var list: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(entries) { entry in
                    VaultRow(entry: entry,
                             edit: { form = VaultForm(entry) },
                             renew: { VaultRenew.openPage(for: entry); form = VaultForm(entry, renew: true) },
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
    let renew: () -> Void
    let remove: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: entry.kind == .login ? "person.badge.key" : "key.horizontal")
                .font(.system(size: 15)).foregroundColor(KitInk.accentText).frame(width: 22, height: 22)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(entry.name).kitType(.heading).foregroundColor(KitInk.text)
                    Text(entry.kind == .login ? "Login" : "Token").kitType(.meta).foregroundColor(KitInk.textMuted)
                    if entry.asks {
                        Label("Asks First", systemImage: "hand.raised").kitType(.meta).foregroundColor(KitInk.textMuted)
                    }
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
                if let day = entry.expires { VaultExpiryLabel(day: day) }
            }
            Spacer(minLength: 8)
            if entry.expires != nil || !(entry.howTo ?? "").isEmpty {
                Button("Renew…", action: renew).buttonStyle(.kit(.ghost))
                    .help("Open the page for a new value, then enter it here")
            }
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
    /// Why an agent asked for this secret, shown above the form.
    let reason: String?
    /// Opened by Renew…: the value field is empty and focused, the how-to sits above it and the expiry is asked for.
    let renewing: Bool
    @Published var name: String { didSet { followName() } }
    @Published var kind: VaultEntry.Kind { didSet { followName() } }
    @Published var username = ""
    @Published var secret = ""
    @Published var note: String
    @Published var url: String
    /// How to get or renew the secret; shown under the value field and kept with the entry.
    @Published var howTo: String
    /// The optional last day: off means "No expiry".
    @Published var hasExpiry: Bool
    @Published var expiryDate: Date
    @Published var userVar: String
    @Published var secretVar: String
    @Published var ask: Bool
    @Published var error: String?
    /// The entry as saved, for an agent's request.
    private(set) var saved: VaultEntry?
    /// The variable names were changed from the ones made from the name; the form then opens Advanced.
    private(set) var customVars: Bool

    init(_ entry: VaultEntry?, reason: String? = nil, renew: Bool = false) {
        original = entry
        self.reason = reason
        renewing = renew && entry != nil
        name = entry?.name ?? ""
        kind = entry?.kind ?? .token
        note = entry?.note ?? ""
        url = entry?.url ?? ""
        howTo = entry?.howTo ?? ""
        let day = entry?.expires.flatMap { VaultExpiry.date(from: $0) }
        // Editing keeps the saved day (an expired one shows in red). A renewal asks for the new day: the
        // picker is shown, on the saved day while that is still ahead, else 30 days out.
        let ahead = day.flatMap { $0 >= Calendar.current.startOfDay(for: Date()) ? $0 : nil }
        hasExpiry = day != nil || renew && entry != nil
        expiryDate = (renew && entry != nil ? ahead : day) ?? Calendar.current.date(byAdding: .day, value: 30, to: Date()) ?? Date()
        ask = entry?.asks ?? false
        let vars = VaultEntry.defaultVars(name: entry?.name ?? "", kind: entry?.kind ?? .token)
        userVar = entry?.kind == .login ? entry?.userVar ?? vars.user : vars.user
        secretVar = entry?.secretVar ?? vars.secret
        customVars = entry.map { $0.variables != Self.defaults($0.name, $0.kind) } ?? false
        if let entry, entry.kind == .login, let value = try? VaultStore.shared.value(for: entry) { username = value.username }
    }

    /// An agent's request: the saved secret by that name if there is one (to update), else a new one
    /// filled in with what the agent said. For a saved one, each of note, URL, how-to and expiry the agent
    /// passed replaces the saved text in the form (`VaultEntry.prefilled`); nothing is saved until the user
    /// presses Save.
    convenience init(request: VaultRequest) {
        let existing = request.name.flatMap(SecretVault.entry)
        self.init(existing, reason: request.reason)
        if existing == nil {
            name = request.name ?? ""
            if let kind = request.kind.flatMap(VaultEntry.Kind.init(rawValue:)) { self.kind = kind }
        }
        let merged = (existing ?? VaultEntry(name: request.name ?? "", kind: .token)).prefilled(by: request)
        note = merged.note
        url = merged.url ?? ""
        howTo = merged.howTo ?? ""
        if let day = merged.expires.flatMap({ VaultExpiry.date(from: $0) }) {
            hasExpiry = true
            expiryDate = day
        }
    }

    /// The chosen day, or nil for "No expiry".
    var expiresDay: String? { hasExpiry ? VaultExpiry.day(of: expiryDate) : nil }

    private static func defaults(_ name: String, _ kind: VaultEntry.Kind) -> [String] {
        let v = VaultEntry.defaultVars(name: name, kind: kind)
        return kind == .login ? [v.user, v.secret] : [v.secret]
    }

    func editedVars() { customVars = true }

    /// `$NPM_TOKEN $NPM_URL`, shown next to the closed Advanced toggle.
    var variablesSummary: String {
        let vars = (kind == .login ? [userVar, secretVar] : [secretVar])
            + (url.trimmingCharacters(in: .whitespaces).isEmpty ? [] : ["\(VaultEntry.prefix(for: name))_URL"])
        return vars.filter { !$0.isEmpty }.map { "$" + $0 }.joined(separator: " ")
    }

    private func followName() {
        guard !customVars else { return }
        let v = VaultEntry.defaultVars(name: name, kind: kind)
        if userVar != v.user { userVar = v.user }
        if secretVar != v.secret { secretVar = v.secret }
    }

    func save() -> Bool {
        let name = name.trimmingCharacters(in: .whitespaces)
        let url = url.trimmingCharacters(in: .whitespacesAndNewlines)
        let howTo = howTo.trimmingCharacters(in: .whitespacesAndNewlines)
        var entry = VaultEntry(name: name, kind: kind, note: note.trimmingCharacters(in: .whitespacesAndNewlines),
                               url: url.isEmpty ? nil : url,
                               howTo: howTo.isEmpty ? nil : howTo,
                               expires: expiresDay,
                               userVar: kind == .login ? userVar.trimmingCharacters(in: .whitespaces) : "",
                               secretVar: secretVar.trimmingCharacters(in: .whitespaces),
                               ask: ask)
        entry.updated = Date()
        do {
            // Editing with the secret left empty keeps the stored one (and the username, unless it changed).
            var value: VaultValue? = VaultValue(username: kind == .login ? username : "", secret: secret)
            if secret.isEmpty, let original, var stored = try VaultStore.shared.value(for: original) {
                stored.username = kind == .login ? username : ""
                value = stored
            }
            try VaultStore.shared.save(entry, value: value, replacing: original)
            saved = entry
            // A new value starts the reminders over.
            if !secret.isEmpty, let original { VaultReminders.clearSent(ids: [original.id, entry.id]) }
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
    /// Variable names are made from the name; most people never change them, so they sit behind this.
    @State private var advanced = false

    private enum Field { case name, url, username, secret, userVar, secretVar }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ScrollView { fields.padding(.vertical, 2).padding(.trailing, 4) }
            footer
        }
        .padding(20)
        .onAppear { advanced = form.customVars; focus = form.original == nil && form.name.isEmpty ? .name : (form.kind == .login && form.username.isEmpty && !form.renewing ? .username : .secret) }
        .part("Secret Form")
    }

    private var hasHowTo: Bool { !form.howTo.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private var fields: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let reason = form.reason, !reason.isEmpty {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "sparkles").foregroundColor(KitInk.accentText)
                    Text("An agent asks: \(reason)").kitType(.label).foregroundColor(KitInk.text)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.accentWash))
                .part("Agent Reason")
            }
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
            if form.renewing, hasHowTo {
                VaultHelpView(text: form.howTo).padding(.leading, 92)
            }
            row(form.kind == .login ? "Password" : "Token") {
                SecureField(form.original == nil ? "" : form.renewing ? "New value" : "Unchanged", text: $form.secret)
                    .focused($focus, equals: .secret)
                    .modifier(KitField(focused: focus == .secret))
            }
            if !form.renewing, hasHowTo {
                VaultHelpView(text: form.howTo).padding(.leading, 92)
            }
            row("Note", top: true) {
                VaultTextArea(text: $form.note, placeholder: "What it's for, e.g. GitHub API for my repos")
                    .part("Note Field")
            }
            row("How to get or renew", top: true) {
                VaultTextArea(text: $form.howTo, placeholder: "Where to click and which scopes, e.g. GitHub > Settings > Developer settings > Tokens. Links work.")
                    .part("How To Field")
            }
            row(form.renewing ? "New expiry" : "Expires") {
                HStack(spacing: 10) {
                    Toggle("No expiry", isOn: Binding(get: { !form.hasExpiry }, set: { form.hasExpiry = !$0 }))
                        .toggleStyle(.checkbox).kitType(.label).foregroundColor(KitInk.text)
                    if form.hasExpiry {
                        DatePicker("", selection: $form.expiryDate, displayedComponents: .date)
                            .datePickerStyle(.field).labelsHidden().fixedSize()
                        if let day = form.expiresDay { VaultExpiryLabel(day: day) }
                    }
                    Spacer(minLength: 0)
                }
                .part("Expires Field")
            }
            row("") {
                Toggle("Ask me before an agent uses it", isOn: $form.ask)
                    .toggleStyle(.checkbox).kitType(.label).foregroundColor(KitInk.text)
                    .part("Ask First Toggle")
            }
            row("") {
                Button { withAnimation(KitMotion.appear) { advanced.toggle() } } label: {
                    HStack(spacing: 4) {
                        Image(systemName: advanced ? "chevron.down" : "chevron.right").font(.system(size: 9, weight: .semibold))
                        Text("Advanced").kitType(.label)
                        if !advanced {
                            Text(form.variablesSummary).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1)
                        }
                    }
                    .foregroundColor(KitInk.textMuted)
                }
                .buttonStyle(.kitPlain)
                .help("The environment variable names a command gets. Made from the name; change them only when a tool expects another name.")
                .part("Advanced Toggle")
            }
            if advanced {
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
                }
            }
            Text("Agents see the name, URL, variables, note, how-to and expiry. Only Notchling can read the value.")
                .kitType(.meta).foregroundColor(KitInk.textMuted)
                .padding(.leading, 92)
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let error = form.error {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundColor(.orange)
                    Text(error).kitType(.label).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
                }
                .padding(.leading, 92)
            }
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
    }

    private func row<Content: View>(_ label: String, top: Bool = false, @ViewBuilder _ content: () -> Content) -> some View {
        HStack(alignment: top ? .firstTextBaseline : .center, spacing: 12) {
            Text(label).kitType(.label).foregroundColor(KitInk.textMuted).multilineTextAlignment(.trailing)
                .frame(width: 80, alignment: .trailing)
            content()
        }
    }
}

/// "Expired" in red once the day has passed, else "in 30 days".
struct VaultExpiryLabel: View {
    let day: String
    var now: Date = Date()

    var body: some View {
        if VaultExpiry.status(of: day, now: now) == .expired {
            Label("Expired \(day)", systemImage: "exclamationmark.circle.fill")
                .kitType(.label).foregroundColor(KitInk.expired)
        } else if let phrase = VaultExpiry.phrase(of: day, now: now) {
            Text("Expires \(day) (\(phrase))").kitType(.meta).foregroundColor(KitInk.textMuted)
        } else {
            Text("Expires \(day)").kitType(.meta).foregroundColor(KitInk.textMuted)
        }
    }
}

/// The how-to under the value field: selectable, with Markdown links and bare URLs clickable.
struct VaultHelpView: View {
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "questionmark.circle").foregroundColor(KitInk.accentText).padding(.top, 1)
            VStack(alignment: .leading, spacing: 3) {
                Text("How to get or renew").kitType(.section).foregroundColor(KitInk.textFaint)
                Text(VaultHelp.attributed(text))
                    .font(.system(size: 12)).foregroundColor(KitInk.text)
                    .tint(KitInk.accentText)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.accentWash))
        .part("How To Help")
    }
}

/// Help text as the form shows it.
enum VaultHelp {
    /// Inline Markdown (links, bold, code) with the line breaks kept, plus bare `https://…` links made clickable.
    static func attributed(_ text: String) -> AttributedString {
        var out = (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
        let plain = String(out.characters)
        let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue)
        for match in detector?.matches(in: plain, range: NSRange(plain.startIndex..., in: plain)) ?? [] {
            guard let url = match.url, let r = Range(match.range, in: plain),
                  let lower = AttributedString.Index(r.lowerBound, within: out),
                  let upper = AttributedString.Index(r.upperBound, within: out) else { continue }
            let range = lower..<upper
            if out[range].runs.allSatisfy({ $0.link == nil }) { out[range].link = url }
        }
        return out
    }
}

/// A multi-line text field: `minLines` tall, growing with its text up to `maxLines`, then scrolling.
/// Tab moves on to the next field; Return starts a new line.
struct VaultTextArea: View {
    @Binding var text: String
    var placeholder = ""
    var minLines = 3
    var maxLines = 6
    @State private var contentHeight: CGFloat = 0
    @State private var focused = false

    private static let font = NSFont.systemFont(ofSize: 13)
    private static let inset = CGSize(width: 8, height: 6)

    private var lineHeight: CGFloat {
        let manager = NSLayoutManager()
        return ceil(manager.defaultLineHeight(for: Self.font))
    }

    var body: some View {
        let area = VaultTextView(text: $text, font: Self.font, inset: Self.inset,
                                 onHeight: { contentHeight = $0 }, onFocus: { focused = $0 })
        let height = min(max(contentHeight, lineHeight * CGFloat(minLines)), lineHeight * CGFloat(maxLines)) + Self.inset.height * 2
        area
            .frame(height: height)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.card))
            .overlay(alignment: .topLeading) {
                if text.isEmpty {
                    Text(placeholder).kitType(.body).foregroundColor(KitInk.textFaint)
                        .padding(.horizontal, Self.inset.width + 5).padding(.vertical, Self.inset.height)
                        .allowsHitTesting(false)
                }
            }
            .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                .strokeBorder(focused ? KitInk.accent : KitInk.hairline, lineWidth: focused ? 1.5 : 1))
            .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous))
    }
}

private struct VaultTextView: NSViewRepresentable {
    @Binding var text: String
    let font: NSFont
    let inset: CGSize
    let onHeight: (CGFloat) -> Void
    let onFocus: (Bool) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let view = FocusTextView()
        view.delegate = context.coordinator
        view.onFocus = { context.coordinator.parent.onFocus($0) }
        view.font = font
        view.textColor = NSColor(KitInk.text)
        view.insertionPointColor = NSColor(KitInk.accent)
        view.drawsBackground = false
        view.isRichText = false
        view.allowsUndo = true
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticTextReplacementEnabled = false
        view.textContainerInset = inset
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.autoresizingMask = [.width]
        view.textContainer?.widthTracksTextView = true
        view.string = text
        view.postsFrameChangedNotifications = true
        NotificationCenter.default.addObserver(context.coordinator, selector: #selector(Coordinator.measure),
                                               name: NSView.frameDidChangeNotification, object: view)
        let scroll = NSScrollView()
        scroll.documentView = view
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        context.coordinator.textView = view
        DispatchQueue.main.async { context.coordinator.measure() }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let view = scroll.documentView as? NSTextView, view.string != text else { return }
        view.string = text
        DispatchQueue.main.async { context.coordinator.measure() }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: VaultTextView
        weak var textView: NSTextView?

        init(_ parent: VaultTextView) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            parent.text = view.string
            measure()
        }

        /// Tab leaves the field instead of typing a tab.
        func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.insertTab(_:)): textView.window?.selectNextKeyView(nil); return true
            case #selector(NSResponder.insertBacktab(_:)): textView.window?.selectPreviousKeyView(nil); return true
            default: return false
            }
        }

        @objc func measure() {
            guard let view = textView, let container = view.textContainer, let manager = view.layoutManager else { return }
            manager.ensureLayout(for: container)
            let height = ceil(manager.usedRect(for: container).height)
            DispatchQueue.main.async { [parent] in parent.onHeight(height) }
        }
    }
}

private final class FocusTextView: NSTextView {
    var onFocus: ((Bool) -> Void)?

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFocus?(true) }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { onFocus?(false) }
        return ok
    }
}
