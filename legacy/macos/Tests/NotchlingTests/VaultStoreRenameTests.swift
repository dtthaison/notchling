import XCTest
@testable import Notchling

/// `VaultStore.rename` against an in-memory index and value map: never the real vault.json or the Keychain.
final class VaultStoreRenameTests: XCTestCase {
    private final class Fake: @unchecked Sendable {
        var index: [VaultEntry]
        var values: [String: VaultValue]
        var failIndexWrites = 0
        init(index: [VaultEntry], values: [String: VaultValue]) { self.index = index; self.values = values }
        struct Down: Error {}

        func backend(center: NotificationCenter) -> VaultStore.Backend {
            VaultStore.Backend(readIndex: { self.index },
                               writeIndex: { list in
                                   if self.failIndexWrites > 0 { self.failIndexWrites -= 1; throw Down() }
                                   self.index = list
                               },
                               loadValues: { self.values },
                               writeValues: { self.values = $0 },
                               center: center)
        }
    }

    func testARenameThatFailedAtTheIndexFinishesOnTheSecondTryWithoutAChangeNotice() throws {
        let old = VaultEntry(name: "sd-claude", kind: .token, secretVar: "SD_CLAUDE_TOKEN")
        let fake = Fake(index: [old], values: ["sd-claude": VaultValue(secret: "the-claude-token-value")])
        let center = NotificationCenter()
        var notices: [String] = []
        let observer = center.addObserver(forName: .vaultValueChanged, object: nil, queue: .main) { note in
            notices.append(note.userInfo?["name"] as? String ?? "?")
        }
        defer { center.removeObserver(observer) }
        let store = VaultStore(backend: fake.backend(center: center))

        fake.failIndexWrites = 1
        XCTAssertThrowsError(try store.rename(old, to: "claude-worker"))
        // the state the review described: the value sits under the new name, the index still lists the old one
        XCTAssertEqual(fake.values.keys.sorted(), ["claude-worker"])
        XCTAssertEqual(fake.index.map(\.name), ["sd-claude"])

        try store.rename(old, to: "claude-worker")
        XCTAssertEqual(fake.index.map(\.name), ["claude-worker"])
        XCTAssertEqual(fake.index.first?.secretVar, "CLAUDE_WORKER_TOKEN")
        XCTAssertEqual(fake.values, ["claude-worker": VaultValue(secret: "the-claude-token-value")], "the value is kept, once")

        RunLoop.main.run(until: Date().addingTimeInterval(0.2))   // announce posts on the main queue
        XCTAssertEqual(notices, [], "the same value under a new name is not a change")
    }

    func testARealChangeStillAnnouncesAndAnEmptyRenameStillRefuses() throws {
        let old = VaultEntry(name: "sd-claude", kind: .token, secretVar: "SD_CLAUDE_TOKEN")
        let fake = Fake(index: [old], values: [:])
        let center = NotificationCenter()
        var notices: [String] = []
        let observer = center.addObserver(forName: .vaultValueChanged, object: nil, queue: .main) { note in
            notices.append(note.userInfo?["name"] as? String ?? "?")
        }
        defer { center.removeObserver(observer) }
        let store = VaultStore(backend: fake.backend(center: center))

        XCTAssertThrowsError(try store.rename(old, to: "claude-worker"), "no value anywhere: nothing to rename")
        XCTAssertEqual(fake.index.map(\.name), ["sd-claude"])

        try store.save(old, value: VaultValue(secret: "first-value-123"), replacing: old)
        try store.save(old, value: VaultValue(secret: "second-value-456"), replacing: old)
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        XCTAssertEqual(notices, ["sd-claude", "sd-claude"])
    }
}
