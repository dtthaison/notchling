import XCTest
@testable import Notchling

/// The order-keeping JSON re-indenter and the media a message names.
final class JSONPrettyTests: XCTestCase {
    func testKeysKeepTheirOrderAndIndentByTwo() {
        let out = JSONPretty.format(#"{"verdict": "ISSUES", "findings": ["a", "b"], "alpha": 1}"#)
        XCTAssertEqual(out, """
        {
          "verdict": "ISSUES",
          "findings": [
            "a",
            "b"
          ],
          "alpha": 1
        }
        """)
    }

    func testNestedObjectsAndArrays() {
        let out = JSONPretty.format(#"[{"a":{"b":[1,[2,3],{}]}},[]]"#)
        XCTAssertEqual(out, """
        [
          {
            "a": {
              "b": [
                1,
                [
                  2,
                  3
                ],
                {}
              ]
            }
          },
          []
        ]
        """)
    }

    func testStringsWithEscapesAndBracesAreCopiedAsWritten() {
        let input = #"{"q":"say \"hi\", {not} [json]: \\","u":"\u00e9\n","n":-1.50e+3,"t":true,"z":null}"#
        let out = JSONPretty.format(input)
        XCTAssertEqual(out, """
        {
          "q": "say \\"hi\\", {not} [json]: \\\\",
          "u": "\\u00e9\\n",
          "n": -1.50e+3,
          "t": true,
          "z": null
        }
        """)
        // Same value after re-indenting.
        let a = try? JSONSerialization.jsonObject(with: Data(input.utf8)) as? NSDictionary
        let b = try? JSONSerialization.jsonObject(with: Data((out ?? "").utf8)) as? NSDictionary
        XCTAssertNotNil(a)
        XCTAssertEqual(a, b)
    }

    func testAlreadyIndentedInputComesOutTheSame() {
        let pretty = JSONPretty.format(#"{"a":[1,2],"b":{"c":"d"}}"#) ?? ""
        XCTAssertEqual(JSONPretty.format(pretty), pretty)
    }

    func testInvalidInputIsReturnedUnchanged() {
        for bad in ["not json", "{\"a\": }", "{\"a\": 1", "[1, 2,]", "42", "\"str\"", "", "{'a': 1}"] {
            XCTAssertNil(JSONPretty.format(bad), bad)
            XCTAssertEqual(JSONPretty.formatted(bad), bad)
        }
    }

    func testToggleAppliesToJSONBlocksOnly() {
        XCTAssertTrue(JSONPretty.applies(language: "json", text: #"{"a":1}"#))
        XCTAssertTrue(JSONPretty.applies(language: "", text: #"[1,2]"#))
        XCTAssertFalse(JSONPretty.applies(language: "json", text: "{broken"))
        XCTAssertFalse(JSONPretty.applies(language: "swift", text: #"{"a":1}"#))
    }

    // MARK: Media

    func testMediaFoundByExtensionAndExistence() {
        let disk: Set<String> = ["/out/clip.mp4", "/out/shot.PNG", "/out/notes.md", "/out/gone"]
        let exists: ChatPaths.Exists = { disk.contains($0) }
        let text = "Rendered /out/clip.mp4 and /out/shot.PNG, plus /out/notes.md and /out/missing.mov."
        let items = ChatMedia.find(in: text, bases: [], exists: exists)
        XCTAssertEqual(items, [ChatMedia.Item(path: "/out/clip.mp4", kind: .video), ChatMedia.Item(path: "/out/shot.PNG", kind: .image)])
    }

    func testMediaRepeatsAreOneAndTheLimitHolds() {
        let names = (1...6).map { "/m/v\($0).mov" }
        let exists: ChatPaths.Exists = { names.contains($0) }
        let text = names.joined(separator: " ") + " /m/v1.mov"
        XCTAssertEqual(ChatMedia.find(in: text, bases: [], exists: exists).count, ChatMedia.perMessage)
        XCTAssertEqual(ChatMedia.find(in: "/m/v1.mov /m/v1.mov", bases: [], exists: exists).count, 1)
        XCTAssertTrue(ChatMedia.find(in: "/m/v1.mov", bases: [], skipping: ["/m/v1.mov"], exists: exists).isEmpty)
    }

    func testRelativeMediaReadAgainstTheSessionFolder() {
        let exists: ChatPaths.Exists = { $0 == "/repo/out/final.m4v" }
        let items = ChatMedia.find(in: "see out/final.m4v", bases: ["/repo"], exists: exists)
        XCTAssertEqual(items, [ChatMedia.Item(path: "/repo/out/final.m4v", kind: .video)])
    }

    func testDurationBadge() {
        XCTAssertEqual(MediaPreviewCard.duration(5), "0:05")
        XCTAssertEqual(MediaPreviewCard.duration(65.4), "1:05")
        XCTAssertEqual(MediaPreviewCard.duration(3725), "1:02:05")
    }
}
