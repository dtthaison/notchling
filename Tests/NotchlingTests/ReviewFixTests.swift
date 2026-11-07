import XCTest
@testable import Notchling

final class ReviewFixTests: XCTestCase {
    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("review-fix-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
    }

    func testPrivateFileIsWrittenOwnerOnlyAndReplacesTheOld() throws {
        let url = dir.appendingPathComponent("auth.json")
        try Data("old".utf8).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: url.path)
        try PrivateFile.write(Data("new".utf8), to: url)
        XCTAssertEqual(try String(contentsOf: url, encoding: .utf8), "new")
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        XCTAssertEqual(mode?.intValue, 0o600)
        // no temporary file left beside it
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path), ["auth.json"])
    }

    func testVaultIndexReadIsEmptyOnlyWhenThereIsNoFile() throws {
        let url = dir.appendingPathComponent("vault.json")
        XCTAssertEqual(try SecretVault.readIndex(at: url), [])
        try Data("{ not json".utf8).write(to: url)
        XCTAssertThrowsError(try SecretVault.readIndex(at: url))
        // the unreadable file is left alone
        XCTAssertEqual(try String(contentsOf: url, encoding: .utf8), "{ not json")
    }

    func testDelegateNoLongerTakesGemini() {
        XCTAssertNil(Delegate.Tool(name: "gemini"))
        XCTAssertEqual(Delegate.Tool(name: "Antigravity"), .agy)
    }
}
