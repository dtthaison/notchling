import XCTest
@testable import ConsoleCore

final class HookResponderTests: XCTestCase {
    private func makeResponder() throws -> (HookResponder, URL) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("responses-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        return (HookResponder(responsesDir: directory), directory)
    }

    private func json(_ directory: URL, _ id: String) throws -> [String: Any] {
        let data = try Data(contentsOf: directory.appendingPathComponent("\(id).json"))
        return try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    private func question(multiSelect: Bool = false, questions: Int = 1) -> PendingPrompt {
        let items = (0..<questions).map {
            PromptQuestion(header: "H\($0)", question: "Question \($0)?", options: ["main", "develop"], multiSelect: multiSelect)
        }
        return PendingPrompt(id: "req-q", sessionId: "s", kind: .question(items), toolName: "AskUserQuestion",
                             summary: "Question 0?", created: fixtureNow)
    }

    private let permission = PendingPrompt(id: "req-p", sessionId: "s", kind: .permission, toolName: "Bash",
                                           summary: "ls", created: fixtureNow)

    func testAnswerShapeMatchesWhatHookBridgeReads() throws {
        let (responder, directory) = try makeResponder()
        try responder.answer(question(questions: 2), choices: ["main", "develop"])
        let object = try json(directory, "req-q")
        // HookBridge.output(for:event:) reads response["action"] == "answer" and response["answers"] as [String: String].
        XCTAssertEqual(object["action"] as? String, "answer")
        XCTAssertEqual(object["answers"] as? [String: String], ["Question 0?": "main", "Question 1?": "develop"])
        XCTAssertEqual(Set(object.keys), ["action", "answers"])
    }

    func testAllowAndDenyShape() throws {
        let (responder, directory) = try makeResponder()
        try responder.allow(permission)
        XCTAssertEqual(try json(directory, "req-p") as NSDictionary, ["action": "allow"] as NSDictionary)

        try responder.deny(permission)
        let denied = try json(directory, "req-p")
        XCTAssertEqual(denied["action"] as? String, "deny")
        XCTAssertNotNil(denied["message"] as? String)
    }

    func testFileIsNamedAfterTheRequestAndNoTemporaryFileRemains() throws {
        let (responder, directory) = try makeResponder()
        try responder.allow(permission)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["req-p.json"])
    }

    func testMultiSelectAndMismatchedAnswersAreRefused() throws {
        let (responder, directory) = try makeResponder()
        XCTAssertThrowsError(try responder.answer(question(multiSelect: true), choices: ["main"])) { error in
            guard case HookResponder.Failure.notAnswerable = error else { return XCTFail("\(error)") }
        }
        XCTAssertThrowsError(try responder.answer(question(questions: 2), choices: ["main"]))
        XCTAssertThrowsError(try responder.allow(question()))
        XCTAssertThrowsError(try responder.answer(permission, choices: []))
        XCTAssertFalse(FileManager.default.fileExists(atPath: directory.path), "nothing is written for a refused answer")
    }

    func testAnswersAPromptReadFromAFixtureRequest() throws {
        let tree = try makeFixtures()
        let prompts = PendingPromptReader(requestsDir: tree.paths.requestsDir, isAlive: { _ in true }).prompts()
        let prompt = try XCTUnwrap(prompts["sess-idle"])
        let responder = HookResponder(responsesDir: tree.paths.responsesDir)
        try responder.answer(prompt, choices: ["develop"])
        let object = try json(tree.paths.responsesDir, "req-question")
        XCTAssertEqual(object["answers"] as? [String: String], ["Which branch should I use?": "develop"])
    }

    func testResponsesDirectoryIsNextToRequests() {
        let paths = ClaudePaths.live
        XCTAssertTrue(paths.responsesDir.path.hasSuffix("Application Support/Notchling/responses"))
    }
}
