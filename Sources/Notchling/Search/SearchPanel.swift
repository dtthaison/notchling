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

/// ⌘⇧P from anywhere: a search box near the top of the screen over saved Artifact pages and
/// conversations (Claude Code, Codex, Antigravity CLI).
final class SearchPanelController: NSObject, NSWindowDelegate {
    private let model = SearchModel()
    private var panel: SearchPanel?
    private var hotKey: GlobalHotKey?
    private var watch: Any?
    /// Opens a saved page (its folder) beside the chat.
    var onOpen: ((String) -> Void)?
    /// Opens a conversation in Chat.
    var onOpenSession: ((SessionMeta) -> Void)?

    /// Registers ⌘⇧P. False when another app already holds it.
    @discardableResult
    func start() -> Bool {
        hotKey = GlobalHotKey(keyCode: kVK_ANSI_P, modifiers: cmdKey | shiftKey, id: 1) { [weak self] in self?.toggle() }
        // Start reading conversations now, so the first search already finds them.
        if hotKey != nil { SessionIndex.shared.refresh() }
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
                                                               copy: { [weak self] in self?.copy($0) }))
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

    private func open(_ item: SearchItem) {
        hide(returning: false)
        switch item {
        case .page(let page): onOpen?(page.folder)
        case .session(let hit): onOpenSession?(hit.meta)
        }
    }

    private func openOnline(_ page: PageLibrary.Entry) {
        hide(returning: false)
        NSWorkspace.shared.open(URL(string: page.url) ?? page.localURL)
    }

    /// A page's claude.ai link, or a session's id.
    private func copy(_ item: SearchItem) {
        let text: String, note: String
        switch item {
        case .page(let page): text = page.url; note = "Link Copied"
        case .session(let hit): text = hit.meta.sessionID; note = "Session ID Copied"
        }
        guard !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        model.flash(note)
    }

    /// Arrows move through the results, Return opens, ⌘Return opens a page on claude.ai, ⌘C copies the
    /// link or session id (unless text is selected in the field), Esc clears, then closes.
    private func handleKey(_ event: NSEvent, in panel: NSPanel) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let command = flags.contains(.command)
        switch event.keyCode {
        case UInt16(kVK_DownArrow): model.move(1); return true
        case UInt16(kVK_UpArrow): model.move(-1); return true
        case UInt16(kVK_Return), UInt16(kVK_ANSI_KeypadEnter):
            guard let item = model.current else { return true }
            if command, case .page(let page) = item, !page.url.isEmpty { openOnline(page) } else { open(item) }
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
            if let editor, editor.selectedRange().length > 0 { editor.copy(nil) } else if let item = model.current { copy(item) }
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
    static let pageRow: CGFloat = 48
    static let sessionRow: CGFloat = 54
    static let header: CGFloat = 26
    static let listPadding: CGFloat = 6
    static let empty: CGFloat = 52
    static let footer: CGFloat = 30
    /// The list scrolls beyond this height.
    static let maxList: CGFloat = 8 * 48
}

/// One row of the results: a saved page or a conversation.
enum SearchItem: Equatable {
    case page(PageLibrary.Entry)
    case session(SessionHit)

    var id: String {
        switch self {
        case .page(let page): return "page:" + page.folder
        case .session(let hit): return "session:" + hit.meta.key
        }
    }

    var rowHeight: CGFloat {
        switch self {
        case .page: return SearchLayout.pageRow
        case .session: return SearchLayout.sessionRow
        }
    }

    var sectionTitle: String {
        switch self {
        case .page: return "Pages"
        case .session: return "Sessions"
        }
    }
}

final class SearchModel: ObservableObject {
    @Published var query = "" { didSet { if query != oldValue { search() } } }
    @Published private(set) var items: [SearchItem] = []
    @Published var selected = 0
    @Published var focusRequest = 0
    /// A short confirmation in the footer, such as "Link Copied".
    @Published private(set) var note: String?
    @Published private(set) var indexing = false
    private var all: [PageLibrary.Entry] = []
    private var pages: [PageLibrary.Entry] = []
    private var sessionHits: [SessionHit] = []
    private var generation = 0
    private var noteTimer: Timer?

    var current: SearchItem? { items.indices.contains(selected) ? items[selected] : nil }

    /// The query's words, to draw them bold in a snippet.
    var words: [String] { SessionSearch.words(query) }

    /// The panel's height for what it shows now.
    var height: CGFloat {
        var h = SearchLayout.field + 1 + SearchLayout.footer
        if items.isEmpty {
            h += SearchLayout.empty + 1
        } else {
            h += listHeight + SearchLayout.listPadding * 2 + 1
        }
        return h
    }

    /// The rows and section headers, up to the height the list scrolls beyond.
    var listHeight: CGFloat {
        var full: CGFloat = 0
        for (index, item) in items.enumerated() {
            full += item.rowHeight + (startsSection(index) ? SearchLayout.header : 0)
        }
        return min(full, SearchLayout.maxList)
    }

    /// Whether the item at `index` is the first of its kind, which gets a header above it.
    func startsSection(_ index: Int) -> Bool {
        guard items.indices.contains(index) else { return false }
        return index == 0 || items[index - 1].sectionTitle != items[index].sectionTitle
    }

    func reload() {
        all = PageLibrary.entries()
        let index = SessionIndex.shared
        index.onChange = { [weak self] in self?.indexChanged() }
        index.refresh()
        indexing = index.isRefreshing
        search()
    }

    private func indexChanged() {
        indexing = SessionIndex.shared.isRefreshing
        // New sessions may match; keep the selection on the same row if it is still there.
        search(keeping: current?.id)
    }

    /// Pages: every word must match (title, project, account or link); a pasted link or id finds its
    /// page; words in the title come first, newest first within each. Sessions are ranked by
    /// `SessionSearch` on a background queue and join the list when they arrive.
    private func search(keeping: String? = nil) {
        let found = PageLibrary.find(query, in: all)
        let words = query.lowercased().split(separator: " ")
        let inTitle = found.filter { page in words.allSatisfy { page.title.lowercased().contains($0) } }
        let rest = found.filter { page in !words.allSatisfy { page.title.lowercased().contains($0) } }
        let empty = words.isEmpty
        pages = Array((inTitle + rest).prefix(empty ? 6 : 20))
        generation += 1
        let mine = generation
        // Show the pages at once, with the sessions found for the last query until the new ones arrive.
        publish(keeping: keeping)
        SessionIndex.shared.search(SessionSearch.words(query), limit: empty ? 6 : 30) { [weak self] hits in
            guard let self, self.generation == mine else { return }
            self.sessionHits = hits
            self.publish(keeping: keeping)
        }
    }

    private func publish(keeping: String?) {
        items = pages.map(SearchItem.page) + sessionHits.map(SearchItem.session)
        if let keeping, let at = items.firstIndex(where: { $0.id == keeping }) { selected = at } else { selected = 0 }
    }

    func move(_ by: Int) {
        guard !items.isEmpty else { return }
        selected = max(0, min(items.count - 1, selected + by))
    }

    func flash(_ text: String) {
        note = text
        noteTimer?.invalidate()
        noteTimer = Timer.scheduledTimer(withTimeInterval: 1.6, repeats: false) { [weak self] _ in self?.note = nil }
    }
}

private struct SearchView: View {
    @ObservedObject var model: SearchModel
    let open: (SearchItem) -> Void
    let copy: (SearchItem) -> Void
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Image(systemName: "magnifyingglass").font(.system(size: 19, weight: .medium))
                    .foregroundColor(KitInk.textMuted)
                TextField("Search Pages and Sessions", text: $model.query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 21))
                    .foregroundColor(KitInk.text)
                    .focused($focused)
                    .kitNoFocusRing()
                    .accessibilityLabel("Search Pages and Sessions")
            }
            .padding(.horizontal, 18)
            .frame(height: SearchLayout.field)
            hairline
            if model.items.isEmpty {
                Text(emptyText)
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

    private var emptyText: String {
        if model.query.isEmpty { return "Nothing Saved Yet" }
        return model.indexing ? "Searching…" : "No Matches"
    }

    private var hairline: some View { Rectangle().fill(KitInk.hairline).frame(height: 1) }

    private var results: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(Array(model.items.enumerated()), id: \.element.id) { index, item in
                        if model.startsSection(index) { SearchSectionHeader(title: item.sectionTitle) }
                        row(item, selected: index == model.selected)
                            .id(item.id)
                            .contentShape(Rectangle())
                            .onHover { if $0 { model.selected = index } }
                            .onTapGesture { open(item) }
                            .pointerCursor()
                    }
                }
                .padding(SearchLayout.listPadding)
            }
            .frame(height: model.listHeight + SearchLayout.listPadding * 2)
            .onChange(of: model.selected) { _ in
                if let item = model.current { proxy.scrollTo(item.id) }
            }
        }
        .part("Search Results")
    }

    @ViewBuilder private func row(_ item: SearchItem, selected: Bool) -> some View {
        switch item {
        case .page(let page):
            SearchRow(page: page, selected: selected, copy: { copy(item) })
        case .session(let hit):
            SessionRow(hit: hit, words: model.words, selected: selected)
        }
    }

    private var footer: some View {
        HStack(spacing: 14) {
            if let note = model.note {
                Label(note, systemImage: "checkmark").foregroundColor(KitInk.accentText)
            } else if model.indexing {
                Text("Indexing sessions…").foregroundColor(KitInk.textFaint)
            } else {
                Text("Pages and Sessions").foregroundColor(KitInk.textFaint)
            }
            Spacer()
            switch model.current {
            case .page(let page)?:
                hint("↩", "Open")
                if !page.url.isEmpty {
                    hint("⌘↩", "Open on claude.ai")
                    hint("⌘C", "Copy Link")
                }
            case .session?:
                hint("↩", "Open in Chat")
                hint("⌘C", "Copy Session ID")
            case nil:
                EmptyView()
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

private struct SearchSectionHeader: View {
    let title: String

    var body: some View {
        Text(title)
            .kitType(.section).foregroundColor(KitInk.textFaint)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 10)
            .padding(.bottom, 4)
            .frame(height: SearchLayout.header, alignment: .bottom)
            .part("Search Section")
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
        .frame(height: SearchLayout.pageRow)
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            .fill(selected ? KitInk.selection : .clear))
        .part("Page Result")
    }

    private var subtitle: String {
        [page.project, page.account].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// A conversation: the tool's icon, its title, the folder and how long ago, and the line around the match.
private struct SessionRow: View {
    let hit: SessionHit
    let words: [String]
    let selected: Bool

    var body: some View {
        HStack(spacing: 12) {
            ToolIcon(tool: hit.meta.tool, size: 20)
                .frame(width: 30, height: 30)
                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                    .fill(selected ? KitInk.accentWash : KitInk.raised))
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 8) {
                    Text(hit.meta.title).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                    Spacer(minLength: 8)
                    Text(place).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1).truncationMode(.head)
                        .layoutPriority(-1)
                    Text(SessionAge.text(from: hit.meta.date)).kitType(.meta).foregroundColor(KitInk.textFaint)
                }
                detail.kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: SearchLayout.sessionRow)
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            .fill(selected ? KitInk.selection : .clear))
        .part("Session Result")
    }

    private var place: String { (hit.meta.folder as NSString).lastPathComponent }

    /// The snippet with the query's words in bold; else the folder's path.
    private var detail: Text {
        guard let snippet = hit.snippet else {
            return Text((hit.meta.folder as NSString).abbreviatingWithTildeInPath)
        }
        var text = AttributedString(snippet)
        for range in SessionSearch.highlights(in: snippet, words: words) {
            if let r = Range(range, in: text) { text[r].inlinePresentationIntent = .stronglyEmphasized }
        }
        return Text(text)
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
