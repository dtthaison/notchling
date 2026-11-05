import XCTest
import AppKit
@testable import Notchling

final class PointerCursorsTests: XCTestCase {
    /// Controls get their own cursor rects; plain views keep NSView's, and installing twice is harmless.
    func testControlsGetTheHandAndOtherViewsDont() {
        let sel = #selector(NSView.resetCursorRects)
        let plain = class_getMethodImplementation(NSView.self, sel)
        PointerCursors.install()
        PointerCursors.install()
        XCTAssertNotEqual(class_getMethodImplementation(NSButton.self, sel), plain)
        XCTAssertNotEqual(class_getMethodImplementation(NSSegmentedControl.self, sel), plain)
        XCTAssertEqual(class_getMethodImplementation(NSView.self, sel), plain)

        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100), styleMask: [.titled], backing: .buffered, defer: true)
        let button = NSButton(title: "Run", target: nil, action: nil)
        window.contentView?.addSubview(button)
        button.resetCursorRects()
        button.isEnabled = false
        button.resetCursorRects()
    }
}
