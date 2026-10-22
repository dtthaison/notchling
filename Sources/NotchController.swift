import AppKit
import SwiftUI

final class NotchPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    /// Gets first look at key presses (Tab, arrows, Return) while typing a reply.
    var keyHandler: ((NSEvent) -> Bool)?

    override func sendEvent(_ event: NSEvent) {
        if event.type == .keyDown {
            if event.modifierFlags.contains(.command), editCommand(event) { return }
            if !event.modifierFlags.contains(.command), keyHandler?(event) == true { return }
        }
        super.sendEvent(event)
    }

    /// The app has no Edit menu, so ⌘C, ⌘V, ⌘X, ⌘A and ⌘Z are routed to the text field here.
    private func editCommand(_ event: NSEvent) -> Bool {
        let shift = event.modifierFlags.contains(.shift)
        let action: Selector
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "c": action = #selector(NSText.copy(_:))
        case "v": action = #selector(NSText.paste(_:))
        case "x": action = #selector(NSText.cut(_:))
        case "a": action = #selector(NSText.selectAll(_:))
        case "z": action = shift ? Selector(("redo:")) : Selector(("undo:"))
        default: return false
        }
        return NSApp.sendAction(action, to: nil, from: self)
    }
}

/// Owns the borderless panel that hangs from the camera notch. The panel is a fixed, transparent
/// canvas that never resizes; only the black shape inside animates, so it always grows out of the
/// top edge. Clicks outside the shape pass straight through to whatever is underneath.
final class NotchController: NSObject, NSWindowDelegate {
    let model = NotchModel()
    private let panel: NotchPanel
    private var screen: NSScreen?
    /// Horizontal centre of the camera housing in screen coordinates.
    private var centerX: CGFloat = 0
    private var monitors: [Any] = []
    private var mouseInside = false

    /// Big enough for the largest state; anything outside the black shape is see-through.
    static let canvas = CGSize(width: 560, height: 640)

    override init() {
        panel = NotchPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel],
                           backing: .buffered, defer: false)
        super.init()
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.level = .statusBar
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary, .ignoresCycle]
        panel.isMovable = false
        panel.hidesOnDeactivate = false
        panel.ignoresMouseEvents = true
        panel.acceptsMouseMovedEvents = true
        panel.delegate = self

        let host = NSHostingView(rootView: NotchView(model: model))
        if #available(macOS 13, *) { host.sizingOptions = [] }
        panel.contentView = host

        model.layoutChanged = { [weak self] in self?.updateMouse() }
        model.wantsKey = { [weak self] in self?.panel.makeKey() }
        panel.keyHandler = { [weak self] event in self?.model.handleReplyKey(event.keyCode) ?? false }
        NotificationCenter.default.addObserver(self, selector: #selector(screensChanged),
                                               name: NSApplication.didChangeScreenParametersNotification, object: nil)
        NotificationCenter.default.addObserver(forName: NSMenu.didBeginTrackingNotification, object: nil, queue: .main) { [weak self] _ in
            self?.model.menuOpen = true
        }
        NotificationCenter.default.addObserver(forName: NSMenu.didEndTrackingNotification, object: nil, queue: .main) { [weak self] _ in
            guard let self else { return }
            self.model.menuOpen = false
            // Fold away now if the pointer ended up outside while the menu was open.
            self.mouseInside = self.panel.isVisible && NSMouseInRect(NSEvent.mouseLocation, self.shapeRect, false)
            self.model.setHover(self.mouseInside)
        }

        // Hover is tracked by hand: the panel ignores the mouse while it's outside the shape.
        let events: NSEvent.EventTypeMask = [.mouseMoved, .leftMouseDragged]
        if let global = NSEvent.addGlobalMonitorForEvents(matching: events, handler: { [weak self] _ in self?.updateMouse() }) {
            monitors.append(global)
        }
        if let local = NSEvent.addLocalMonitorForEvents(matching: events, handler: { [weak self] e in self?.updateMouse(); return e }) {
            monitors.append(local)
        }
    }

    var isVisible: Bool { panel.isVisible }

    func show() {
        screensChanged()
        panel.orderFrontRegardless()
    }

    func hide() {
        model.go(.collapsed)
        panel.orderOut(nil)
    }

    /// Prefers the screen with a camera housing; otherwise sits under the main screen's menu bar.
    @objc private func screensChanged() {
        let notched = NSScreen.screens.first { $0.safeAreaInsets.top > 0 }
        screen = notched ?? NSScreen.main
        guard let frame = screen?.frame else { return }
        if let s = notched, let left = s.auxiliaryTopLeftArea, let right = s.auxiliaryTopRightArea {
            let width = s.frame.width - left.width - right.width
            model.notchSize = CGSize(width: width, height: s.safeAreaInsets.top)
            centerX = frame.minX + left.width + width / 2
        } else {
            model.notchSize = CGSize(width: 0, height: max(24, frame.maxY - (screen?.visibleFrame.maxY ?? frame.maxY)))
            centerX = frame.midX
        }
        let size = Self.canvas
        panel.setFrame(NSRect(x: centerX - size.width / 2, y: frame.maxY - size.height,
                              width: size.width, height: size.height), display: true)
    }

    /// The black shape's area on screen for the current state.
    private var shapeRect: NSRect {
        guard let frame = screen?.frame else { return .zero }
        let size = model.size(for: model.state)
        // A little slack below the collapsed strip makes it easier to hit.
        let extra: CGFloat = model.state == .collapsed ? 4 : 0
        return NSRect(x: centerX - size.width / 2, y: frame.maxY - size.height - extra,
                      width: size.width, height: size.height + extra)
    }

    private func updateMouse() {
        let inside = panel.isVisible && NSMouseInRect(NSEvent.mouseLocation, shapeRect, false)
        panel.ignoresMouseEvents = !inside
        if inside != mouseInside {
            mouseInside = inside
            model.setHover(inside)
        }
        if case .detail = model.state, !panel.isKeyWindow { panel.makeKey() }
    }

    func windowDidResignKey(_ notification: Notification) {
        // Clicking anywhere else closes the conversation; a reply in flight still completes.
        if case .detail = model.state { model.go(.collapsed) }
    }
}
