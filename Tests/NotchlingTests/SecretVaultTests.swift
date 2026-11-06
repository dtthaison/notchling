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

    // MARK: Notes, how-to and expiry

    /// A calendar pinned to UTC so the tests give the same answer in every time zone.
    private var utc: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: "UTC")!
        return c
    }

    private func date(_ day: String, hour: Int = 12) -> Date {
        let p = day.split(separator: "-").map { Int($0)! }
        return utc.date(from: DateComponents(year: p[0], month: p[1], day: p[2], hour: hour))!
    }

    func testMultiLineNoteRoundTrips() throws {
        let note = "GitHub API for my repos.\nScopes: repo, read:org.\n\nRenew yearly."
        let entry = VaultEntry(name: "github", kind: .token, note: note, secretVar: "GITHUB_TOKEN")
        let back = try XCTUnwrap(SecretVault.decode(try SecretVault.encode([entry])).first)
        XCTAssertEqual(back.note, note)
        let text = SecretVault.describe(back, now: date("2026-11-06"), calendar: utc)
        XCTAssertTrue(text.contains("\n    GitHub API for my repos.\n    Scopes: repo, read:org.\n"), text)
        XCTAssertTrue(text.contains("\n    Renew yearly."), text)
    }

    func testHowToIsPrintedUnderTheNote() {
        let e = VaultEntry(name: "npm", kind: .token, note: "Publishing", howTo: "1. Open npmjs.com\n2. Tokens > Generate",
                           expires: "2026-12-06", secretVar: "NPM_TOKEN")
        let lines = SecretVault.describe(e, now: date("2026-11-06"), calendar: utc).components(separatedBy: "\n")
        let note = lines.firstIndex(of: "    Publishing")
        let label = lines.firstIndex(of: "    How to get/renew:")
        XCTAssertNotNil(note)
        XCTAssertEqual(label, note.map { $0 + 1 })
        XCTAssertEqual(lines[(label ?? 0) + 1], "      1. Open npmjs.com")
        XCTAssertEqual(lines[(label ?? 0) + 2], "      2. Tokens > Generate")
        XCTAssertTrue(lines.contains("    expires 2026-12-06 (in 30 days)"), lines.joined(separator: "\n"))
    }

    func testOldEntriesDecodeWithoutHowToAndExpires() throws {
        let old = #"[{"name":"a","kind":"token","note":"one\ntwo","userVar":"","secretVar":"A_TOKEN","updated":"2026-11-05T00:00:00Z"}]"#
        let e = try XCTUnwrap(SecretVault.decode(Data(old.utf8)).first)
        XCTAssertNil(e.howTo)
        XCTAssertNil(e.expires)
        XCTAssertEqual(e.note, "one\ntwo")
        XCTAssertFalse(SecretVault.describe(e, now: date("2026-11-06"), calendar: utc).contains("How to get"))
        XCTAssertFalse(SecretVault.describe(e, now: date("2026-11-06"), calendar: utc).contains("expires"))
    }

    func testExpiryParsing() {
        XCTAssertEqual(VaultExpiry.parse("2026-12-06"), "2026-12-06")
        XCTAssertEqual(VaultExpiry.parse(" 2028-02-29 "), "2028-02-29")
        for bad in ["", "tomorrow", "2026-13-01", "2026-02-30", "2027-02-29", "26-12-06", "2026-1-6", "2026/12/06", "2026-12-06T10:00"] {
            XCTAssertNil(VaultExpiry.parse(bad), bad)
        }
    }

    func testExpiryWording() {
        let now = date("2026-11-06", hour: 15)
        XCTAssertEqual(VaultExpiry.line(for: "2026-12-06", now: now, calendar: utc), "expires 2026-12-06 (in 30 days)")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-07", now: now, calendar: utc), "expires 2026-11-07 (in 1 day, renew soon)")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-06", now: now, calendar: utc), "expires 2026-11-06 (today, renew soon)")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-05", now: now, calendar: utc), "EXPIRED 2026-11-05")
        XCTAssertEqual(VaultExpiry.status(of: "2026-12-06", now: now, calendar: utc), .inDays(30))
        XCTAssertEqual(VaultExpiry.status(of: "2025-01-01", now: now, calendar: utc), .expired)
        XCTAssertNil(VaultExpiry.status(of: "nonsense", now: now, calendar: utc))
        let e = VaultEntry(name: "x", kind: .token, expires: "2026-11-01", secretVar: "X_TOKEN")
        XCTAssertTrue(SecretVault.describe(e, now: now, calendar: utc).contains("\n    EXPIRED 2026-11-01"))
    }

    func testExpiryDayConversion() {
        XCTAssertEqual(VaultExpiry.day(of: date("2026-12-06", hour: 23), calendar: utc), "2026-12-06")
        XCTAssertEqual(VaultExpiry.date(from: "2026-12-06", calendar: utc), date("2026-12-06"))
    }

    func testRequestParsingKeepsNewlinesAndChecksExpiry() throws {
        let ok = VaultCLI.parseRequest(["Notchling", "--secret-request", "gh", "--kind", "token", "--note", "a\nb",
                                        "--how", "Settings > Tokens\nPick repo", "--expires", "2026-12-06", "--reason", "r"])
        let r = try ok.get()
        XCTAssertEqual(r.name, "gh")
        XCTAssertEqual(r.note, "a\nb")
        XCTAssertEqual(r.howTo, "Settings > Tokens\nPick repo")
        XCTAssertEqual(r.expires, "2026-12-06")
        let bare = try VaultCLI.parseRequest(["Notchling", "--secret-request", "gh"]).get()
        XCTAssertNil(bare.howTo); XCTAssertNil(bare.expires); XCTAssertNil(bare.note)
        switch VaultCLI.parseRequest(["Notchling", "--secret-request", "gh", "--expires", "2026-02-30"]) {
        case .success: XCTFail("a bad date must be refused")
        case .failure(let e): XCTAssertTrue(e.message.contains("--expires") && e.message.contains("YYYY-MM-DD"), e.message)
        }
        if case .success = VaultCLI.parseRequest(["Notchling", "--secret-request", "gh", "--kind", "pin"]) { XCTFail("bad kind") }
        if case .success = VaultCLI.parseRequest(["Notchling", "--secret-request"]) { XCTFail("no name") }
    }

    func testEditPrefillMergesPassedFieldsOverSavedOnes() {
        let saved = VaultEntry(name: "gh", kind: .token, note: "old note", howTo: "old steps", expires: "2026-01-01",
                               secretVar: "GH_TOKEN")
        var saved2 = saved
        saved2.url = "https://api.github.com"
        let none = saved2.prefilled(by: VaultRequest(op: .request, name: "gh"))
        XCTAssertEqual(none, saved2)
        let some = saved2.prefilled(by: VaultRequest(op: .request, name: "gh", note: "new\nnote", howTo: "new steps"))
        XCTAssertEqual(some.note, "new\nnote")
        XCTAssertEqual(some.howTo, "new steps")
        XCTAssertEqual(some.url, "https://api.github.com")
        XCTAssertEqual(some.expires, "2026-01-01")
        let all = saved2.prefilled(by: VaultRequest(op: .request, name: "gh", url: "https://x.io", note: "n", howTo: "h", expires: "2027-03-04"))
        XCTAssertEqual([all.url, all.note, all.howTo, all.expires], ["https://x.io", "n", "h", "2027-03-04"])
        // Identity, kind and variables never change, and a bad date from an older client is ignored.
        XCTAssertEqual([all.name, all.secretVar], ["gh", "GH_TOKEN"])
        XCTAssertEqual(saved2.prefilled(by: VaultRequest(op: .request, name: "gh", expires: "soon")).expires, "2026-01-01")
    }

    func testSkillDocumentsHowAndExpires() {
        for tool in VaultSkill.Tool.allCases {
            let text = VaultSkill.content(binary: "/x/Notchling", names: [], tool: tool)
            XCTAssertTrue(text.contains("reminds the user"), "\(tool)")
            XCTAssertTrue(text.contains("--how"), "\(tool)")
            XCTAssertTrue(text.contains("--expires"), "\(tool)")
            XCTAssertTrue(text.contains("YYYY-MM-DD"), "\(tool)")
        }
    }

    func testHelpTextLinksBareURLsAndMarkdownLinks() {
        let text = VaultHelp.attributed("Open [Tokens](https://github.com/settings/tokens) or https://example.com/new\nthen tick repo.")
        let links = text.runs.compactMap(\.link).map(\.absoluteString)
        XCTAssertEqual(links, ["https://github.com/settings/tokens", "https://example.com/new"])
        XCTAssertTrue(String(text.characters).contains("\nthen tick repo."))
    }

    // MARK: Expiry reminders and renewing

    func testRenewSoonShowsWithinSevenDays() {
        let now = date("2026-11-06")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-13", now: now, calendar: utc), "expires 2026-11-13 (in 7 days, renew soon)")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-06", now: now, calendar: utc), "expires 2026-11-06 (today, renew soon)")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-14", now: now, calendar: utc), "expires 2026-11-14 (in 8 days)")
        XCTAssertEqual(VaultExpiry.line(for: "2026-11-05", now: now, calendar: utc), "EXPIRED 2026-11-05")
    }

    private func remind(_ expires: String?, now: String = "2026-11-06", sent: [String: [String]] = [:]) -> [VaultReminder.Due] {
        let e = VaultEntry(name: "sd-worker-m1", kind: .token, expires: expires, secretVar: "SD_TOKEN")
        return VaultReminder.due(entries: [e], sent: sent, now: date(now, hour: 9), calendar: utc)
    }

    func testReminderThresholds() {
        XCTAssertEqual(remind("2026-11-14"), [], "8 days out is nothing")
        XCTAssertEqual(remind(nil), [])
        let week = remind("2026-11-13")
        XCTAssertEqual(week.map(\.threshold), [.week])
        XCTAssertEqual(week.first?.message, "sd-worker-m1 expires in 7 days (2026-11-13)")
        XCTAssertEqual(remind("2026-11-07").first?.message, "sd-worker-m1 expires in 1 day (2026-11-07)")
        XCTAssertEqual(remind("2026-11-07").map(\.threshold), [.day])
        XCTAssertEqual(remind("2026-11-06").first?.message, "sd-worker-m1 expires today (2026-11-06)")
        XCTAssertEqual(remind("2026-11-06").map(\.threshold), [.today])
        XCTAssertEqual(remind("2026-11-05").map(\.threshold), [.expired])
        XCTAssertEqual(remind("2026-11-05").first?.message, "sd-worker-m1 expired on 2026-11-05")
        // The Mac was off for days: the nearest threshold fires once, with the real days left.
        XCTAssertEqual(remind("2026-11-09").first?.message, "sd-worker-m1 expires in 3 days (2026-11-09)")
    }

    func testReminderFiresOncePerThreshold() throws {
        let due = try XCTUnwrap(remind("2026-11-13").first)
        let sent = VaultReminder.markSent(due, in: [:])
        XCTAssertEqual(remind("2026-11-13", sent: sent), [])
        XCTAssertEqual(remind("2026-11-13", now: "2026-11-07", sent: sent), [], "6 days is still the 7-day reminder")
        XCTAssertEqual(remind("2026-11-13", now: "2026-11-12", sent: sent).map(\.threshold), [.day])
        let expired = try XCTUnwrap(remind("2026-11-01").first)
        let after = VaultReminder.markSent(expired, in: sent)
        XCTAssertEqual(remind("2026-11-01", sent: after), [])
        XCTAssertEqual(remind("2026-11-01", now: "2026-12-30", sent: after), [], "expired is announced once")
    }

    func testReminderMemoryResetsWhenExpiresChangesOrOnRenew() throws {
        let due = try XCTUnwrap(remind("2026-11-13").first)
        let sent = VaultReminder.markSent(due, in: [:])
        XCTAssertEqual(remind("2026-11-13", sent: sent), [])
        XCTAssertEqual(remind("2026-11-10", sent: sent).map(\.threshold), [.week], "a new date starts again")
        XCTAssertEqual(remind("2026-11-13", sent: VaultReminder.clearing("sd-worker-m1", in: sent)).map(\.threshold), [.week])
        let other = VaultReminder.markSent(VaultReminder.Due(name: "npm", id: "npm", day: "2026-11-13", threshold: .week, message: ""), in: sent)
        XCTAssertEqual(Set(VaultReminder.clearing("sd-worker-m1", in: other).keys), ["npm|2026-11-13"])
        let entries = [VaultEntry(name: "npm", kind: .token, expires: "2026-11-13", secretVar: "N")]
        XCTAssertEqual(Set(VaultReminder.pruned(other, to: entries).keys), ["npm|2026-11-13"])
    }

    func testRenewURL() {
        func url(_ u: String?, name: String = "x") -> String? {
            VaultRenew.url(for: VaultEntry(name: name, kind: .token, url: u, secretVar: "X_TOKEN"))?.absoluteString
        }
        XCTAssertEqual(url("https://api.github.com"), "https://github.com/settings/personal-access-tokens")
        XCTAssertEqual(url("github.com/org/repo"), "https://github.com/settings/personal-access-tokens")
        XCTAssertEqual(url("https://registry.npmjs.org/-/tokens"), "https://registry.npmjs.org/-/tokens")
        XCTAssertEqual(url("example.com/keys"), "https://example.com/keys")
        XCTAssertNil(url(nil))
        XCTAssertNil(url(""))
    }

    // MARK: Fix round 1

    func testRenewOpensOnlyHttpAndHttps() {
        func url(_ u: String) -> String? {
            VaultRenew.url(for: VaultEntry(name: "x", kind: .token, url: u, secretVar: "X_TOKEN"))?.absoluteString
        }
        for refused in ["file:///Users/me/evil.command", "smb://evil.example/share", "vscode://x/y", "javascript:alert(1)",
                        "ssh://host", "x-apple.systempreferences:com.apple.x", "https://", "ftp://example.com/f"] {
            XCTAssertNil(url(refused), refused)
        }
        XCTAssertEqual(url("https://example.com/keys"), "https://example.com/keys")
        XCTAssertEqual(url("http://localhost:3000/tokens"), "http://localhost:3000/tokens")
        // A URL that is refused is said so, as plain text.
        let bad = VaultEntry(name: "x", kind: .token, url: "smb://evil.example/share", secretVar: "X_TOKEN")
        XCTAssertEqual(VaultRenew.refusal(for: bad), "smb://evil.example/share")
        XCTAssertNil(VaultRenew.refusal(for: VaultEntry(name: "x", kind: .token, url: "https://example.com", secretVar: "X_TOKEN")))
        XCTAssertNil(VaultRenew.refusal(for: VaultEntry(name: "x", kind: .token, secretVar: "X_TOKEN")))
    }

    func testRenewNeedsAValueAndAChosenExpiry() {
        let saved = VaultEntry(name: "gh", kind: .token, expires: "2026-01-01", secretVar: "GH_TOKEN")
        let form = VaultForm(saved, renew: true)
        XCTAssertFalse(form.canSave, "no value, no expiry chosen")
        XCTAssertFalse(form.save(), "a renewal without a value never saves")
        XCTAssertNil(form.saved)
        form.secret = "a-new-value-that-is-not-real"
        XCTAssertFalse(form.canSave, "the expiry still has to be picked")
        XCTAssertFalse(form.save())
        form.chooseExpiry(date: false)
        XCTAssertTrue(form.canSave)
        XCTAssertNil(form.expiresDay, "No expiry stays off")
        form.secret = ""
        XCTAssertFalse(form.canSave, "choosing the expiry doesn't replace the value")
        form.secret = "x"
        form.chooseExpiry(date: true)
        XCTAssertNotNil(form.expiresDay)
        XCTAssertTrue(form.canSave)
    }

    func testRenewDoesNotTurnExpiryOnForAnEntryWithoutOne() {
        let none = VaultEntry(name: "gh", kind: .token, howTo: "steps", secretVar: "GH_TOKEN")
        let form = VaultForm(none, renew: true)
        XCTAssertFalse(form.hasExpiry)
        XCTAssertNil(form.expiresDay)
        XCTAssertFalse(form.expiryChosen)
        // An ordinary edit needs no value and no choice.
        XCTAssertTrue(VaultForm(none).canSave)
    }

    func testRequestChangesAreListedWithTheirOldValues() {
        let saved = VaultEntry(name: "gh", kind: .token, note: "old note", url: "https://api.github.com/v3", howTo: "old steps",
                               expires: "2026-01-01", secretVar: "GH_TOKEN")
        let same = VaultEntry.prefillChanges(of: saved, by: VaultRequest(op: .request, name: "gh", url: "https://api.github.com/v3",
                                                                       note: "old note", expires: "2026-01-01"))
        XCTAssertEqual(same, VaultPrefillChanges(), "passing the saved value changes nothing")
        let c = VaultEntry.prefillChanges(of: saved, by: VaultRequest(op: .request, name: "gh", url: "https://api.github-cdn.io/v3",
                                                                    note: "n", howTo: "h", expires: "2027-03-04"))
        XCTAssertEqual(c.old[.note], "old note")
        XCTAssertEqual(c.old[.url], "https://api.github.com/v3")
        XCTAssertEqual(c.old[.howTo], "old steps")
        XCTAssertEqual(c.old[.expires], "2026-01-01")
        XCTAssertEqual(c.hostChange, VaultPrefillChanges.HostChange(from: "api.github.com", to: "api.github-cdn.io"))
        // The path alone moving is a change, but not of host.
        let path = VaultEntry.prefillChanges(of: saved, by: VaultRequest(op: .request, name: "gh", url: "https://api.github.com/v4"))
        XCTAssertNotNil(path.old[.url])
        XCTAssertNil(path.hostChange)
        // www. and case don't count as another host.
        XCTAssertNil(VaultEntry.prefillChanges(of: saved, by: VaultRequest(op: .request, name: "gh", url: "HTTPS://API.GitHub.com")).hostChange)
        // A secret that had no URL gets one: that is a host change from nothing.
        let bare = VaultEntry(name: "gh", kind: .token, secretVar: "GH_TOKEN")
        let from = VaultEntry.prefillChanges(of: bare, by: VaultRequest(op: .request, name: "gh", url: "https://evil.io"))
        XCTAssertEqual(from.hostChange, VaultPrefillChanges.HostChange(from: nil, to: "evil.io"))
        XCTAssertEqual(from.old[.url], "")
        // The merged entry is still what prefilled() gives.
        XCTAssertEqual(saved.prefilled(by: VaultRequest(op: .request, name: "gh", note: "n")).note, "n")
    }

    func testHowToLinksAreHttpsOnlyAndShowTheirHost() {
        let evil = VaultHelp.attributed("[https://github.com/settings/tokens](https://github.com.evil.io/login) then go")
        XCTAssertEqual(String(evil.characters), "https://github.com/settings/tokens (github.com.evil.io) then go")
        XCTAssertEqual(evil.runs.compactMap(\.link).map(\.absoluteString), ["https://github.com.evil.io/login"])
        let file = VaultHelp.attributed("[run this](file:///tmp/a.command) or [site](http://example.com) or [x](vscode://a/b)")
        XCTAssertEqual(file.runs.compactMap(\.link).count, 0, "no link besides https")
        XCTAssertEqual(String(file.characters), "run this or site or x")
        let ok = VaultHelp.attributed("Open [Tokens](https://github.com/settings/tokens) and https://example.com/new, not http://example.org/x")
        XCTAssertEqual(ok.runs.compactMap(\.link).map(\.absoluteString), ["https://github.com/settings/tokens", "https://example.com/new"])
        XCTAssertTrue(String(ok.characters).hasPrefix("Open Tokens (github.com) and"))
    }
}
