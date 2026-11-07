import Foundation

/// Notchling's hook requests, `requests/<id>.json`, read-only. Console never writes a response in P1:
/// that file protocol belongs to Notchling and a second answerer would race it.
public struct PendingPromptReader {
    private let requestsDir: URL
    private let isAlive: (Int32) -> Bool

    public init(requestsDir: URL, isAlive: @escaping (Int32) -> Bool = isProcessAlive) {
        self.requestsDir = requestsDir
        self.isAlive = isAlive
    }

    /// Oldest prompt per session id. Requests whose hook process is gone are stale and ignored.
    public func prompts() -> [String: PendingPrompt] {
        var all: [PendingPrompt] = []
        for url in JSONFile.contents(of: requestsDir)
        where url.pathExtension == "json" && !url.lastPathComponent.hasPrefix(".") {
            guard let object = JSONFile.object(at: url) else { continue }
            guard let prompt = Self.prompt(from: object, isAlive: isAlive) else {
                ConsoleLog.logger.debug("prompts \(url.path, privacy: .public): not a live prompt, skipped")
                continue
            }
            all.append(prompt)
        }
        var bySession: [String: PendingPrompt] = [:]
        for prompt in all.sorted(by: { $0.created < $1.created }) where bySession[prompt.sessionId] == nil {
            bySession[prompt.sessionId] = prompt
        }
        return bySession
    }

    static func prompt(from object: [String: Any], isAlive: (Int32) -> Bool) -> PendingPrompt? {
        guard let id = object["id"] as? String,
              let sessionId = object["session_id"] as? String,
              let pid = (object["pid"] as? NSNumber)?.int32Value,
              let event = object["hook_event_name"] as? String,
              isAlive(pid) else { return nil }
        let toolName = object["tool_name"] as? String ?? ""
        let input = object["tool_input"] as? [String: Any] ?? [:]
        let created = Date(timeIntervalSince1970: object["created"] as? Double ?? 0)

        if event == "PreToolUse" {
            guard let questions = questions(from: input), !questions.isEmpty else { return nil }
            return PendingPrompt(id: id, sessionId: sessionId, kind: .question(questions), toolName: toolName,
                                 summary: questions[0].question, created: created)
        }
        return PendingPrompt(id: id, sessionId: sessionId, kind: .permission, toolName: toolName,
                             summary: describe(input), created: created)
    }

    private static func questions(from input: [String: Any]) -> [PromptQuestion]? {
        guard let raw = input["questions"] as? [[String: Any]] else { return nil }
        return raw.map { item in
            PromptQuestion(
                header: item["header"] as? String ?? "",
                question: item["question"] as? String ?? "",
                options: (item["options"] as? [[String: Any]] ?? []).compactMap { $0["label"] as? String },
                multiSelect: item["multiSelect"] as? Bool ?? false)
        }
    }

    private static func describe(_ input: [String: Any]) -> String {
        if let command = input["command"] as? String { return command }
        if let path = input["file_path"] as? String ?? input["notebook_path"] as? String { return path }
        if let url = input["url"] as? String { return url }
        if let pattern = input["pattern"] as? String { return pattern }
        if let text = input["description"] as? String { return text }
        let json = (try? JSONSerialization.data(withJSONObject: input, options: [.sortedKeys]))
            .flatMap { String(data: $0, encoding: .utf8) } ?? ""
        return String(json.prefix(300))
    }
}
