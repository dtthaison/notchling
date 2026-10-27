import AppKit
import ApplicationServices

/// Finds where the idle pet sits: the top edge of the frontmost window, next to its traffic-light
/// buttons, read through Accessibility. Polls while it is wanted
/// (and on every app switch) off the main thread, since a slow app can stall an AX call.
final class TabPerch {
    /// The notch screen's frame and the camera's x, in Cocoa screen coordinates.
    var geometry: () -> (frame: CGRect, centerX: CGFloat)? = { nil }
    /// Whether idle mode is on right now; when it isn't, nothing is polled.
    var isWanted: () -> Bool = { false }
    /// The perch in the pet's camera-centred frame (x = 0 under the camera, y = 0 at the top of the
    /// screen), or nil when there is nothing to sit on.
    var onChange: (CGRect?) -> Void = { _ in }

    private var timer: Timer?
    private var observer: NSObjectProtocol?
    private var busy = false
    private var published: CGRect?

    /// The pet's layer is only so tall; a window hanging lower than this has no seat for it.
    private static let lowest = 190.0

    func start() {
        guard timer == nil else { return }
        let timer = Timer(timeInterval: 0.5, repeats: true) { [weak self] _ in self?.poll() }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
        observer = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.poll() }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        observer.map(NSWorkspace.shared.notificationCenter.removeObserver)
        observer = nil
    }

    private func poll() {
        guard isWanted() else { return publish(nil) }
        guard AXIsProcessTrusted() else {
            askOnce()
            return publish(nil)
        }
        guard !busy, let geometry = geometry(), let app = NSWorkspace.shared.frontmostApplication else { return }
        // Our own windows (settings, accounts) have nothing to sit on; stay where it is.
        guard app.processIdentifier != ProcessInfo.processInfo.processIdentifier else { return }
        busy = true
        let pid = app.processIdentifier
        let primaryTop = (NSScreen.screens.first?.frame.maxY ?? geometry.frame.maxY)
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let found = Self.find(pid: pid, screen: geometry.frame, centerX: geometry.centerX, primaryTop: primaryTop)
            DispatchQueue.main.async {
                self?.busy = false
                self?.publish(found)
            }
        }
    }

    private func publish(_ rect: CGRect?) {
        func close(_ a: CGRect, _ b: CGRect) -> Bool {
            abs(a.minX - b.minX) < 1 && abs(a.minY - b.minY) < 1 && abs(a.width - b.width) < 1
        }
        if let rect, let published, close(rect, published) { return }
        if rect == nil && published == nil { return }
        published = rect
        onChange(rect)
    }

    /// The system prompt shows once, ever: asking again on every launch would nag when declined.
    private func askOnce() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: "askedAccessibility") else { return }
        defaults.set(true, forKey: "askedAccessibility")
        _ = AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary)
    }

    // MARK: Reading the window

    /// AX positions have their origin at the top-left of the primary screen, y down, so the notch
    /// screen's top is `primaryTop - screen.maxY` down from it, and x lines up with Cocoa's.
    private static func find(pid: pid_t, screen: CGRect, centerX: CGFloat, primaryTop: CGFloat) -> CGRect? {
        let app = AXUIElementCreateApplication(pid)
        AXUIElementSetMessagingTimeout(app, 0.25)
        guard let window = element(app, kAXFocusedWindowAttribute),
              let windowFrame = frame(window) else { return nil }
        let screenTop = primaryTop - screen.maxY
        let bounds = CGRect(x: screen.minX, y: screenTop, width: screen.width, height: screen.height)
        guard bounds.contains(CGPoint(x: windowFrame.midX, y: windowFrame.minY + 1)) else { return nil }
        if flag(window, "AXFullScreen") == true || windowFrame.width >= bounds.width && windowFrame.height >= bounds.height {
            return nil
        }

        // Just right of the traffic lights (which sit at minX + 10...72), kept inside narrow windows.
        let seatX = min(windowFrame.minX + 92, windowFrame.maxX - 20)
        let seat = CGRect(x: seatX - 60, y: windowFrame.minY, width: 120, height: 0)
        let perch = CGRect(x: seat.minX - centerX, y: seat.minY - screenTop, width: seat.width, height: seat.height)
        return perch.minY <= lowest ? perch : nil
    }

    private static func copy(_ node: AXUIElement, _ attribute: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(node, attribute as CFString, &value) == .success else { return nil }
        return value
    }

    private static func element(_ node: AXUIElement, _ attribute: String) -> AXUIElement? {
        guard let value = copy(node, attribute), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
        return unsafeBitCast(value, to: AXUIElement.self)
    }

    private static func flag(_ node: AXUIElement, _ attribute: String) -> Bool? {
        (copy(node, attribute) as? NSNumber)?.boolValue
    }

    private static func frame(_ node: AXUIElement) -> CGRect? {
        var origin = CGPoint.zero, size = CGSize.zero
        guard let position = copy(node, kAXPositionAttribute), CFGetTypeID(position) == AXValueGetTypeID(),
              AXValueGetValue(unsafeBitCast(position, to: AXValue.self), .cgPoint, &origin),
              let extent = copy(node, kAXSizeAttribute), CFGetTypeID(extent) == AXValueGetTypeID(),
              AXValueGetValue(unsafeBitCast(extent, to: AXValue.self), .cgSize, &size) else { return nil }
        return CGRect(origin: origin, size: size)
    }
}
