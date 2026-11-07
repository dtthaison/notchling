import XCTest
@testable import Notchling

final class ComposeAttachmentsTests: XCTestCase {
    func testTextThenPaths() {
        XCTAssertEqual(ChatModel.compose("look at this\n", paths: ["/tmp/a.png"]), "look at this /tmp/a.png")
    }

    func testPathsWithSpacesAreQuoted() {
        XCTAssertEqual(ChatModel.compose("", paths: ["/x/Screenshot 1.png", "/x/b.png"]), "\"/x/Screenshot 1.png\" /x/b.png")
    }

    func testNoAttachmentsKeepsText() {
        XCTAssertEqual(ChatModel.compose("  hi  ", paths: []), "hi")
        XCTAssertEqual(ChatModel.compose("", paths: []), "")
    }
}
