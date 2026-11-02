import Foundation

/// Publishes to an Artifact link that another saved account owns, with that account's saved login, so
/// Claude Code can update the page without switching accounts. claude.ai lets only the owner (or an editor
/// in reach of its organization) publish to a link; a personal account can't share edit access at all.
///
/// Runs in the `PreToolUse(Artifact)` hook. It handles a plain single-file HTML publish (with `edits` too)
/// and sends the file as written, the way Claude Code does for an ordinary page; anything else (supporting
/// files, capabilities, a contract, Markdown) is left to Claude Code.
enum OwnerPublish {
    enum Outcome {
        /// Published; the text tells Claude what happened.
        case done(String)
        /// Tried and failed; the real publish runs, with this as context.
        case failed(String)
        /// Stopped on purpose; the publish doesn't run, and the text says why.
        case refused(String)
    }

    private static let unsupported = ["asset", "files", "file_paths", "root", "type_url", "from_url", "capabilities", "contract"]

    static func run(_ event: [String: Any]) -> Outcome? {
        let input = event["tool_input"] as? [String: Any] ?? [:]
        let action = input["action"] as? String
        guard action == nil || action == "publish",
              let link = input["url"] as? String, let id = PageLibrary.artifactID(in: link), id.count == 22,
              let path = input["file_path"] as? String, (path as NSString).pathExtension.lowercased() == "html",
              unsupported.allSatisfy({ input[$0] == nil }),
              let page = PageLibrary.entries().first(where: { $0.artifactID == id }), !page.account.isEmpty,
              let owner = AccountStore.savedAccountToken(email: page.account) else { return nil }

        let cwd = event["cwd"] as? String ?? ""
        let base = cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser : URL(fileURLWithPath: cwd)
        let source = URL(fileURLWithPath: path, relativeTo: base).standardizedFileURL
        guard var text = try? String(contentsOf: source, encoding: .utf8) else { return nil }
        let edits = input["edits"] as? [[String: Any]] ?? []
        for edit in edits {
            // Claude Code reports a bad edit itself; leave it to that.
            guard let old = edit["old_str"] as? String, let new = edit["new_str"] as? String, !old.isEmpty,
                  text.components(separatedBy: old).count == 2 else { return nil }
            text = text.replacingOccurrences(of: old, with: new)
        }

        let slug = uuid(fromShortID: id)
        let perm = request("GET", "/api/frame/perm/\(slug)?org=\(owner.org)", token: owner.token)
        guard perm.status == 200, let live = perm.json?["live"] as? String else {
            return .failed("Notchling couldn't publish this as \(page.account), the page's owner: reading the page failed (HTTP \(perm.status)\(perm.message)).")
        }
        // Like Claude Code's own guard: don't overwrite a version published elsewhere (another Mac, or a save
        // from the page itself) that this file wasn't built on, unless the publish says force.
        if !page.version.isEmpty, live != page.version, input["force"] as? Bool != true {
            return .refused("Not published: \(page.url) has a newer version on claude.ai (\(live)) than the one last published "
                + "from this Mac (\(page.version)), so publishing this file would overwrite it. Notchling's local copy is the older "
                + "version. Ask the user whether to overwrite it; if so, publish again with force: true.")
        }
        var body: [String: Any] = ["slug": slug, "baseVersion": live, "content": text,
                                   "title": PageLibrary.pageTitle(Data(text.utf8), ext: "html", fallback: page.title)]
        for key in ["description", "label"] { if let v = input[key] as? String { body[key] = v } }
        if input["force"] as? Bool == true { body["force"] = true }
        let result = request("POST", "/api/frame/deploy/direct", token: owner.token, body: body)
        guard result.status == 200, let version = result.json?["version"] as? String else {
            return .failed("Notchling couldn't publish this as \(page.account), the page's owner (HTTP \(result.status)\(result.message)).")
        }
        if !edits.isEmpty { try? text.write(to: source, atomically: true, encoding: .utf8) }

        // Keep the local copy current, as a normal publish would.
        var saved = event
        saved["hook_event_name"] = "PostToolUse"
        saved["tool_response"] = "Published \(path) at \(page.url) (version \(version))"
        PageLibrary.handle(saved)

        let active = activeEmail()
        return .done("Done, not an error: Notchling published \(path) to \(page.url) as \(page.account), the page's owner"
            + (active.isEmpty ? "" : " (Claude Code is signed in as \(active), which can't publish to it)")
            + (version == live ? ". claude.ai already had exactly this content (version \(version))" : ". New version \(version), over \(live)")
            + (edits.isEmpty ? "." : "; the edits were applied to the file too.")
            + " Don't publish it again. Notchling's local copy is updated.")
    }

    /// A 22-character link id is the artifact's UUID in base58, which is what the API takes.
    static func uuid(fromShortID id: String) -> String {
        let alphabet = Array("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz")
        var bytes = [UInt8](repeating: 0, count: 16)
        for ch in id {
            guard var carry = alphabet.firstIndex(of: ch) else { return id }
            for i in stride(from: 15, through: 0, by: -1) {
                carry += Int(bytes[i]) * 58
                bytes[i] = UInt8(carry & 0xff)
                carry >>= 8
            }
        }
        let hex = bytes.map { String(format: "%02x", $0) }.joined()
        let h = Array(hex)
        return [0..<8, 8..<12, 12..<16, 16..<20, 20..<32].map { String(h[$0]) }.joined(separator: "-")
    }

    private static func activeEmail() -> String {
        if let lane = LaneStore.current() { return lane.email }
        let claude = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
        return (HookBridge.readJSON(claude)?["oauthAccount"] as? [String: Any])?["emailAddress"] as? String ?? ""
    }

    /// One call to Claude's artifact API (the host Claude Code's Artifact tool uses), waiting up to 50 s.
    private static func request(_ method: String, _ path: String, token: String,
                                body: [String: Any]? = nil) -> (status: Int, json: [String: Any]?, message: String) {
        guard let url = URL(string: "https://api.anthropic.com" + path) else { return (0, nil, "") }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.timeoutInterval = 50
        for (k, v) in ["Authorization": "Bearer \(token)", "anthropic-beta": "oauth-2025-04-20", "Accept": "application/json",
                       "X-Frame-CP": "go", "X-Frame-Surface": "code", "X-Frame-Platform": "cli", "User-Agent": "Notchling"] {
            request.setValue(v, forHTTPHeaderField: k)
        }
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        }
        let done = DispatchSemaphore(value: 0)
        var result: (Int, [String: Any]?, String) = (0, nil, ": no answer")
        URLSession.shared.dataTask(with: request) { data, response, error in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            let json = data.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
            var text = data.map { String(decoding: $0.prefix(300), as: UTF8.self) } ?? error?.localizedDescription ?? ""
            text = text.trimmingCharacters(in: .whitespacesAndNewlines)
            result = (status, json, text.isEmpty || status == 200 ? "" : ": \(text)")
            done.signal()
        }.resume()
        done.wait()
        return result
    }
}
