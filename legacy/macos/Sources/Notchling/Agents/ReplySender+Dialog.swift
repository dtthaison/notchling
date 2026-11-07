import Foundation

/// Questions in a background job's terminal: reading one, and answering it with the screen checked after
/// each question (`AskDialog` reads the screen). Both run in one attach, and one at a time, so Chat's read
/// of a new question can't move the tabs under an answer being typed.
extension ReplySender {
    static let dialogLock = NSLock()

    private static let arrowLeft = "\u{1b}[D"
    private static let arrowRight = "\u{1b}[C"

    /// Keeps feeding the screen until `done` holds or `timeout` passes.
    @discardableResult
    static func wait(_ master: Int32, _ screen: TerminalScreen, timeout: TimeInterval = 3,
                     until done: (TerminalScreen) -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            _ = waitForQuiet(master, quiet: 0.3, timeout: 0.9, into: screen)
            if done(screen) { return true }
        } while Date() < deadline
        return false
    }

    /// The keys that answer one question on its page (2.1.319). A digit picks a single-select option and moves
    /// on. On a multi-select page digits tick the options and Tab moves on: Tab steps through the question tabs
    /// there, so the older walk (Tab to "Next", then Enter) ended on the review screen with the next question
    /// unanswered. The number after the last option opens "Type something".
    static func pageKeys(for choice: Choice, in item: PendingQuestion.Item) -> [String] {
        switch choice {
        case .options(let numbers): return numbers.map(String.init) + ["\t"]
        default: return keys(for: choice, in: item)
        }
    }

    /// The question open in the session's terminal, every question of it, or nil when none is open or
    /// the screen couldn't be read to the end. A background job writes a question into its transcript only
    /// once it's answered and the prompt hook may be off, so the screen is what shows it. Several questions
    /// are tabs: Right steps through them (nothing is answered by moving), Left puts the dialog back on
    /// the first. Blocking; call off the main thread.
    static func readQuestion(job jobID: String) throws -> PendingQuestion? {
        dialogLock.lock()
        defer { dialogLock.unlock() }
        var found: PendingQuestion?
        try attached(jobID, refuseDialog: false, settle: nil) { master, screen in
            guard dialogOpen(screen), let first = AskDialog.page(in: screen.lines),
                  !first.onReview else { return }
            var pages = [first]
            var moved = 0
            let total = max(first.tabs.count, 1)
            while pages.count < total {
                let before = pages[pages.count - 1].question
                write(master, arrowRight)
                moved += 1
                var next: AskDialog.Page?
                wait(master, screen, timeout: 2.5) { s in
                    guard let page = AskDialog.page(in: s.lines), !page.onReview, page.question != before else { return false }
                    next = page
                    return true
                }
                guard let next else { break }
                pages.append(next)
            }
            if moved > 0 {
                for _ in 0..<moved {
                    write(master, arrowLeft)
                    Thread.sleep(forTimeInterval: 0.15)
                }
                wait(master, screen, timeout: 2.5) { AskDialog.shows(first.question, in: $0.lines) }
            }
            guard pages.count == total else { return }
            let items = pages.enumerated().map { index, page in
                PendingQuestion.Item(header: index < first.tabs.count ? first.tabs[index] : page.header, question: page.question,
                                     options: page.options, details: page.details, multiSelect: page.multiSelect)
            }
            found = PendingQuestion(toolUseID: "screen:\(jobID):" + items.map(\.question).joined(separator: "|"), items: items)
        }
        return found
    }

    /// Answers an AskUserQuestion prompt in a background job's terminal. The screen is checked before each
    /// question is answered (it must be showing that question) and after (the next question, the review
    /// screen, or the dialog gone), so a dialog that moved on, or was answered in the terminal meanwhile,
    /// ends the typing instead of putting the keys into something else.
    static func answer(_ choices: [Choice], to question: PendingQuestion, toJob jobID: String) throws {
        guard !question.items.isEmpty else { return }
        dialogLock.lock()
        defer { dialogLock.unlock() }
        try attached(jobID, refuseDialog: false, settle: nil) { master, screen in
            guard dialogOpen(screen) else { throw Failure.questionGone }
            let items = question.items
            // The dialog opens on its first question; if someone moved the tabs, walk back to it.
            var tries = 0
            while !AskDialog.shows(items[0].question, in: screen.lines) {
                guard tries <= items.count else { throw Failure.questionMismatch }
                tries += 1
                write(master, arrowLeft)
                _ = waitForQuiet(master, quiet: 0.4, timeout: 1.5, into: screen)
            }
            for (index, (item, choice)) in zip(items, choices).enumerated() {
                guard AskDialog.shows(item.question, in: screen.lines) else { throw Failure.questionMismatch }
                for key in pageKeys(for: choice, in: item) {
                    write(master, key)
                    Thread.sleep(forTimeInterval: 0.15)
                }
                let next = index + 1 < items.count ? items[index + 1] : nil
                let moved = wait(master, screen, timeout: 4) { s in
                    if let next { return AskDialog.shows(next.question, in: s.lines) }
                    return AskDialog.isReview(s.lines) || !dialogOpen(s)
                }
                guard moved else { throw Failure.questionMismatch }
            }
            if AskDialog.isReview(screen.lines) {
                write(master, "1")
                wait(master, screen, timeout: 4) { !dialogOpen($0) }
            }
        }
    }
}
