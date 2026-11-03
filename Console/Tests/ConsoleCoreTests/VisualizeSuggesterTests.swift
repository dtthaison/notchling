import XCTest
@testable import ConsoleCore

final class VisualizeSuggesterTests: XCTestCase {
    private func folder(_ files: [String: String] = [:], directories: [String] = []) throws -> String {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("viz-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        for directory in directories {
            try FileManager.default.createDirectory(at: root.appendingPathComponent(directory), withIntermediateDirectories: true)
        }
        for (path, content) in files {
            let url = root.appendingPathComponent(path)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data(content.utf8).write(to: url)
        }
        return root.path
    }

    private let suggester = VisualizeSuggester()

    func testPhoneForFlutterXcodeAndAndroidFolders() throws {
        XCTAssertEqual(suggester.suggest(cwd: try folder(["pubspec.yaml": "name: a"])), .phone)
        XCTAssertEqual(suggester.suggest(cwd: try folder(directories: ["App.xcodeproj"])), .phone)
        XCTAssertEqual(suggester.suggest(cwd: try folder(directories: ["android"])), .phone)
    }

    func testWebForNextViteOrReact() throws {
        XCTAssertEqual(suggester.suggest(cwd: try folder(["package.json": #"{"dependencies":{"next":"15"}}"#])), .web)
        XCTAssertEqual(suggester.suggest(cwd: try folder(["package.json": #"{"devDependencies":{"vite":"6"}}"#])), .web)
        XCTAssertEqual(suggester.suggest(cwd: try folder(["package.json": #"{"dependencies":{"react":"19"}}"#])), .web)
    }

    func testEntityForPrismaDriftOrSql() throws {
        XCTAssertEqual(suggester.suggest(cwd: try folder(["prisma/schema.prisma": "model A {}"])), .entity)
        XCTAssertEqual(suggester.suggest(cwd: try folder(["schema.sql": "create table a();"])), .entity)
        XCTAssertEqual(suggester.suggest(cwd: try folder(["lib/db/tables.dart": "class A extends Table {}"])), .entity)
    }

    func testFlowIsTheFallback() throws {
        XCTAssertEqual(suggester.suggest(cwd: try folder()), .flow)
        XCTAssertEqual(suggester.suggest(cwd: try folder(["package.json": #"{"dependencies":{"lodash":"4"}}"#])), .flow)
        XCTAssertEqual(suggester.suggest(cwd: ""), .flow)
        XCTAssertEqual(suggester.suggest(cwd: "/definitely/not/here"), .flow)
    }

    func testPhoneWinsOverEntityWhenBothMatch() throws {
        XCTAssertEqual(suggester.suggest(cwd: try folder(["pubspec.yaml": "x", "prisma/schema.prisma": "y"])), .phone)
    }

    func testBrokenPackageJsonFallsThrough() throws {
        XCTAssertEqual(suggester.suggest(cwd: try folder(["package.json": "{not json"])), .flow)
    }

    func testGitBranchFromHead() throws {
        let attached = try folder([".git/HEAD": "ref: refs/heads/feat/agent-console\n"])
        XCTAssertEqual(GitBranch.current(cwd: attached), "feat/agent-console")
        let detached = try folder([".git/HEAD": "0123456789abcdef\n"])
        XCTAssertEqual(GitBranch.current(cwd: detached), "0123456")
        XCTAssertNil(GitBranch.current(cwd: try folder()))
    }

    func testChatResultCarriesTheLatestAssistantModel() throws {
        let lines = [
            #"{"type":"assistant","uuid":"a","message":{"role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"text","text":"x"}]}}"#,
            #"{"type":"assistant","uuid":"b","message":{"role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"y"}]}}"#,
        ].map { Substring($0) }
        XCTAssertEqual(ChatTranscript.parse(lines: lines, isMain: true, path: "t").model, "claude-opus-5-5")
    }
}
