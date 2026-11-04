import XCTest
@testable import Notchling

/// The inspector's Git section: reading `git status` and `git diff --numstat`.
final class GitInfoTests: XCTestCase {
    func testStatus() {
        let text = """
        ## feat/data-connectors...origin/feat/data-connectors [ahead 2, behind 1]
         M src/app.ts
        A  src/new.ts
         D old.ts
        R  a/before.ts -> a/after.ts
        ?? notes.md
        UU conflict.ts
        """
        let info = GitInfo.parseStatus(text)
        XCTAssertEqual(info.branch, "feat/data-connectors")
        XCTAssertEqual(info.upstream, "origin/feat/data-connectors")
        XCTAssertEqual(info.ahead, 2)
        XCTAssertEqual(info.behind, 1)
        XCTAssertEqual(info.changes.map(\.status), ["M", "A", "D", "R", "?", "U"])
        XCTAssertEqual(info.changes.map(\.path), ["src/app.ts", "src/new.ts", "old.ts", "a/after.ts", "notes.md", "conflict.ts"])
    }

    func testBranchWithoutUpstream() {
        XCTAssertEqual(GitInfo.parseStatus("## main\n").branch, "main")
        XCTAssertNil(GitInfo.parseStatus("## main\n").upstream)
        XCTAssertEqual(GitInfo.parseStatus("## No commits yet on main\n").branch, "main")
        XCTAssertEqual(GitInfo.parseStatus("## HEAD (no branch)\n").branch, "detached HEAD")
    }

    func testNumstat() {
        var info = GitInfo.parseStatus("## main\n M src/app.ts\nR  a/before.ts -> a/after.ts\n M logo.png\n")
        info.applyNumstat("12\t3\tsrc/app.ts\n4\t0\ta/{before.ts => after.ts}\n-\t-\tlogo.png\n")
        XCTAssertEqual(info.changes[0].added, 12)
        XCTAssertEqual(info.changes[0].removed, 3)
        XCTAssertEqual(info.changes[1].added, 4)
        XCTAssertNil(info.changes[2].added)
    }
}
