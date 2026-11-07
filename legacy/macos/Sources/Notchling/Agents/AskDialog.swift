import Foundation

/// Reads Claude Code's AskUserQuestion dialog off a terminal screen (`TerminalScreen.lines`). A background
/// job writes the question into its transcript only once it's answered, and without the prompt hook nothing
/// else describes it, so this is where Chat gets the question, options and descriptions from. Checked on
/// 2.1.318 against `Fixtures/terminal/question-open.bin`:
///
///     ────────────────────────────
///     ←  ☐ Color  ☐ Fruit  ✔ Submit  →      (only with several questions)
///     Favourite color?
///     ❯ 1. Red
///          Red                                (an option's description)
///       2. Blue
///          Blue
///       3. Type something.
///     ────────────────────────────
///       4. Chat about this
///     Enter to select · Tab/Arrow keys to navigate · Esc to cancel
///
/// The screen shows one question at a time; the tab row names them all.
enum AskDialog {
    struct Page: Equatable {
        var question: String
        /// The option labels, without "Type something" and "Chat about this".
        var options: [String]
        /// What each option means, as Claude wrote it; "" when the row has no description.
        var details: [String]
        var multiSelect: Bool
        /// The question tabs' titles, without Submit. Empty when the dialog has a single question.
        var tabs: [String]
        /// The final "Review your answers" screen: nothing to read, "1" submits.
        var onReview = false
        /// The question's short title, when the page shows it over the question rather than in the tabs.
        var header = ""
    }

    private static let optionRow = try! NSRegularExpression(
        pattern: #"^(?:[❯>›]\s*)?(?:(\[[ xX✓✔]\]|[☐☑☒◻◼□■])\s*)?(\d+)\.\s+(.+?)\s*$"#)
    private static let checkbox = try! NSRegularExpression(pattern: #"^\[[ xX✓✔]\]\s*"#)
    private static let separator = try!NSRegularExpression(pattern: #"^[─━═\-]{8,}$"#)

    /// The footer every question page ends with.
    private static func footer(in rows: [String]) -> Int? {
        rows.lastIndex { $0.contains("Enter to select") || $0.contains("Esc to cancel") || $0.contains("Enter to confirm") }
    }

    /// The review screen that follows the last question of a multi-question or multi-select dialog.
    static func isReview(_ lines: [String]) -> Bool {
        lines.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.suffix(8)
            .contains { $0.contains("Submit answers") }
    }

    /// True when the dialog on screen is asking this question (matched on its start, since long text wraps).
    /// Only the dialog's own question counts: the same words are often higher up, in what Claude said before asking.
    static func shows(_ question: String, in lines: [String]) -> Bool {
        let wanted = normalized(question).prefix(48)
        guard !wanted.isEmpty, let page = page(in: lines), !page.onReview else { return false }
        return normalized(page.question).contains(wanted)
    }

    private static func normalized(_ text: String) -> String {
        text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    /// The question page on screen, or nil when there isn't a dialog with numbered options on it.
    static func page(in lines: [String]) -> Page? {
        let rows = lines.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        guard let end = footer(in: rows) else { return nil }
        let body = Array(rows[..<end])
        if isReview(lines) {
            return Page(question: "Review your answers", options: [], details: [], multiSelect: false,
                        tabs: tabs(in: body), onReview: true)
        }

        // The numbered rows, read upward from the last one: the options, then "Type something" and "Chat about this".
        let numbered: [(row: Int, number: Int, label: String, box: Bool)] = body.enumerated().compactMap { index, text in
            let range = NSRange(text.startIndex..., in: text)
            guard let match = optionRow.firstMatch(in: text, range: range),
                  let number = Range(match.range(at: 2), in: text).flatMap({ Int(text[$0]) }),
                  let label = Range(match.range(at: 3), in: text).map({ String(text[$0]) }) else { return nil }
            // A multi-select page puts its checkbox after the number: "1. [ ] Cat" (2.1.319).
            if let box = checkbox.firstMatch(in: label, range: NSRange(label.startIndex..., in: label)),
               let marked = Range(box.range, in: label) {
                return (index, number, String(label[marked.upperBound...]), true)
            }
            return (index, number, label, match.range(at: 1).location != NSNotFound)
        }
        guard let last = numbered.last else { return nil }
        var block = [last]
        for row in numbered.dropLast().reversed() {
            guard row.number == block[0].number - 1 else { continue }
            block.insert(row, at: 0)
            if row.number == 1 { break }
        }
        guard block.first?.number == 1 else { return nil }

        // Options end where "Type something" / "Other" / "Chat about this" begin.
        // A permission dialog is numbered too (Yes / No) but has neither of those rows.
        guard let extras = block.firstIndex(where: { isExtra($0.label) }) else { return nil }
        let options = Array(block[..<extras])
        guard !options.isEmpty else { return nil }
        var details: [String] = []
        for (i, option) in options.enumerated() {
            let next = i + 1 < block.count ? block[i + 1].row : body.count
            details.append(body[(option.row + 1)..<next]
                .filter { !isSeparator($0) }
                .joined(separator: " "))
        }

        // The question: the rows above the first option, back to the tab row or the dialog's top rule.
        var question: [String] = []
        for index in stride(from: options[0].row - 1, through: 0, by: -1) {
            let text = body[index]
            // Also the prompt line above a dialog that has no rule over it.
            if isSeparator(text) || isTabRow(text) || text.hasPrefix("❯") { break }
            question.insert(text, at: 0)
        }
        // A dialog with one question has no tab row; its header sits over the question as a chip ("☐ Size").
        var header = ""
        if question.count > 1, let chip = chipTitle(question[0]) {
            header = chip
            question.removeFirst()
        }
        guard !question.isEmpty else { return nil }
        return Page(question: question.joined(separator: " "), options: options.map(\.label), details: details,
                    multiSelect: options.contains { $0.box }, tabs: tabs(in: body), header: header)
    }

    private static let stateMarks = Set("☐☒☑✔✓□■")

    /// "☐ Size" gives "Size"; nil for any other row.
    private static func chipTitle(_ row: String) -> String? {
        guard let mark = row.first, stateMarks.contains(mark) else { return nil }
        let title = row.dropFirst().trimmingCharacters(in: .whitespaces)
        return title.isEmpty || title.count > 40 ? nil : title
    }

    private static func isExtra(_ label: String) -> Bool {
        let text = label.lowercased()
        return text.hasPrefix("type something") || text == "other" || text.hasPrefix("chat about this")
    }

    private static func isSeparator(_ text: String) -> Bool {
        separator.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) != nil
    }

    /// The row of question tabs always ends with a Submit tab between the arrows.
    private static func isTabRow(_ text: String) -> Bool {
        (text.contains("←") || text.contains("→")) && text.lowercased().contains("submit")
    }

    /// "←  ☐ Color  ☐ Fruit  ✔ Submit  →" gives ["Color", "Fruit"].
    static func tabs(in body: [String]) -> [String] {
        guard let row = body.first(where: isTabRow) else { return [] }
        let stripped = row.replacingOccurrences(of: "←", with: " ").replacingOccurrences(of: "→", with: " ")
        var titles = stripped.components(separatedBy: "  ")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .map { title -> String in
                // A leading state mark (☐ open, ☒ or ✔ answered) is not part of the name.
                guard let first = title.unicodeScalars.first, !first.properties.isAlphabetic,
                      !CharacterSet.decimalDigits.contains(first) else { return title }
                return String(title.dropFirst()).trimmingCharacters(in: .whitespaces)
            }
        if titles.last?.lowercased() == "submit" { titles.removeLast() }
        return titles
    }
}
