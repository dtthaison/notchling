import CryptoKit
import Foundation
import Network

/// Signs in to a Claude account the way `claude auth login` does: OAuth with PKCE, and the browser
/// sends the code back to a listener on 127.0.0.1. Claude Code's own login isn't touched.
final class AccountLogin {
    /// The sign-in page; open it again or copy it into another browser window.
    private(set) var url: URL?

    private var listener: NWListener?
    private let verifier = AccountLogin.randomString()
    private let state = AccountLogin.randomString()
    private var port: UInt16 = 0
    private var done: ((Result<[String: Any], Error>) -> Void)?
    private var finished = false

    private static let authorizeURL = "https://claude.com/cai/oauth/authorize"
    private static let tokenURL = "https://platform.claude.com/v1/oauth/token"
    private static let successURL = "https://platform.claude.com/oauth/code/success?app=claude-code"
    private static let scopes = "org:create_api_key user:profile user:inference user:sessions:claude_code "
        + "user:mcp_servers user:file_upload user:plugins"

    /// Starts listening, then calls `ready` with the sign-in page. `done` gets the login as Claude Code
    /// stores it under `claudeAiOauth`. Both run on the main thread.
    func start(ready: @escaping (URL) -> Void, done: @escaping (Result<[String: Any], Error>) -> Void) {
        self.done = done
        do {
            let parameters = NWParameters.tcp
            parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
            let listener = try NWListener(using: parameters)
            self.listener = listener
            listener.stateUpdateHandler = { [weak self] state in
                guard let self else { return }
                switch state {
                case .ready:
                    guard let port = listener.port?.rawValue, let url = self.pageURL(port: port) else { return }
                    self.port = port
                    self.url = url
                    DispatchQueue.main.async { ready(url) }
                case .failed(let error):
                    self.finish(.failure(error))
                default:
                    break
                }
            }
            listener.newConnectionHandler = { [weak self] connection in self?.handle(connection) }
            listener.start(queue: .global(qos: .userInitiated))
        } catch {
            finish(.failure(error))
        }
    }

    func cancel() {
        finished = true
        listener?.cancel()
        listener = nil
    }

    private func pageURL(port: UInt16) -> URL? {
        var parts = URLComponents(string: Self.authorizeURL)
        parts?.queryItems = [
            URLQueryItem(name: "code", value: "true"),
            URLQueryItem(name: "client_id", value: AccountStore.clientID),
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "redirect_uri", value: "http://localhost:\(port)/callback"),
            URLQueryItem(name: "scope", value: Self.scopes),
            URLQueryItem(name: "code_challenge", value: Self.challenge(verifier)),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "state", value: state),
        ]
        return parts?.url
    }

    // MARK: Callback

    private func handle(_ connection: NWConnection) {
        connection.start(queue: .global(qos: .userInitiated))
        connection.receive(minimumIncompleteLength: 1, maximumLength: 16 * 1024) { [weak self] data, _, _, _ in
            guard let self else { connection.cancel(); return }
            let request = data.map { String(decoding: $0, as: UTF8.self) } ?? ""
            // "GET /callback?code=…&state=… HTTP/1.1"
            let target = request.split(separator: " ").dropFirst().first.map { String($0) } ?? ""
            let query = URLComponents(string: "http://localhost\(target)")
            let items = Dictionary((query?.queryItems ?? []).map { ($0.name, $0.value ?? "") }, uniquingKeysWith: { a, _ in a })
            guard query?.path == "/callback", let code = items["code"], items["state"] == self.state else {
                Self.reply(connection, status: "404 Not Found", location: nil)
                return
            }
            Self.reply(connection, status: "302 Found", location: Self.successURL)
            self.listener?.cancel()
            self.exchange(code)
        }
    }

    private static func reply(_ connection: NWConnection, status: String, location: String?) {
        var head = "HTTP/1.1 \(status)\r\nContent-Length: 0\r\nConnection: close\r\n"
        if let location { head += "Location: \(location)\r\n" }
        connection.send(content: Data((head + "\r\n").utf8), completion: .contentProcessed { _ in connection.cancel() })
    }

    private func exchange(_ code: String) {
        let body: [String: Any] = [
            "grant_type": "authorization_code", "code": code, "redirect_uri": "http://localhost:\(port)/callback",
            "client_id": AccountStore.clientID, "code_verifier": verifier, "state": state,
        ]
        guard let url = URL(string: Self.tokenURL) else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        guard let (status, json) = AccountStore.send(request) else {
            finish(.failure(AccountError("Couldn't reach Anthropic to finish signing in.")))
            return
        }
        guard status == 200, let json, let access = json["access_token"] as? String,
              let refresh = json["refresh_token"] as? String else {
            finish(.failure(AccountError("Sign-in was refused (\(status)). Try again.")))
            return
        }
        let now = Date().timeIntervalSince1970
        var oauth: [String: Any] = [
            "accessToken": access, "refreshToken": refresh,
            "expiresAt": Int((now + (json["expires_in"] as? Double ?? 3600)) * 1000),
            "scopes": (json["scope"] as? String ?? Self.scopes).split(separator: " ").map(String.init),
        ]
        if let s = json["refresh_token_expires_in"] as? Double { oauth["refreshTokenExpiresAt"] = Int((now + s) * 1000) }
        finish(.success(oauth))
    }

    private func finish(_ result: Result<[String: Any], Error>) {
        DispatchQueue.main.async {
            guard !self.finished else { return }
            self.finished = true
            self.listener?.cancel()
            self.done?(result)
        }
    }

    // MARK: PKCE

    private static func randomString() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        return base64URL(Data(bytes))
    }

    private static func challenge(_ verifier: String) -> String {
        base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
    }

    private static func base64URL(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}
