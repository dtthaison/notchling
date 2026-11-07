import XCTest
@testable import Notchling

/// What the hook prints back to Claude Code for each answer from the notch.
final class HookOutputTests: XCTestCase {
    func specific(_ out: [String: Any]?) throws -> [String: Any] {
        try XCTUnwrap(out?["hookSpecificOutput"] as? [String: Any])
    }

    func testAnswerFromPreToolUse() throws {
        let event: [String: Any] = ["hook_event_name": "PreToolUse", "tool_input": ["questions": ["q"]]]
        let out = try specific(HookBridge.output(for: ["action": "answer", "answers": ["q": "Yes"]], event: event))
        XCTAssertEqual(out["hookEventName"] as? String, "PreToolUse")
        XCTAssertEqual(out["permissionDecision"] as? String, "allow")
        let input = try XCTUnwrap(out["updatedInput"] as? [String: Any])
        XCTAssertEqual(input["answers"] as? [String: String], ["q": "Yes"])
        XCTAssertNotNil(input["questions"], "updatedInput replaces the whole input, so questions go back too")
    }

    func testAnswerFromPermissionRequest() throws {
        let event: [String: Any] = ["hook_event_name": "PermissionRequest", "tool_input": [:]]
        let out = try specific(HookBridge.output(for: ["action": "answer", "answers": [:]], event: event))
        XCTAssertEqual((out["decision"] as? [String: Any])?["behavior"] as? String, "allow")
    }

    func testDenyHasAMessage() throws {
        let out = try specific(HookBridge.output(for: ["action": "deny"], event: [:]))
        let decision = try XCTUnwrap(out["decision"] as? [String: Any])
        XCTAssertEqual(decision["behavior"] as? String, "deny")
        XCTAssertEqual(decision["message"] as? String, "Denied from the notch.")
    }

    func testAllowWithoutAlwaysAddsNoRules() throws {
        let out = try specific(HookBridge.output(for: ["action": "allow"], event: [:]))
        XCTAssertNil((out["decision"] as? [String: Any])?["updatedPermissions"])
    }

    func testPassPrintsNothing() {
        XCTAssertNil(HookBridge.output(for: ["action": "pass"], event: [:]))
        XCTAssertNil(HookBridge.output(for: [:], event: [:]))
    }
}

/// Finding saved Artifact pages by link, id or words.
final class PageLookupTests: XCTestCase {
    let pages = [
        PageLibrary.Entry(folder: "a", file: "index.html", title: "Cost Report", url: "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv",
                          source: "", project: "notchling", account: "me@example.com", updated: 1),
        PageLibrary.Entry(folder: "b", file: "index.html", title: "Release Plan", url: "https://claude.ai/code/artifact/ZZZZZZZZZZZZZZZZZZZZZZ",
                          source: "", project: "lingeva", account: "work@example.com", updated: 2),
    ]

    func testArtifactIDFromLinksAndBareIDs() {
        XCTAssertEqual(PageLibrary.artifactID(in: " https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv "), "AbCdEfGhIjKlMnOpQrStUv")
        XCTAssertEqual(PageLibrary.artifactID(in: "claude.ai/code/artifact/abc_def-123"), "abc_def-123")
        XCTAssertEqual(PageLibrary.artifactID(in: "KVBHDXYvY8f1b1AZG6PbAR"), "KVBHDXYvY8f1b1AZG6PbAR")
        XCTAssertNil(PageLibrary.artifactID(in: "cost report"))
    }

    func testFindByLinkThenByWords() {
        XCTAssertEqual(PageLibrary.find("https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv", in: pages).map(\.folder), ["a"])
        XCTAssertEqual(PageLibrary.find("abcdefghijklmnopqrstuv", in: pages).map(\.folder), ["a"], "an id matches regardless of case as a fallback")
        XCTAssertEqual(PageLibrary.find("plan lingeva", in: pages).map(\.folder), ["b"])
        XCTAssertEqual(PageLibrary.find("work@example", in: pages).map(\.folder), ["b"])
        XCTAssertEqual(PageLibrary.find("", in: pages).count, 2)
        XCTAssertTrue(PageLibrary.find("nothing here", in: pages).isEmpty)
    }

    func testEntryJSONRoundTrip() throws {
        let e = pages[0]
        let back = try XCTUnwrap(PageLibrary.Entry(e.json))
        XCTAssertEqual(back.json as NSDictionary, e.json as NSDictionary)
        XCTAssertNil(PageLibrary.Entry(["title": "no folder"]))
    }

    /// A 22-character link id is the page's UUID in base58.
    func testShortIDToUUID() {
        XCTAssertEqual(OwnerPublish.uuid(fromShortID: "2vQiSN564g8wL9dUb2VwFy"), "0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f4a6c")
        XCTAssertEqual(OwnerPublish.uuid(fromShortID: "not-base58!"), "not-base58!", "anything else passes through")
    }
}

/// Claude's markdown into blocks for the chat.
final class MarkdownTests: XCTestCase {
    func testBlocks() {
        let text = """
        # Title

        First paragraph
        continues here.

        - one
        - two

        ```swift
        let x = 1
        ```

        | A | B |
        | 1 | 2 |

        ---
        """
        let blocks = MarkdownParser.blocks(text)
        XCTAssertEqual(blocks.first, .heading(level: 1, text: "Title"))
        XCTAssertTrue(blocks.contains(.code(language: "swift", text: "let x = 1")))
        XCTAssertTrue(blocks.contains(.table(header: ["A", "B"], rows: [["1", "2"]])), "tables need no |---| row")
        XCTAssertEqual(blocks.last, .rule)
        XCTAssertTrue(blocks.contains { if case .list(let items) = $0 { return items.map(\.text) == ["one", "two"] }; return false })
        XCTAssertTrue(blocks.contains { if case .paragraph(let p) = $0 { return p.contains("First paragraph") }; return false })
    }
}
