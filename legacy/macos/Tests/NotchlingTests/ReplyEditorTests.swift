import AppKit
import SwiftUI
import XCTest
@testable import Notchling

final class ReplyEditorTests: XCTestCase {
    private let font = KitType.body.nsFont
    private let long = String(repeating: "word another ", count: 40)

    func testEmptyIsOneLine() {
        let m = ReplyMeasurer()
        XCTAssertEqual(m.height(of: "", font: font, width: 300), m.lineHeight(for: font))
    }

    func testExplicitLinesGrowTheBox() {
        let m = ReplyMeasurer()
        let line = m.lineHeight(for: font)
        XCTAssertEqual(m.height(of: "a\nb\nc", font: font, width: 300), line * 3, accuracy: 1)
        // A trailing newline is an empty line the cursor sits on.
        XCTAssertEqual(m.height(of: "a\n", font: font, width: 300), line * 2, accuracy: 1)
    }

    func testStopsAtEightLines() {
        let m = ReplyMeasurer()
        let text = (0..<20).map { "line \($0)" }.joined(separator: "\n")
        XCTAssertEqual(m.height(of: text, font: font, width: 300), m.lineHeight(for: font) * 8)
        XCTAssertGreaterThan(m.contentHeight(of: text, font: font, width: 300), m.lineHeight(for: font) * 8)
    }

    /// The bug: after a window resize the box kept wrapping at the old width. Each width is measured on
    /// its own, and asking for an earlier width again gives the earlier answer.
    func testHeightFollowsTheWidthItIsAskedAt() {
        let m = ReplyMeasurer()
        let narrow = m.contentHeight(of: long, font: font, width: 160)
        let wide = m.contentHeight(of: long, font: font, width: 640)
        XCTAssertGreaterThan(narrow, wide)
        XCTAssertEqual(m.contentHeight(of: long, font: font, width: 160), narrow)
        XCTAssertEqual(m.contentHeight(of: long, font: font, width: 640), wide)
    }

    func testFontChangesTheMeasure() {
        let m = ReplyMeasurer()
        let body = m.contentHeight(of: long, font: font, width: 300)
        let mono = m.contentHeight(of: long, font: KitType.mono.nsFont, width: 300)
        XCTAssertNotEqual(body, mono)
    }

    func testProbeWidthDoesNotExplode() {
        let m = ReplyMeasurer()
        // SwiftUI proposes 0 while working out flexibility.
        XCTAssertEqual(m.height(of: long, font: font, width: 0), m.lineHeight(for: font) * 8)
    }

    /// The live view wraps at its scroll view's current width, so what the measurer says matches what is drawn.
    func testLiveViewRewrapsWhenResizedAndMatchesTheMeasurer() throws {
        let (scroll, view) = ReplyEditor.makeViews()
        view.string = long
        let m = ReplyMeasurer()
        for width in [200, 520, 260] as [CGFloat] {
            scroll.frame = NSRect(x: 0, y: 0, width: width, height: 300)
            scroll.layoutSubtreeIfNeeded()
            let container = try XCTUnwrap(view.textContainer)
            XCTAssertEqual(container.size.width, width, accuracy: 1, "container follows the frame at \(width)")
            let manager = try XCTUnwrap(view.layoutManager)
            manager.ensureLayout(for: container)
            let drawn = ceil(manager.usedRect(for: container).height)
            XCTAssertEqual(drawn, m.contentHeight(of: long, font: view.font ?? font, width: width), accuracy: 1,
                           "measured height equals the drawn height at \(width)")
        }
    }

    func testScrollViewPutsTheFirstLineBackWhenItFits() {
        let (scroll, view) = ReplyEditor.makeViews()
        view.string = "one\ntwo"
        scroll.frame = NSRect(x: 0, y: 0, width: 300, height: 120)
        scroll.layoutSubtreeIfNeeded()
        scroll.contentView.scroll(to: NSPoint(x: 0, y: 17))
        scroll.layout()
        XCTAssertEqual(scroll.contentView.bounds.origin.y, 0)
    }

    /// Through SwiftUI: the editor's height is asked for at the width it ends up with, and asked again when
    /// that width changes.
    func testHostedEditorChangesHeightWithTheWindowWidth() throws {
        func find(_ view: NSView) -> ReplyScrollView? {
            if let scroll = view as? ReplyScrollView { return scroll }
            return view.subviews.lazy.compactMap(find).first
        }
        let editor = ReplyEditor(text: .constant(long), focused: .constant(false), placeholder: "", placeholderIsSuggestion: false,
                                 font: font, accessibilityLabel: "Reply", onSubmit: {})
        let host = NSHostingView(rootView: editor.frame(maxWidth: .infinity).padding(.vertical, 6))
        host.frame = NSRect(x: 0, y: 0, width: 220, height: 400)
        host.layoutSubtreeIfNeeded()
        let narrow = try XCTUnwrap(find(host)).frame
        host.frame = NSRect(x: 0, y: 0, width: 700, height: 400)
        host.layoutSubtreeIfNeeded()
        let wide = try XCTUnwrap(find(host)).frame
        XCTAssertEqual(narrow.width, 220, accuracy: 1)
        XCTAssertEqual(wide.width, 700, accuracy: 1)
        let m = ReplyMeasurer()
        XCTAssertEqual(narrow.height, m.height(of: long, font: font, width: 220), accuracy: 1)
        XCTAssertEqual(wide.height, m.height(of: long, font: font, width: 700), accuracy: 1)
        XCTAssertGreaterThan(narrow.height, wide.height)
    }
}
