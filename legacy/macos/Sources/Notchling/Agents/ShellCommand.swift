import Foundation

/// A reply starting with "!" runs as a shell command in the session, as in Claude Code's prompt. Commands
/// are often copied from Claude's answer in a terminal, which wraps long lines and indents each
/// continuation by two spaces; pasted as is, the shell runs the pieces as separate, broken commands.
/// `normalize` puts such a command back together before it is sent.
enum ShellCommand {
    /// Sending trims leading spaces, so "  !ls" runs too.
    static func isCommand(_ text: String) -> Bool {
        text.drop { $0.isWhitespace }.hasPrefix("!")
    }

    /// The command as it should run, still starting with "!". Text that isn't a command comes back as is.
    /// Running it twice changes nothing.
    static func normalize(_ text: String) -> String {
        guard isCommand(text) else { return text }
        var body = String(text.drop { $0.isWhitespace }.dropFirst())
        body = plainCharacters(body.replacingOccurrences(of: "\r\n", with: "\n"))

        let lines = body.components(separatedBy: "\n")
            .filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("```") }
        // A heredoc's lines are data: indentation there is content, not a terminal's wrapping.
        let joinWrapped = !body.contains("<<")

        var result = ""
        for (index, raw) in lines.enumerated() {
            if index == 0 || result.isEmpty {
                result = withoutPrompt(raw.drop { $0.isWhitespace })
                continue
            }
            let line = Substring(raw)
            let continues = line.first?.isWhitespace == true && !line.allSatisfy(\.isWhitespace)
            if joinWrapped, continues, !result.hasSuffix("\\") {
                let rest = String(line.drop { $0.isWhitespace })
                if result.last?.isWhitespace == true {
                    result += rest                        // wrapped at a space, which stayed on the line
                } else if endsWithSeparator(result) {
                    result += " " + rest                  // a script's indented body: keep the words apart
                } else {
                    result += rest                        // wrapped in the middle of a word or path
                }
            } else {
                result += "\n" + withoutPrompt(line)
            }
        }
        let tidy = result.components(separatedBy: "\n")
            .map { $0.replacingOccurrences(of: "[ \\t]+$", with: "", options: .regularExpression) }
            .joined(separator: "\n")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return "!" + tidy
    }

    /// Typography that macOS or a web page puts in place of what the shell needs.
    private static func plainCharacters(_ s: String) -> String {
        var out = s
        for (from, to) in [("\u{201C}", "\""), ("\u{201D}", "\""), ("\u{201E}", "\""), ("\u{2018}", "'"),
                           ("\u{2019}", "'"), ("\u{2014}", "--"), ("\u{2013}", "--"), ("\u{00A0}", " "),
                           ("\u{2009}", " "), ("\u{202F}", " "), ("\u{200B}", "")] {
            out = out.replacingOccurrences(of: from, with: to)
        }
        return out
    }

    /// A pasted shell prompt ("$ git status", "% ls") isn't part of the command.
    private static func withoutPrompt(_ line: Substring) -> String {
        for prompt in ["$ ", "% "] where line.hasPrefix(prompt) { return String(line.dropFirst(prompt.count)) }
        return String(line)
    }

    /// Ends with a keyword or operator after which an indented line is the script's next part, not the
    /// rest of a wrapped word.
    private static func endsWithSeparator(_ text: String) -> Bool {
        guard let last = text.split(whereSeparator: \.isWhitespace).last else { return false }
        return ["then", "do", "else", "{", "(", "&&", "||", "|", ";"].contains(String(last))
    }
}
