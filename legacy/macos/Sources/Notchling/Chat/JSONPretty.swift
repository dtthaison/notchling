import Foundation

/// Re-indents JSON without reordering anything: keys stay in the order the writer put them (what
/// `JSONSerialization.sortedKeys` can't do), strings and numbers are copied byte for byte, and text
/// that isn't a JSON object or array is left to the caller.
enum JSONPretty {
    private static let cache = NSCache<NSString, NSString>()

    /// The text as 2-space indented JSON, or nil when it isn't a JSON object or array.
    static func format(_ text: String) -> String? {
        let key = text as NSString
        if let hit = cache.object(forKey: key) { return hit.length == 0 ? nil : hit as String }
        let result = reindent(text)
        cache.setObject((result ?? "") as NSString, forKey: key)
        return result
    }

    /// `format`, or the text itself when it doesn't parse.
    static func formatted(_ text: String) -> String { format(text) ?? text }

    /// Whether a fenced block should offer the Pretty/Raw toggle: labelled json, or unlabelled
    /// content that parses as an object or array.
    static func applies(language: String, text: String) -> Bool {
        let lang = language.lowercased()
        guard lang == "json" || lang.isEmpty else { return false }
        return format(text) != nil
    }

    private static func reindent(_ text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let first = trimmed.first, first == "{" || first == "[",
              let data = trimmed.data(using: .utf8),
              (try? JSONSerialization.jsonObject(with: data)) != nil else { return nil }

        var out = String()
        out.reserveCapacity(trimmed.utf8.count * 2)
        var depth = 0
        var inString = false
        var escaped = false
        // An opener whose line break waits to see whether it closes at once ({} and []).
        var pendingOpen: Character?
        var previous: Character = " "

        func newline() {
            out.append("\n")
            out.append(String(repeating: "  ", count: depth))
        }

        for c in trimmed {
            if inString {
                out.append(c)
                if escaped { escaped = false } else if c == "\\" { escaped = true } else if c == "\"" { inString = false }
                continue
            }
            if c.isWhitespace { continue }
            // JSONSerialization lets a trailing comma through; JSON doesn't.
            if (c == "}" || c == "]") && previous == "," { return nil }
            previous = c
            if let open = pendingOpen {
                pendingOpen = nil
                if (open == "{" && c == "}") || (open == "[" && c == "]") {
                    depth -= 1
                    out.append(c)
                    continue
                }
                newline()
            }
            switch c {
            case "\"": inString = true; out.append(c)
            case "{", "[": out.append(c); depth += 1; pendingOpen = c
            case "}", "]": depth -= 1; newline(); out.append(c)
            case ",": out.append(","); newline()
            case ":": out.append(": ")
            default: out.append(c)
            }
        }
        return out
    }
}
