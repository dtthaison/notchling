import XCTest
@testable import Notchling

/// Codex CLI logins (`~/.codex/auth.json`, 0.166) and its usage endpoint, with made-up tokens.
final class CodexLoginTests: XCTestCase {
    private func jwt(_ claims: [String: Any]) -> String {
        let body = try! JSONSerialization.data(withJSONObject: claims)
        let b64 = body.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        return "eyJhbGciOiJSUzI1NiJ9.\(b64).sig"
    }

    private func login(exp: Date = Date().addingTimeInterval(86400)) -> Data {
        let id = jwt(["email": "a@example.com", "sub": "auth0|x",
                      "https://api.openai.com/auth": ["chatgpt_account_id": "ws-1", "chatgpt_user_id": "user-1",
                                                      "chatgpt_plan_type": "plus"]])
        let access = jwt(["exp": exp.timeIntervalSince1970])
        let root: [String: Any] = ["auth_mode": "chatgpt", "OPENAI_API_KEY": NSNull(), "last_refresh": "2026-11-05T07:02:51Z",
                                   "tokens": ["id_token": id, "access_token": access, "refresh_token": "rt", "account_id": "ws-1"]]
        return try! JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted])
    }

    func testIdentityFromTheIdToken() {
        let who = CodexLogin.identity(in: login())
        XCTAssertEqual(who, CodexLogin.Identity(id: "user-1:ws-1", email: "a@example.com", plan: "Plus"))
    }

    func testAnAPIKeyLoginHasNoAccount() {
        let data = Data(#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x","tokens":null}"#.utf8)
        XCTAssertNil(CodexLogin.identity(in: data))
    }

    func testAnExpiredAccessTokenIsNotUsed() {
        XCTAssertEqual(CodexLogin.access(in: login())?.workspace, "ws-1")
        XCTAssertNil(CodexLogin.access(in: login(exp: Date().addingTimeInterval(-10))), "never renewed, so skipped")
    }

    func testUsageAsGroups() {
        let body = #"""
        {"plan_type":"pro","rate_limit":{"allowed":true,"primary_window":{"used_percent":42,"limit_window_seconds":18000,"reset_at":1795593916},
        "secondary_window":{"used_percent":7.5,"limit_window_seconds":604800,"reset_after_seconds":3600}},
        "code_review_rate_limit":null,"additional_rate_limits":[{"limit_name":"GPT-5.5 Spark","rate_limit":{"primary_window":{"used_percent":100,"limit_window_seconds":2592000}}}]}
        """#
        let now = Date(timeIntervalSince1970: 1_000_000)
        let read = CodexLogin.usage(from: Data(body.utf8), now: now)
        XCTAssertEqual(read.plan, "Pro")
        let groups = read.usage?.groups ?? []
        XCTAssertEqual(groups.map(\.name), ["Codex", "GPT-5.5 Spark"])
        XCTAssertEqual(groups[0].bars.map(\.label), ["5h", "Week"])
        XCTAssertEqual(groups[0].bars[0].window, UsageWindow(percent: 42, resetsAt: Date(timeIntervalSince1970: 1795593916)))
        XCTAssertEqual(groups[0].bars[1].window.resetsAt, now.addingTimeInterval(3600))
        XCTAssertEqual(groups[1].bars.map(\.label), ["Month"])
    }

    func testWindowLabels() {
        XCTAssertEqual(CodexLogin.windowLabel(seconds: 2_592_000), "Month")
        XCTAssertEqual(CodexLogin.windowLabel(seconds: 3600), "1h")
        XCTAssertEqual(CodexLogin.windowLabel(seconds: 86400 * 3), "3 days")
        XCTAssertEqual(CodexLogin.planName("team_plus"), "Team Plus")
    }

    func testSwitchWarning() {
        XCTAssertNil(CodexStore.switchWarning((app: false, server: false, sessions: 0)))
        XCTAssertEqual(CodexStore.switchWarning((app: false, server: true, sessions: 2)),
                       "Codex's background server restarts, so its running tasks stop. 2 Codex sessions are open in terminals. They keep the old account until you start them again.")
    }
}
