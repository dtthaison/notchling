import Foundation

/// Bridges Claude Code hooks and the app through files in ~/Library/Application Support/Notchling:
///   requests/<id>.json   written by the hook: a question or permission prompt waiting on you
///   responses/<id>.json  written by the app: your decision, which the hook hands back to Claude Code
///   heartbeat            touched by the app while it runs; hooks do nothing when it's stale
enum HookBridge {
    static let dir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Notchling")
    static let requests = dir.appendingPathComponent("requests")
    static let responses = dir.appendingPathComponent("responses")
    static let heartbeat = dir.appendingPathComponent("heartbeat")

    /// How long a hook waits for a decision before falling back to the terminal prompt.
    static let waitSeconds: TimeInterval = 290

    static func prepare() {
        for d in [requests, responses] {
            try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        }
    }

    static func beat() {
        prepare()
        FileManager.default.createFile(atPath: heartbeat.path, contents: Data("\(getpid())".utf8))
    }

    static var appIsRunning: Bool {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: heartbeat.path),
              let modified = attrs[.modificationDate] as? Date else { return false }
        return Date().timeIntervalSince(modified) < 15
    }

    static func readJSON(_ url: URL) -> [String: Any]? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    /// Writes then renames, so the reader never sees half a file.
    static func writeJSON(_ obj: [String: Any], to url: URL) {
        guard let data = try? JSONSerialization.data(withJSONObject: obj) else { return }
        let tmp = url.deletingLastPathComponent().appendingPathComponent(".\(url.lastPathComponent).tmp")
        guard (try? data.write(to: tmp)) != nil else { return }
        if rename(tmp.path, url.path) != 0 { try? data.write(to: url) }
    }

    // MARK: - Hook side (`Notchling --hook`, run by Claude Code)

    /// Reads the hook event from stdin, waits for a decision from the app, prints it. Exits at once
    /// with no output (Claude Code's normal prompt) when the app isn't running, and for `claude -p` runs.
    static func runHook() -> Never {
        let input = FileHandle.standardInput.readDataToEndOfFile()
        guard appIsRunning,
              let event = (try? JSONSerialization.jsonObject(with: input)) as? [String: Any],
              let name = event["hook_event_name"] as? String,
              name == "PermissionRequest" || (name == "PreToolUse" && event["tool_name"] as? String == "AskUserQuestion"),
              !isHeadlessSession() else { exit(0) }

        prepare()
        let id = UUID().uuidString
        var request = event
        request["id"] = id
        request["pid"] = Int(getpid())
        request["created"] = Date().timeIntervalSince1970
        let requestURL = requests.appendingPathComponent("\(id).json")
        let responseURL = responses.appendingPathComponent("\(id).json")
        writeJSON(request, to: requestURL)

        // Claude Code gave up on the hook; the app clears requests whose process has gone.
        signal(SIGTERM) { _ in exit(0) }
        let deadline = Date().addingTimeInterval(waitSeconds)
        var response: [String: Any]?
        while Date() < deadline, appIsRunning {
            if let r = readJSON(responseURL) { response = r; break }
            usleep(200_000)
        }
        try? FileManager.default.removeItem(at: requestURL)
        try? FileManager.default.removeItem(at: responseURL)

        guard let response, let output = output(for: response, event: event),
              let data = try? JSONSerialization.data(withJSONObject: output) else { exit(0) }
        FileHandle.standardOutput.write(data)
        exit(0)
    }

    private static func output(for response: [String: Any], event: [String: Any]) -> [String: Any]? {
        let input = event["tool_input"] as? [String: Any] ?? [:]
        switch response["action"] as? String {
        case "answer":
            // updatedInput replaces the whole input, so the questions go back with the answers.
            var updated = input
            updated["answers"] = response["answers"] as? [String: String] ?? [:]
            return ["hookSpecificOutput": ["hookEventName": "PreToolUse",
                                           "permissionDecision": "allow",
                                           "updatedInput": updated]]
        case "allow":
            var decision: [String: Any] = ["behavior": "allow"]
            if response["always"] as? Bool == true {
                decision["updatedPermissions"] = alwaysRules(event)
            }
            return ["hookSpecificOutput": ["hookEventName": "PermissionRequest", "decision": decision]]
        case "deny":
            return ["hookSpecificOutput": ["hookEventName": "PermissionRequest",
                                           "decision": ["behavior": "deny",
                                                        "message": response["message"] as? String ?? "Denied from the notch."]]]
        default:
            return nil  // "pass": show the normal terminal prompt
        }
    }

    /// The suggestions behind "Always allow": new rules and directories, never a change of mode.
    static func alwaysRules(_ event: [String: Any]) -> [[String: Any]] {
        (event["permission_suggestions"] as? [[String: Any]] ?? []).filter { $0["type"] as? String != "setMode" }
    }

    /// True for `claude -p` runs, which nobody is watching; they keep their normal behaviour.
    private static func isHeadlessSession() -> Bool {
        let sessions = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude/sessions")
        var pid = getppid()
        for _ in 0..<6 where pid > 1 {
            if let s = readJSON(sessions.appendingPathComponent("\(pid).json")) {
                return s["entrypoint"] as? String == "sdk-cli"
            }
            pid = parentPID(of: pid) ?? 1
        }
        return false
    }

    static func parentPID(of pid: pid_t) -> pid_t? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        return info.kp_eproc.e_ppid
    }

    // MARK: - App side

    /// Requests whose hook is still waiting, oldest first. Clears ones whose hook has gone.
    static func pendingRequests() -> [HookRequest] {
        let files = (try? FileManager.default.contentsOfDirectory(at: requests, includingPropertiesForKeys: nil)) ?? []
        var result: [HookRequest] = []
        for url in files where url.pathExtension == "json" && !url.lastPathComponent.hasPrefix(".") {
            guard let obj = readJSON(url), let request = HookRequest(obj) else { continue }
            if kill(request.pid, 0) != 0 && errno != EPERM {
                try? FileManager.default.removeItem(at: url)
                continue
            }
            result.append(request)
        }
        return result.sorted { $0.created < $1.created }
    }

    static func respond(to request: HookRequest, _ response: [String: Any]) {
        prepare()
        writeJSON(response, to: responses.appendingPathComponent("\(request.id).json"))
    }

    // MARK: - Installing the hooks

    private static let settingsURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude/settings.json")
    private static let marker = "/Contents/MacOS/Notchling' --hook"
    /// What the hook looked like when the app was called Awake; removed when installing.
    private static let oldMarker = "/Contents/MacOS/Awake' --hook"

    static var hookCommand: String {
        "'" + Bundle.main.bundlePath + "/Contents/MacOS/Notchling' --hook"
    }

    static var isInstalled: Bool {
        (try? String(contentsOf: settingsURL, encoding: .utf8))?.contains(marker) ?? false
    }

    /// Adds (or removes) Notchling's PreToolUse(AskUserQuestion) and PermissionRequest hooks, leaving
    /// every other setting and hook as it was. Keeps the previous file as settings.json.notchling-backup.
    static func setInstalled(_ install: Bool) throws {
        let data = (try? Data(contentsOf: settingsURL)) ?? Data("{}".utf8)
        guard var settings = (try JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return }
        try? data.write(to: settingsURL.appendingPathExtension("notchling-backup"))

        var hooks = settings["hooks"] as? [String: Any] ?? [:]
        for (event, matcher) in [("PreToolUse", "AskUserQuestion"), ("PermissionRequest", "*")] {
            var groups = (hooks[event] as? [[String: Any]] ?? []).filter { group in
                !((group["hooks"] as? [[String: Any]]) ?? []).contains { hook in
                    let command = hook["command"] as? String ?? ""
                    return command.contains(marker) || command.contains(oldMarker)
                }
            }
            if install {
                groups.append(["matcher": matcher,
                               "hooks": [["type": "command", "command": hookCommand, "timeout": Int(waitSeconds) + 10]]])
            }
            hooks[event] = groups.isEmpty ? nil : groups
        }
        settings["hooks"] = hooks.isEmpty ? nil : hooks

        let out = try JSONSerialization.data(withJSONObject: settings, options: [.prettyPrinted, .withoutEscapingSlashes])
        try out.write(to: settingsURL, options: .atomic)
    }
}

/// A question or permission prompt a hook is holding open for the notch.
struct HookRequest: Equatable {
    enum Kind: Equatable { case question(PendingQuestion), permission }

    let id: String
    let sessionID: String
    let pid: pid_t
    let created: Double
    let kind: Kind
    let toolName: String
    let summary: String
    let canAlwaysAllow: Bool

    init?(_ obj: [String: Any]) {
        guard let id = obj["id"] as? String, let session = obj["session_id"] as? String,
              let pid = (obj["pid"] as? NSNumber)?.int32Value,
              let event = obj["hook_event_name"] as? String else { return nil }
        self.id = id
        sessionID = session
        self.pid = pid
        created = obj["created"] as? Double ?? 0
        toolName = obj["tool_name"] as? String ?? ""
        let input = obj["tool_input"] as? [String: Any] ?? [:]
        canAlwaysAllow = !HookBridge.alwaysRules(obj).isEmpty

        if event == "PreToolUse" {
            guard let q = TranscriptReader.question(id: "hook:\(id)", input: input) else { return nil }
            kind = .question(q)
            summary = q.items.first?.question ?? ""
        } else {
            kind = .permission
            summary = Self.describe(input)
        }
    }

    var question: PendingQuestion? {
        if case .question(let q) = kind { return q }
        return nil
    }

    private static func describe(_ input: [String: Any]) -> String {
        if let command = input["command"] as? String { return command }
        if let path = input["file_path"] as? String ?? input["notebook_path"] as? String { return path }
        if let url = input["url"] as? String { return url }
        if let pattern = input["pattern"] as? String { return pattern }
        if let d = input["description"] as? String { return d }
        let json = (try? JSONSerialization.data(withJSONObject: input, options: [.sortedKeys]))
            .flatMap { String(data: $0, encoding: .utf8) } ?? ""
        return String(json.prefix(300))
    }
}
