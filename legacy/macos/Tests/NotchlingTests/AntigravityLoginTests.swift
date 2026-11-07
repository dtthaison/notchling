import XCTest
@testable import Notchling

final class AntigravityLoginTests: XCTestCase {
    /// The older format: the account is the id token's `sub`.
    func testIDTokenLogin() throws {
        let payload = try JSONSerialization.data(withJSONObject: ["sub": "1234", "email": "a@example.com"])
        let part = payload.base64EncodedString()
            .replacingOccurrences(of: "=", with: "").replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
        let login = #"{"auth_method":"oauth","id_token":"x.\#(part).y","token":{"access_token":"t"}}"#
        let account = AntigravityStore.account(in: login)
        XCTAssertEqual(account?.id, "1234")
        XCTAssertEqual(account?.email, "a@example.com")
    }

    /// The newer format agy writes names only the email.
    func testEmailOnlyLogin() {
        let login = #"{"token":{"access_token":"t","refresh_token":"r"},"auth_method":"keyring","id_token_email":"B@example.com"}"#
        let account = AntigravityStore.account(in: login)
        XCTAssertEqual(account?.id, AntigravityStore.emailIDPrefix + "b@example.com")
        XCTAssertEqual(account?.email, "B@example.com")
    }

    func testUnknownLogin() {
        XCTAssertNil(AntigravityStore.account(in: #"{"token":{}}"#))
        XCTAssertNil(AntigravityStore.account(in: "not json"))
    }
}
