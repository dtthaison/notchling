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

    func testFind() {
        let list = [
            VaultEntry(name: "github", kind: .token, note: "my repos", url: "https://api.github.com", secretVar: "GH_TOKEN"),
            VaultEntry(name: "npm", kind: .token, url: "https://registry.npmjs.org", secretVar: "NPM_TOKEN"),
            VaultEntry(name: "aws-prod", kind: .login, note: "AWS console", userVar: "A_U", secretVar: "A_P"),
        ]
        XCTAssertEqual(SecretVault.find("https://api.github.com/repos/x/y", in: list).first?.name, "github")
        XCTAssertEqual(SecretVault.find("uploads.github.com", in: list).first?.name, "github")
        XCTAssertEqual(SecretVault.find("publish to registry.npmjs.org", in: list).first?.name, "npm")
        XCTAssertEqual(SecretVault.find("aws console", in: list).map(\.name), ["aws-prod"])
        XCTAssertTrue(SecretVault.find("gitlab.com", in: list).isEmpty)
        XCTAssertEqual(SecretVault.find("", in: list).count, 3)
        XCTAssertEqual(VaultEntry.host(of: "https://www.Example.com/x"), "example.com")
        XCTAssertNil(VaultEntry.host(of: "github"))
    }

    func testProblems() {
        let a = VaultEntry(name: "a", kind: .token, secretVar: "A_TOKEN")
        XCTAssertNil(SecretVault.problem(saving: a, in: [], replacing: nil))
        XCTAssertNotNil(SecretVault.problem(saving: a, in: [a], replacing: nil))
        XCTAssertNil(SecretVault.problem(saving: a, in: [a], replacing: a))
        let b = VaultEntry(name: "b", kind: .token, secretVar: "A_TOKEN")
        XCTAssertNotNil(SecretVault.problem(saving: b, in: [a], replacing: nil))
        let c = VaultEntry(name: "c", kind: .token, url: "x.com", secretVar: "C_URL")
        XCTAssertNotNil(SecretVault.problem(saving: c, in: [], replacing: nil))
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
