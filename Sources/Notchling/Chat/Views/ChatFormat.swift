import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

enum ChatFormat {
    /// "47s · ↓ 2.3k tokens", "3m 5s · ↓ 12k tokens".
    static func workStats(since start: Date, now: Date, tokens: Int) -> String {
        let s = max(0, Int(now.timeIntervalSince(start)))
        let time = s < 60 ? "\(s)s" : s < 3600 ? "\(s / 60)m \(s % 60)s" : "\(s / 3600)h \(s % 3600 / 60)m"
        guard tokens > 0 else { return time }
        let count = tokens < 1000 ? "\(tokens)"
            : tokens < 10_000 ? String(format: "%.1fk", Double(tokens) / 1000)
            : "\(tokens / 1000)k"
        return "\(time) · ↓ \(count) tokens"
    }

    static func openTitle(_ agent: Agent) -> String {
        switch agent.target {
        case .job: return "Open in Terminal"
        case .process: return "Open in \(agent.host.isEmpty || agent.host == "claude -p" ? "Terminal" : agent.host)"
        case .resume: return "Resume in Terminal"
        case .codex, .antigravity: return "Open in Terminal"
        }
    }

    static func openIcon(_ agent: Agent) -> KitIcon {
        if case .resume = agent.target { return .resume }
        return .terminal
    }

    static func openHelp(_ agent: Agent) -> String {
        switch agent.target {
        case .job: return "Attach to this background session in a terminal"
        case .process: return "Bring the session's terminal window forward"
        case .resume: return "Start this session again in a terminal with claude --resume"
        case .codex: return "Continue this Codex session in a terminal with codex resume"
        case .antigravity: return "Continue this Antigravity session in a terminal with agy --conversation"
        }
    }

    /// "now", "5m", "2h", "3d".
    static func age(_ date: Date) -> String {
        let s = max(0, Int(Date().timeIntervalSince(date)))
        if date == .distantPast { return "" }
        if s < 60 { return "now" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86_400 { return "\(s / 3600)h" }
        return "\(s / 86_400)d"
    }

    /// "12s", "4m 05s", "1h 12m".
    static func duration(_ seconds: Double) -> String {
        let s = Int(seconds.rounded())
        if s < 60 { return "\(max(s, 1))s" }
        if s < 3600 { return String(format: "%dm %02ds", s / 60, s % 60) }
        return "\(s / 3600)h \(s % 3600 / 60)m"
    }

    /// "claude-opus-5-5" -> "Opus 5.5".
    static func modelName(_ id: String) -> String {
        let parts = id.replacingOccurrences(of: "claude-", with: "").split(separator: "-")
        guard let family = parts.first else { return id }
        let version = parts.dropFirst().prefix { $0.allSatisfy(\.isNumber) && $0.count <= 2 }.joined(separator: ".")
        return family.prefix(1).uppercased() + family.dropFirst() + (version.isEmpty ? "" : " " + version)
    }

    /// "12.4k", "1.2M", "400k".
    static func tokens(_ n: Int) -> String {
        func short(_ v: Double, _ unit: String) -> String {
            var s = String(format: "%.1f", v)
            if s.hasSuffix(".0") { s.removeLast(2) }
            return s + unit
        }
        if n >= 1_000_000 { return short(Double(n) / 1_000_000, "M") }
        if n >= 1000 { return short(Double(n) / 1000, "k") }
        return "\(n)"
    }

    /// The model's context window in Claude Code, where it's known. Every current model has 1M (Claude Code's
    /// /context said "24.8k / 1m" for Opus 5.5 on 2026-11-14; it had said 400k earlier, which made the meter read 2.5x
    /// too full); Haiku 4.5 has 200k. Others give nil, and the header shows the count alone rather than guess.
    static func contextWindow(model: String?) -> Int? {
        guard let model else { return nil }
        if model.hasPrefix("claude-haiku-4-5") { return 200_000 }
        let million = ["claude-opus-5", "claude-sonnet-5", "claude-haiku-5", "claude-fable-5", "claude-mythos-5",
                       "claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6"]
        if million.contains(where: { model.hasPrefix($0) }) { return 1_000_000 }
        return nil
    }

    private static let timeFormat: DateFormatter = { let f = DateFormatter(); f.timeStyle = .short; f.dateStyle = .none; return f }()
    private static let dayFormat: DateFormatter = { let f = DateFormatter(); f.setLocalizedDateFormatFromTemplate("EEEE d MMMM"); return f }()

    static func time(_ date: Date) -> String { timeFormat.string(from: date) }

    static func day(_ date: Date) -> String {
        if Calendar.current.isDateInToday(date) { return "Today" }
        if Calendar.current.isDateInYesterday(date) { return "Yesterday" }
        return dayFormat.string(from: date)
    }
}
