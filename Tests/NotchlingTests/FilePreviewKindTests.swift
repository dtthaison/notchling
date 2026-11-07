import XCTest
@testable import Notchling

/// Which viewer a file from a message opens in.
final class FilePreviewKindTests: XCTestCase {
    func kind(_ name: String, head: String? = nil) -> FilePreviewKind {
        FilePreviewKind.of(URL(fileURLWithPath: "/x/" + name), sniff: { _ in head.map { Data($0.utf8) } })
    }

    func testByType() {
        XCTAssertEqual(kind("README.md"), .markdown)
        XCTAssertEqual(kind("overnight-machines.html"), .web)
        XCTAssertEqual(kind("logo.svg"), .web)
        XCTAssertEqual(kind("timeline.ts"), .text, "TypeScript, which macOS registers as video (MPEG-2 transport stream)")
        XCTAssertEqual(kind("Package.swift"), .text)
        XCTAssertEqual(kind("state.json"), .text)
        XCTAssertEqual(kind("Dockerfile"), .text)
        XCTAssertEqual(kind("photo.PNG"), .image)
        XCTAssertEqual(kind("app-home.jpg"), .image)
        XCTAssertEqual(kind("spec.pdf"), .pdf)
        XCTAssertEqual(kind("aptusfit-reel-v2.mp4"), .media)
        XCTAssertEqual(kind("voice.m4a"), .media)
        XCTAssertEqual(kind("deck.pptx"), .quickLook)
        XCTAssertEqual(kind("budget.xlsx"), .quickLook)
    }

    func testUnknownFilesAreSniffed() {
        XCTAssertEqual(kind("notes.weird", head: "plain words\n"), .text)
        XCTAssertEqual(kind("blob.weird", head: "a\u{0}b"), .quickLook)
        XCTAssertEqual(kind("gone.weird", head: nil), .quickLook)
    }
}
