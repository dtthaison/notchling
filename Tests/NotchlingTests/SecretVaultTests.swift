import XCTest
@testable import Notchling

final class SecretVaultTests: XCTestCase {
    private func run(_ hide: [String: String], chunks: [String]) -> String {
        let r = Redactor(hide)
        var out = Data()
        for c in chunks { out.append(r.feed(Data(c.utf8))) }
        out.append(r.finish())
        return String(decoding: out, as: UTF8.self)
    }

    func testHidesValue() {
        XCTAssertEqual(run(["s3cr3t-tok": "[secret:gh]"], chunks: ["token is s3cr3t-tok, again s3cr3t-tok\n"]),
                       "token is [secret:gh], again [secret:gh]\n")
    }

    func testHidesValueSplitAcrossReads() {
        XCTAssertEqual(run(["abcdefgh": "[x]"], chunks: ["one abc", "def", "gh two"]), "one [x] two")
    }

    func testKeepsShortStringsAndPlainOutput() {
        XCTAssertEqual(run(["ab": "[x]"], chunks: ["ab cd"]), "ab cd")
        XCTAssertEqual(run(["longsecret": "[x]"], chunks: ["a", "b", "c"]), "abc")
    }

    func testLongestFirst() {
        XCTAssertEqual(run(["pass": "[u]", "password1": "[p]"], chunks: ["password1 pass"]), "[p] [u]")
    }

    func testDefaultVariables() {
        XCTAssertEqual(VaultEntry.prefix(for: "my-api.v2"), "MY_API_V2")
        XCTAssertEqual(VaultEntry.defaultVars(name: "github", kind: .token).secret, "GITHUB_TOKEN")
        XCTAssertEqual(VaultEntry.defaultVars(name: "npm", kind: .login).user, "NPM_USERNAME")
        XCTAssertTrue(VaultEntry.validName("gh.work-2"))
        XCTAssertFalse(VaultEntry.validName("2fa"))
        XCTAssertFalse(VaultEntry.validVar("A-B"))
    }

    func testEnvironment() {
        let e = VaultEntry(name: "reg", kind: .login, userVar: "REG_USERNAME", secretVar: "REG_PASSWORD")
        let (env, hide) = SecretVault.environment(for: [(e, VaultValue(username: "alice", secret: "hunter22"))])
        XCTAssertEqual(env, ["REG_USERNAME": "alice", "REG_PASSWORD": "hunter22"])
        XCTAssertEqual(hide["hunter22"], "[secret:reg]")
    }

    func testURL() {
        let e = VaultEntry(name: "github", kind: .token, url: "https://api.github.com", secretVar: "GH_TOKEN")
        XCTAssertEqual(e.allVariables, ["GH_TOKEN", "GITHUB_URL"])
        XCTAssertEqual(SecretVault.environment(for: [(e, VaultValue(secret: "tok-1234"))]).env["GITHUB_URL"], "https://api.github.com")
        XCTAssertEqual(VaultSkill.label(e), "github (api.github.com)")
        XCTAssertEqual(VaultSkill.label(VaultEntry(name: "npm", kind: .token, url: "registry.npmjs.org")), "npm (registry.npmjs.org)")
        // Lists saved before URLs existed still load.
        let old = #"[{"name":"a","kind":"token","note":"","userVar":"","secretVar":"A_TOKEN","updated":"2026-11-05T00:00:00Z"}]"#
        let d = JSONDecoder(); d.dateDecodingStrategy = .iso8601
        XCTAssertNil(try d.decode([VaultEntry].self, from: Data(old.utf8)).first?.urlVar)
    }
}
