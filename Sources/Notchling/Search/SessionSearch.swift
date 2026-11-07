import Foundation

// The pure half of conversation search: what text a transcript line contributes to the index, how
// a query matches and ranks sessions, and the snippet shown around a match. The index that holds
// the sessions and reads the files is `SessionIndex`.

extension SessionTool {
    /// The key stored in the index and its cache file.
    var key: String {
        switch self {
        case .claude: return "claude"
        case .codex: return "codex"
        case .antigravity: return "agy"
        }
    }

    init?(key: String) {
        switch key {
        case "claude": self = .claude
        case "codex": self = .codex
        case "agy": self = .antigravity
        default: return nil
        }
    }
}

/// One conversation in the index: what search shows and what Chat needs to open it.
struct SessionMeta: Codable, Equatable {
    /// `<tool>:<session id>`, unique across tools.
    var key: String
    var toolKey: String
    var sessionID: String
    var title: String
    var folder: String
    /// The transcript or rollout file; empty for Antigravity, whose steps aren't read.
    var path: String
    /// The Claude Code config folder it was found in when that's a lane; nil for ~/.claude.
    var configDir: String?
    /// When the conversation last changed (seconds since 1970).
    var modified: Double
    /// The file's size when it was indexed; with `modified`, tells whether it needs reading again.
    var size: Int64 = 0

    var tool: SessionTool { SessionTool(key: toolKey) ?? .claude }
    var date: Date { Date(timeIntervalSince1970: modified) }
}

enum SessionRole: Equatable { case user, assistant }

/// A transcript line as far as search is concerned.
enum TranscriptLine: Equatable {
    case message(SessionRole, String, cwd: String?)
    case customTitle(String)
    case aiTitle(String)
}

enum TranscriptText {
    /// Lines longer than this are skipped, like `TranscriptReader` does (they hold base64 images).
    static let maxLineBytes = 100_000
    /// One message contributes at most this much, so a pasted log can't crowd out the rest.
    static let maxMessageChars = 6_000

    private static let markers = ["\"role\":\"user\"", "\"role\":\"assistant\"", "\"customTitle\"", "\"aiTitle\""]
        .map { Data($0.utf8) }

    /// A byte search that doesn't copy: most lines of a transcript are tool output and fail it.
    private static func contains(_ data: Data, _ needle: Data) -> Bool {
        data.withUnsafeBytes { h in
            needle.withUnsafeBytes { n in
                guard let hb = h.baseAddress, let nb = n.baseAddress else { return false }
                return memmem(hb, h.count, nb, n.count) != nil
            }
        }
    }

    /// A Claude Code transcript line: what you typed and what Claude said (text only, no thinking or
    /// tool calls), and the titles. Meta lines, sidechains and the slash-command and system text Claude
    /// Code stores as user lines are left out.
    static func claude(_ line: Data) -> TranscriptLine? {
        guard line.count <= maxLineBytes, markers.contains(where: { contains(line, $0) }),
              let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any] else { return nil }
        switch obj["type"] as? String {
        case "custom-title":
            return (obj["customTitle"] as? String).flatMap { $0.isEmpty ? nil : .customTitle($0) }
        case "ai-title":
            return (obj["aiTitle"] as? String).flatMap { $0.isEmpty ? nil : .aiTitle($0) }
        case "user", "assistant":
            break
        default:
            return nil
        }
        guard obj["isSidechain"] as? Bool != true, obj["isMeta"] as? Bool != true,
              let message = obj["message"] as? [String: Any],
              let roleName = message["role"] as? String else { return nil }
        let role: SessionRole = roleName == "assistant" ? .assistant : .user
        let text: String
        if let s = message["content"] as? String {
            text = s
        } else if let parts = message["content"] as? [[String: Any]] {
            text = parts.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : nil }.joined(separator: "\n")
        } else {
            return nil
        }
        guard let clean = clean(text, role: role) else { return nil }
        return .message(role, clean, cwd: obj["cwd"] as? String)
    }

    /// A Codex rollout line: `response_item` messages, leaving out the developer prompt and the
    /// environment and AGENTS.md text Codex injects as user messages.
    static func codex(_ line: Data) -> TranscriptLine? {
        guard line.count <= maxLineBytes, contains(line, Data("\"message\"".utf8)),
              let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any],
              obj["type"] as? String == "response_item",
              let payload = obj["payload"] as? [String: Any],
              payload["type"] as? String == "message" else { return nil }
        switch payload["role"] as? String {
        case "user":
            return CodexSessions.userText(payload).flatMap { clean($0, role: .user) }.map { .message(.user, $0, cwd: nil) }
        case "assistant":
            let text = (payload["content"] as? [[String: Any]] ?? [])
                .compactMap { $0["type"] as? String == "output_text" ? $0["text"] as? String : nil }
                .joined(separator: "\n")
            return clean(text, role: .assistant).map { .message(.assistant, $0, cwd: nil) }
        default:
            return nil
        }
    }

    /// Trimmed and capped; nil for text that isn't something the conversation said.
    static func clean(_ text: String, role: SessionRole) -> String? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return nil }
        if role == .user {
            // Slash commands, their output, caveats, system reminders and hook text are wrapped in tags.
            if t.hasPrefix("<") || t.hasPrefix("Caveat:") || t.hasPrefix("[Request interrupted") { return nil }
        }
        return t.count > maxMessageChars ? String(t.prefix(maxMessageChars)) : t
    }
}

/// A conversation's text, gathered line by line.
struct SessionTextBuilder {
    /// Enough for the gist of a long session; the newest text is kept.
    static let maxChars = 40_000

    private(set) var messages: [String] = []
    private(set) var firstPrompt: String?
    private(set) var cwd: String?
    private(set) var customTitle: String?
    private(set) var aiTitle: String?

    mutating func add(_ line: TranscriptLine) {
        switch line {
        case .message(let role, let text, let folder):
            if firstPrompt == nil, role == .user { firstPrompt = text }
            if cwd == nil, let folder, !folder.isEmpty { cwd = folder }
            messages.append(text)
        case .customTitle(let t): customTitle = t
        case .aiTitle(let t): aiTitle = t
        }
    }

    /// What you named it, else Claude Code's own title, else the start of the first prompt.
    var title: String {
        if let customTitle { return customTitle }
        if let aiTitle { return aiTitle }
        return firstPrompt.map(CodexSessions.title) ?? "Session"
    }

    /// The messages joined with newlines, cut from the front to fit `maxChars`.
    var text: String {
        var kept: [String] = []
        var total = 0
        for message in messages.reversed() {
            if total + message.count > Self.maxChars {
                if kept.isEmpty { kept.append(String(message.suffix(Self.maxChars))) }
                break
            }
            kept.append(message)
            total += message.count + 1
        }
        return kept.reversed().joined(separator: "\n")
    }
}

// MARK: - Matching

/// A session ready to search: its text lowercased once, as UTF-8 bytes, so a key press costs a
/// byte search per word and no allocation.
final class IndexedSession {
    let meta: SessionMeta
    let text: String
    let titleLower: String
    let folderLower: String
    let lower: [UInt8]

    init(meta: SessionMeta, text: String) {
        self.meta = meta
        self.text = text
        titleLower = meta.title.lowercased()
        folderLower = meta.folder.lowercased()
        lower = Array(text.lowercased().utf8)
    }
}

struct SessionHit: Equatable {
    var meta: SessionMeta
    var snippet: String?
    var score: Double
}

enum SessionSearch {
    /// Recent sessions only: the same 14 days the Chat sidebar lists.
    static let recent: TimeInterval = 14 * 86400

    /// The query's words, lowercased.
    static func words(_ query: String) -> [String] {
        query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
    }

    /// Every word must appear in the title, the folder or the text. Title matches rank first, then a
    /// phrase match, how often the words occur, and how recently the session changed. Best first.
    static func rank(_ words: [String], in sessions: [IndexedSession], now: Date = Date(), limit: Int = 30) -> [SessionHit] {
        guard !words.isEmpty else { return [] }
        let needles = words.map { Array($0.utf8) }
        let phrase = words.count > 1 ? Array(words.joined(separator: " ").utf8) : nil
        var scored: [(session: IndexedSession, score: Double)] = []
        for session in sessions {
            guard let score = score(needles, phrase: phrase, session: session, now: now) else { continue }
            scored.append((session, score))
        }
        scored.sort { $0.score != $1.score ? $0.score > $1.score : $0.session.meta.modified > $1.session.meta.modified }
        return scored.prefix(limit).map { hit in
            SessionHit(meta: hit.session.meta, snippet: snippet(in: hit.session, words: words), score: hit.score)
        }
    }

    private static func score(_ needles: [[UInt8]], phrase: [UInt8]?, session: IndexedSession, now: Date) -> Double? {
        var score = 0.0
        var inTitle = 0
        for needle in needles {
            let word = String(decoding: needle, as: UTF8.self)
            let title = session.titleLower.contains(word)
            let folder = session.folderLower.contains(word)
            let hits = count(needle, in: session.lower, upTo: 5)
            guard title || folder || hits > 0 else { return nil }
            if title { score += 30; inTitle += 1 }
            if folder { score += 8 }
            if hits > 0 { score += 3 + Double(hits) * 2 }
        }
        if inTitle == needles.count { score += 20 }
        if let phrase {
            if session.titleLower.contains(String(decoding: phrase, as: UTF8.self)) { score += 30 }
            if find(phrase, in: session.lower) != nil { score += 12 }
        }
        let age = max(0, now.timeIntervalSince1970 - session.meta.modified)
        score += 10 * max(0, 1 - age / recent)
        return score
    }

    /// Where `needle` first occurs in `haystack`.
    static func find(_ needle: [UInt8], in haystack: [UInt8], from start: Int = 0) -> Int? {
        guard !needle.isEmpty, start >= 0, start + needle.count <= haystack.count else { return nil }
        return haystack.withUnsafeBufferPointer { h in
            needle.withUnsafeBufferPointer { n in
                guard let base = h.baseAddress, let nb = n.baseAddress,
                      let hit = memmem(base + start, h.count - start, nb, n.count) else { return nil }
                return base.distance(to: hit.assumingMemoryBound(to: UInt8.self))
            }
        }
    }

    static func count(_ needle: [UInt8], in haystack: [UInt8], upTo cap: Int) -> Int {
        var n = 0, at = 0
        while n < cap, let hit = find(needle, in: haystack, from: at) {
            n += 1
            at = hit + max(1, needle.count)
        }
        return n
    }

    // MARK: Snippet

    /// About `radius` characters either side of the best match in the text: the whole phrase if it
    /// occurs, else the first word that does. Nil when only the title or folder matched.
    static func snippet(in session: IndexedSession, words: [String], radius: Int = 56) -> String? {
        var position: Int?
        if words.count > 1 { position = find(Array(words.joined(separator: " ").utf8), in: session.lower) }
        if position == nil {
            position = words.compactMap { find(Array($0.utf8), in: session.lower) }.min()
        }
        guard let position else { return nil }
        // The original text when lowercasing kept the byte offsets; else the lowercased bytes.
        let original = Array(session.text.utf8)
        let bytes = original.count == session.lower.count ? original : session.lower
        // Bytes per character differ, so cut by bytes with room for multi-byte characters.
        var from = max(0, position - radius), to = min(bytes.count, position + radius * 2)
        while from > 0, from < bytes.count, bytes[from] & 0xC0 == 0x80 { from -= 1 }
        while to < bytes.count, bytes[to] & 0xC0 == 0x80 { to += 1 }
        var text = String(decoding: bytes[from..<to], as: UTF8.self)
        text = collapse(text)
        var out = text
        if from > 0 { out = "…" + String(out.drop(while: { !$0.isWhitespace }).drop(while: \.isWhitespace)) }
        if to < bytes.count {
            // Drop the word cut in half at the far end.
            if let space = out.lastIndex(where: \.isWhitespace) { out = String(out[..<space]) }
            out += "…"
        }
        return out
    }

    /// Newlines and runs of spaces become one space.
    static func collapse(_ text: String) -> String {
        var out = ""
        var space = false
        for ch in text {
            if ch.isWhitespace {
                if !space, !out.isEmpty { out.append(" ") }
                space = true
            } else {
                out.append(ch)
                space = false
            }
        }
        return out.trimmingCharacters(in: .whitespaces)
    }

    /// Where the query's words sit in a snippet, for drawing them bold.
    static func highlights(in snippet: String, words: [String]) -> [Range<String.Index>] {
        let lower = snippet.lowercased()
        guard lower.count == snippet.count else { return [] }
        var ranges: [Range<String.Index>] = []
        for word in Set(words) where !word.isEmpty {
            var from = lower.startIndex
            while let r = lower.range(of: word, range: from..<lower.endIndex) {
                let lo = snippet.index(snippet.startIndex, offsetBy: lower.distance(from: lower.startIndex, to: r.lowerBound))
                let hi = snippet.index(lo, offsetBy: lower.distance(from: r.lowerBound, to: r.upperBound))
                ranges.append(lo..<hi)
                from = r.upperBound
            }
        }
        return ranges.sorted { $0.lowerBound < $1.lowerBound }
    }
}

/// How long ago, in a word or two: "now", "5m", "3h", "2d".
enum SessionAge {
    static func text(from date: Date, now: Date = Date()) -> String {
        let s = max(0, now.timeIntervalSince(date))
        switch s {
        case ..<60: return "now"
        case ..<3600: return "\(Int(s / 60))m"
        case ..<86400: return "\(Int(s / 3600))h"
        case ..<(86400 * 14): return "\(Int(s / 86400))d"
        default: return "\(Int(s / (86400 * 7)))w"
        }
    }
}
