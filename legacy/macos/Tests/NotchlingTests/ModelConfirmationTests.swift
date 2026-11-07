import XCTest
@testable import Notchling

/// /model's confirmation line sets the header's model at once, not at the next reply.
final class ModelConfirmationTests: XCTestCase {
    func testModelID() {
        typealias P = ConversationReader.ConversationParser
        XCTAssertEqual(P.modelID(inConfirmation: "Set model to `Sonnet 5.5` and saved as your default for new sessions"), "claude-sonnet-5-5")
        XCTAssertEqual(P.modelID(inConfirmation: "<local-command-stdout>Kept model as `Opus 5.5`</local-command-stdout>"), "claude-opus-5-5")
        XCTAssertEqual(ChatFormat.modelName("claude-sonnet-5-5"), "Sonnet 5.5")
        XCTAssertNil(P.modelID(inConfirmation: "Set model to `Default (recommended)`"))
        XCTAssertNil(P.modelID(inConfirmation: "Something else"))
    }
}
