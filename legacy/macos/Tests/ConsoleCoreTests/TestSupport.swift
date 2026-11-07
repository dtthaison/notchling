import XCTest
@testable import ConsoleCore

/// Fixture time: 2026-11-01T13:57:45Z, the statusUpdatedAt of the busy session.
let fixtureNow = Date(timeIntervalSince1970: 1_793_541_465)

/// A writable copy of the hand-written fixtures, so tests can move mtimes and files around.
final class FixtureTree {
    let root: URL
    var paths: ClaudePaths {
        ClaudePaths(claudeDir: root.appendingPathComponent("claude"),
                    requestsDir: root.appendingPathComponent("notchling/requests"))
    }
    var chatDir: URL { root.appendingPathComponent("chat") }

    init() throws {
        guard let bundled = Bundle.module.resourceURL?.appendingPathComponent("Fixtures") else {
            throw CocoaError(.fileNoSuchFile)
        }
        root = FileManager.default.temporaryDirectory.appendingPathComponent("console-fixtures-\(UUID().uuidString)")
        try FileManager.default.copyItem(at: bundled, to: root)
    }

    func remove() {
        do { try FileManager.default.removeItem(at: root) } catch { XCTFail("cleanup failed: \(error)") }
    }

    func setModified(_ date: Date, _ relativePath: String) throws {
        try FileManager.default.setAttributes([.modificationDate: date],
                                              ofItemAtPath: root.appendingPathComponent(relativePath).path)
    }
}

extension XCTestCase {
    func makeFixtures() throws -> FixtureTree {
        let tree = try FixtureTree()
        addTeardownBlock { tree.remove() }
        return tree
    }
}

/// Every pid in the fixtures counts as alive unless a test removes it.
func allPids() -> Set<Int32> { [101, 102, 103, 104, 105, 106, 107, 108, 4242, 4243] }
