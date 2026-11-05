import AppKit
import Carbon
import SwiftUI

/// A system-wide shortcut through Carbon's hot keys, which need no Accessibility permission and
/// don't see any other key press.
final class GlobalHotKey {
    private var ref: EventHotKeyRef?
    private let id: UInt32
    private static var handlers: [UInt32: () -> Void] = [:]
    private static var installed = false

    /// Nil when another app already holds the shortcut.
    init?(keyCode: Int, modifiers: Int, id: UInt32, handler: @escaping () -> Void) {
        self.id = id
        Self.installHandler()
        let hotID = EventHotKeyID(signature: OSType(0x4E54_434C), id: id) // "NTCL"
        guard RegisterEventHotKey(UInt32(keyCode), UInt32(modifiers), hotID, GetApplicationEventTarget(), 0, &ref) == noErr else {
            return nil
        }
        Self.handlers[id] = handler
    }

    deinit {
        if let ref { UnregisterEventHotKey(ref) }
        Self.handlers[id] = nil
    }

    private static func installHandler() {
        guard !installed else { return }
        installed = true
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, event, _ in
            var hotID = EventHotKeyID()
            GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
                              nil, MemoryLayout<EventHotKeyID>.size, nil, &hotID)
            DispatchQueue.main.async { GlobalHotKey.handlers[hotID.id]?() }
            return noErr
        }, 1, &spec, nil, nil)
    }
}

/// Borderless and non-activating, like Spotlight: it takes the keyboard without bringing Notchling
/// forward (or giving it a Dock icon), and goes away when you click elsewhere.
final class SearchPanel: NSPanel {
    var keyHandler: ((NSEvent) -> Bool)?

    init() {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel, .fullSizeContentView],
                   backing: .buffered, defer: false)
        isFloatingPanel = true
        level = .modalPanel
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        backgroundColor = .clear
        isOpaque = false
        hasShadow = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    override func sendEvent(_ event: NSEvent) {
        if event.type == .keyDown, keyHandler?(event) == true { return }
        super.sendEvent(event)
    }
}

/// ⌘⇧P from anywhere: a search box near the top of the screen. For now it finds saved Artifacts.
final class SearchPanelController: NSObject, NSWindowDelegate {
    private let model = SearchModel()
    private var panel: SearchPanel?
    private var hotKey: GlobalHotKey?
    private var watch: Any?
    /// Opens a saved page (its folder) beside the chat.
    var onOpen: ((String) -> Void)?

    /// Registers ⌘⇧P. False when another app already holds it.
    @discardableResult
    func start() -> Bool {
        hotKey = GlobalHotKey(keyCode: kVK_ANSI_P, modifiers: cmdKey | shiftKey, id: 1) { [weak self] in self?.toggle() }
        return hotKey != nil
    }

    func toggle() {
        if let panel, panel.isVisible, panel.isKeyWindow { hide() } else { show() }
    }

    func show() {
        model.reload()
        let panel = self.panel ?? makePanel()
        self.panel = panel
        // Like Spotlight: about a quarter of the way down the screen the mouse is on.
        let screen = NSScreen.screens.first { NSMouseInRect(NSEvent.mouseLocation, $0.frame, false) } ?? NSScreen.main
        if let frame = screen?.frame {
            top = NSPoint(x: frame.midX, y: frame.maxY - frame.height * 0.22)
        }
        resize()
        // A non-activating panel only gets the keyboard while its app is active, so activate, and
        // give the app you were in its focus back when the panel closes without opening anything.
        let front = NSWorkspace.shared.frontmostApplication
        if front?.processIdentifier != ProcessInfo.processInfo.processIdentifier { previous = front }
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
        // The field takes focus with its text selected, so typing replaces the last search.
        DispatchQueue.main.async { self.model.focusRequest += 1 }
    }

    /// Closes the panel. `returning` gives focus back to the app you were in (not when a page opens).
    func hide(returning: Bool = true) {
        guard let panel, panel.isVisible else { return }
        panel.orderOut(nil)
        if returning { previous?.activate() }
        previous = nil
    }

    private var top = NSPoint.zero
    private var previous: NSRunningApplication?

    private func makePanel() -> SearchPanel {
        let panel = SearchPanel()
        panel.delegate = self
        panel.contentView = NSHostingView(rootView: SearchView(model: model, open: { [weak self] in self?.open($0) },
                                                               copy: { [weak self] in self?.copyLink($0) }))
        panel.keyHandler = { [weak self] event in self?.handleKey(event, in: panel) ?? false }
        watch = model.objectWillChange.sink { [weak self] _ in DispatchQueue.main.async { self?.resize() } }
        return panel
    }

    private func resize() {
        guard let panel else { return }
        let size = NSSize(width: SearchLayout.width, height: model.height)
        let frame = NSRect(x: top.x - size.width / 2, y: top.y - size.height, width: size.width, height: size.height)
        guard panel.frame != frame else { return }
        panel.setFrame(frame, display: true)
        panel.invalidateShadow()
    }

    /// Clicked elsewhere: that click already chose where focus goes.
    func windowDidResignKey(_ notification: Notification) {
        hide(returning: false)
    }

    // MARK: Actions

    private func open(_ page: PageLibrary.Entry) {
        hide(returning: false)
        onOpen?(page.folder)
    }

    private func openOnline(_ page: PageLibrary.Entry) {
        hide(returning: false)
        NSWorkspace.shared.open(URL(string: page.url) ?? page.localURL)
    }

    private func copyLink(_ page: PageLibrary.Entry) {
        guard !page.url.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(page.url, forType: .string)
        model.flash("Link Copied")
    }

    /// Arrows move through the results, Return opens, ⌘Return opens on claude.ai, ⌘C copies the link
    /// (unless text is selected in the field), Esc clears, then closes.
    private func handleKey(_ event: NSEvent, in panel: NSPanel) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let command = flags.contains(.command)
        switch event.keyCode {
        case UInt16(kVK_DownArrow): model.move(1); return true
        case UInt16(kVK_UpArrow): model.move(-1); return true
        case UInt16(kVK_Return), UInt16(kVK_ANSI_KeypadEnter):
            guard let page = model.current else { return true }
            if command, !page.url.isEmpty { openOnline(page) } else { open(page) }
            return true
        case UInt16(kVK_Escape):
            if model.query.isEmpty { hide() } else { model.query = "" }
            return true
        default: break
        }
        guard command, !flags.contains(.option), !flags.contains(.control) else { return false }
        let editor = panel.firstResponder as? NSTextView
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "c":
            if let editor, editor.selectedRange().length > 0 { editor.copy(nil) } else if let page = model.current { copyLink(page) }
            return true
        case "v": editor?.paste(nil); return true
        case "x": editor?.cut(nil); return true
        case "a": editor?.selectAll(nil); return true
        case "w": hide(); return true
        default: return false
        }
    }
}

enum SearchLayout {
    static let width: CGFloat = 680
    static let field: CGFloat = 56
    static let row: CGFloat = 48
    static let listPadding: CGFloat = 6
    static let empty: CGFloat = 52
    static let footer: CGFloat = 30
    static let visibleRows = 8
}

final class SearchModel: ObservableObject {
    @Published var query = "" { didSet { if query != oldValue { search() } } }
    @Published private(set) var results: [PageLibrary.Entry] = []
    @Published var selected = 0
    @Published var focusRequest = 0
    /// A short confirmation in the footer, such as "Link Copied".
    @Published private(set) var note: String?
    private var all: [PageLibrary.Entry] = []
    private var noteTimer: Timer?

    var current: PageLibrary.Entry? { results.indices.contains(selected) ? results[selected] : nil }

    /// The panel's height for what it shows now.
    var height: CGFloat {
        var h = SearchLayout.field + 1 + SearchLayout.footer
        if !results.isEmpty {
            h += CGFloat(min(results.count, SearchLayout.visibleRows)) * SearchLayout.row + SearchLayout.listPadding * 2 + 1
        } else {
            h += SearchLayout.empty + 1
        }
        return h
    }

    func reload() {
        all = PageLibrary.entries()
        search()
    }

    /// Every word must match (title, project, account or link); a pasted link or id finds its page.
    /// Pages with the words in their title come first, newest first within each.
    private func search() {
        let found = PageLibrary.find(query, in: all)
        let words = query.lowercased().split(separator: " ")
        let inTitle = found.filter { page in words.allSatisfy { page.title.lowercased().contains($0) } }
        let rest = found.filter { page in !words.allSatisfy { page.title.lowercased().contains($0) } }
        results = Array((inTitle + rest).prefix(50))
        selected = 0
    }

    func move(_ by: Int) {
        guard !results.isEmpty else { return }
        selected = max(0, min(results.count - 1, selected + by))
    }

    func flash(_ text: String) {
        note = text
        noteTimer?.invalidate()
        noteTimer = Timer.scheduledTimer(withTimeInterval: 1.6, repeats: false) { [weak self] _ in self?.note = nil }
    }
}

private struct SearchView: View {
    @ObservedObject var model: SearchModel
    let open: (PageLibrary.Entry) -> Void
    let copy: (PageLibrary.Entry) -> Void
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Image(systemName: "magnifyingglass").font(.system(size: 19, weight: .medium))
                    .foregroundColor(KitInk.textMuted)
                TextField("Search Artifacts", text: $model.query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 21))
                    .foregroundColor(KitInk.text)
                    .focused($focused)
                    .kitNoFocusRing()
                    .accessibilityLabel("Search Artifacts")
            }
            .padding(.horizontal, 18)
            .frame(height: SearchLayout.field)
            hairline
            if model.results.isEmpty {
                Text(model.query.isEmpty ? "No Artifacts Saved Yet" : "No Matching Artifacts")
                    .kitType(.body).foregroundColor(KitInk.textMuted)
                    .frame(maxWidth: .infinity).frame(height: SearchLayout.empty)
            } else {
                results
            }
            hairline
            footer
        }
        .frame(width: SearchLayout.width, height: model.height, alignment: .top)
        .background(PanelBackground())
        .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusCard, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusCard, style: .continuous)
            .strokeBorder(KitInk.hairline.opacity(0.8), lineWidth: 1))
        .onChange(of: model.focusRequest) { _ in focused = true }
    }

    private var hairline: some View { Rectangle().fill(KitInk.hairline).frame(height: 1) }

    private var results: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(Array(model.results.enumerated()), id: \.element.folder) { index, page in
                        SearchRow(page: page, selected: index == model.selected, copy: { copy(page) })
                            .id(page.folder)
                            .contentShape(Rectangle())
                            .onHover { if $0 { model.selected = index } }
                            .onTapGesture { open(page) }
                            .pointerCursor()
                    }
                }
                .padding(SearchLayout.listPadding)
            }
            .frame(height: CGFloat(min(model.results.count, SearchLayout.visibleRows)) * SearchLayout.row
                   + SearchLayout.listPadding * 2)
            .onChange(of: model.selected) { _ in
                if let page = model.current { proxy.scrollTo(page.folder) }
            }
        }
    }

    private var footer: some View {
        HStack(spacing: 14) {
            if let note = model.note {
                Label(note, systemImage: "checkmark").foregroundColor(KitInk.accentText)
            } else {
                Text("Artifacts").foregroundColor(KitInk.textFaint)
            }
            Spacer()
            if let page = model.current {
                hint("↩", "Open")
                if !page.url.isEmpty {
                    hint("⌘↩", "Open on claude.ai")
                    hint("⌘C", "Copy Link")
                }
            }
            hint("esc", "Close")
        }
        .kitType(.meta)
        .padding(.horizontal, 16)
        .frame(height: SearchLayout.footer)
    }

    private func hint(_ key: String, _ title: String) -> some View {
        HStack(spacing: 5) {
            Text(key).foregroundColor(KitInk.textMuted)
            Text(title).foregroundColor(KitInk.textFaint)
        }
    }
}

private struct SearchRow: View {
    let page: PageLibrary.Entry
    let selected: Bool
    let copy: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "doc.richtext")
                .font(.system(size: 14, weight: .medium))
                .foregroundColor(selected ? KitInk.accentText : KitInk.textMuted)
                .frame(width: 30, height: 30)
                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                    .fill(selected ? KitInk.accentWash : KitInk.raised))
            VStack(alignment: .leading, spacing: 2) {
                Text(page.title).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                Text(subtitle).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
            }
            Spacer(minLength: 8)
            if selected, !page.url.isEmpty {
                Button(action: copy) {
                    Label("Copy Link", systemImage: "link")
                }
                .buttonStyle(.kit(.secondary))
                .help("Copy the claude.ai link (⌘C)")
                .pointerCursor()
            }
            Text(PageDates.short(page.updated)).kitType(.meta).foregroundColor(KitInk.textFaint)
        }
        .padding(.horizontal, 10)
        .frame(height: SearchLayout.row)
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            .fill(selected ? KitInk.selection : .clear))
    }

    private var subtitle: String {
        [page.project, page.account].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// The blurred backdrop Spotlight has, which SwiftUI's materials don't give a borderless panel.
private struct PanelBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .popover
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}
