import Foundation

/// Answers a Notchling hook request by writing `responses/<request id>.json`, the file Notchling's
/// hook process is polling for. The shape is the one HookBridge.output(for:event:) reads:
///   {"action":"answer","answers":{"<question text>":"<option label>"}}
///   {"action":"allow"}   {"action":"deny","message":"..."}
public struct HookResponder {
    public enum Failure: Error, LocalizedError, Equatable {
        case notAnswerable(String)
        case writeFailed(String)

        public var errorDescription: String? {
            switch self {
            case .notAnswerable(let why): return why
            case .writeFailed(let why): return "Không ghi được câu trả lời: \(why)"
            }
        }
    }

    private let responsesDir: URL

    public init(responsesDir: URL) {
        self.responsesDir = responsesDir
    }

    public func allow(_ prompt: PendingPrompt) throws {
        guard prompt.kind == .permission else { throw Failure.notAnswerable("Đây là câu hỏi, không phải permission.") }
        try write(["action": "allow"], for: prompt)
    }

    public func deny(_ prompt: PendingPrompt, message: String = "Denied from Agent Console.") throws {
        guard prompt.kind == .permission else { throw Failure.notAnswerable("Đây là câu hỏi, không phải permission.") }
        try write(["action": "deny", "message": message], for: prompt)
    }

    /// `choices[i]` is the option label picked for `questions[i]`. Multi-select questions are the
    /// terminal's job, so they are refused here.
    public func answer(_ prompt: PendingPrompt, choices: [String]) throws {
        guard case .question(let questions) = prompt.kind else {
            throw Failure.notAnswerable("Đây là permission, không phải câu hỏi.")
        }
        guard !questions.contains(where: \.multiSelect) else {
            throw Failure.notAnswerable("Câu hỏi chọn nhiều đáp án: trả lời trên terminal.")
        }
        guard choices.count == questions.count else {
            throw Failure.notAnswerable("Thiếu câu trả lời.")
        }
        var answers: [String: String] = [:]
        for (question, choice) in zip(questions, choices) { answers[question.question] = choice }
        try write(["action": "answer", "answers": answers], for: prompt)
    }

    /// Writes then renames, so the hook never reads half a file (same as HookBridge.writeJSON).
    private func write(_ object: [String: Any], for prompt: PendingPrompt) throws {
        let url = responsesDir.appendingPathComponent("\(prompt.id).json")
        let temporary = responsesDir.appendingPathComponent(".\(prompt.id).json.tmp")
        do {
            try FileManager.default.createDirectory(at: responsesDir, withIntermediateDirectories: true)
            let data = try JSONSerialization.data(withJSONObject: object)
            try data.write(to: temporary)
            if rename(temporary.path, url.path) != 0 {
                throw CocoaError(.fileWriteUnknown)
            }
        } catch {
            ConsoleLog.error("HookResponder.write", url.path, "\(error)")
            throw Failure.writeFailed(error.localizedDescription)
        }
    }
}
