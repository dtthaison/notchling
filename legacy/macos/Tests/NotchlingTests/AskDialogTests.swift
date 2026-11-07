import XCTest
@testable import Notchling

/// A question read off a background job's terminal, and the same question as the transcript writes it.
final class AskDialogTests: XCTestCase {
    private let rule = String(repeating: "─", count: 120)
    private let footer = "Enter to select · Tab/Arrow keys to navigate · Esc to cancel"

    private func fixtureLines() throws -> [String] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "question-open", withExtension: "bin", subdirectory: "Fixtures/terminal"))
        let screen = TerminalScreen(rows: 50, cols: 200)
        screen.feed(Array(try Data(contentsOf: url)))
        return screen.lines
    }

    /// The first page of a real two-question dialog (Claude Code 2.1.318).
    func testReadsTheRecordedDialog() throws {
        let page = try XCTUnwrap(AskDialog.page(in: try fixtureLines()))
        XCTAssertEqual(page.question, "Favourite color?")
        XCTAssertEqual(page.options, ["Red", "Blue"])
        XCTAssertEqual(page.details, ["Red", "Blue"])
        XCTAssertEqual(page.tabs, ["Color", "Fruit"])
        XCTAssertFalse(page.multiSelect)
        XCTAssertFalse(page.onReview)
    }

    /// A single-question dialog read from a live job (2.1.319): no tab row, the header is a chip over the
    /// question, and the question has brackets in it, which the one-line summary in the job's state can't carry
    /// ("answer: Pick (a size)? (Small · Large)").
    func testReadsTheRecordedSingleQuestionDialog() throws {
        let page = try XCTUnwrap(AskDialog.page(in: [
            " ▐▛███▛█   Claude Code v2.1.319", "", "❯ Call the AskUserQuestion tool exactly once with one question: 'Pick (a size)?' (options Small, Large). Then stop. Do",
            "  not read or change any files.", rule, " ☐ Size", "", "Pick (a size)?", "", "❯ 1. Small", "     Choose the small size.",
            "  2. Large", "     Choose the large size.", "  3. Type something.", rule, "  4. Chat about this", "",
            "Enter to select · ↑/↓ to navigate · Esc to cancel", "", "──────────────────────────── askprobe2 ─",
        ]))
        XCTAssertEqual(page.question, "Pick (a size)?")
        XCTAssertEqual(page.header, "Size")
        XCTAssertEqual(page.options, ["Small", "Large"])
        XCTAssertEqual(page.details, ["Choose the small size.", "Choose the large size."])
        XCTAssertEqual(page.tabs, [])
        XCTAssertTrue(AskDialog.shows("Pick (a size)?", in: [rule, " ☐ Size", "Pick (a size)?", "❯ 1. Small", "  2. Type something.", rule, "  3. Chat about this", footer]))
    }

    func testSecondQuestionPageWithAnsweredTab() throws {
        let page = try XCTUnwrap(AskDialog.page(in: [
            "❯ Call the AskUserQuestion tool exactly once with two questions", rule,
            "←  ☒ Color  ☐ Fruit  ✔ Submit  →", "", "Favourite fruit?", "",
            "❯ 1. Apple", "     Apple", "  2. Pear", "     Pear", "  3. Type something.", rule, "  4. Chat about this", "", footer,
        ]))
        XCTAssertEqual(page.question, "Favourite fruit?")
        XCTAssertEqual(page.options, ["Apple", "Pear"])
        XCTAssertEqual(page.tabs, ["Color", "Fruit"], "an answered tab's mark is not part of its name")
    }

    func testSingleQuestionWrapsAndHasNoTabs() throws {
        let page = try XCTUnwrap(AskDialog.page(in: [
            "❯ Ask me how to deploy", "",
            "Which environment should the release go to first, and should the database migration run", "before or after the new build is live?", "",
            "❯ 1. Staging", "     Safest: nothing real is touched", "  2. Production", "  3. Type something.", rule, "  4. Chat about this", footer,
        ]))
        XCTAssertEqual(page.question, "Which environment should the release go to first, and should the database migration run before or after the new build is live?")
        XCTAssertEqual(page.options, ["Staging", "Production"])
        XCTAssertEqual(page.details, ["Safest: nothing real is touched", ""])
        XCTAssertEqual(page.tabs, [])
    }

    /// A multi-select page read from a live job (2.1.319): the checkbox follows the number, "Type something"
    /// has one too, and a Next row follows the list.
    func testCheckboxesMeanMultiSelect() throws {
        let page = try XCTUnwrap(AskDialog.page(in: [
            rule, "←  ☐ Pets  ☐ Where  ✔ Submit  →", "", "Which pets?", "",
            "❯ 1. [ ] Cat", "         A cat", "  2. [x] Dog", "         A dog", "  3. [ ] Fish", "         A fish",
            "  4. [ ] Type something", "     Next", rule, "  5. Chat about this", "", "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
        ]))
        XCTAssertTrue(page.multiSelect)
        XCTAssertEqual(page.question, "Which pets?")
        XCTAssertEqual(page.options, ["Cat", "Dog", "Fish"])
        XCTAssertEqual(page.details, ["A cat", "A dog", "A fish"])
        XCTAssertEqual(page.tabs, ["Pets", "Where"])
    }

    func testTheReviewScreenIsNotAQuestion() throws {
        let lines = [rule, "←  ☒ Color  ☒ Fruit  ✔ Submit  →", "Review your answers", "",
                     " ● Favourite color? → Red", "", "Ready to submit your answers?", "❯ 1. Submit answers", "  2. Cancel", footer]
        XCTAssertTrue(AskDialog.isReview(lines))
        let page = try XCTUnwrap(AskDialog.page(in: lines))
        XCTAssertTrue(page.onReview)
        XCTAssertTrue(page.options.isEmpty)
    }

    /// A permission dialog is numbered and ends in "Esc to cancel" too, but it isn't an AskUserQuestion.
    func testPermissionDialogIsNotAQuestion() {
        XCTAssertNil(AskDialog.page(in: [
            rule, "Bash command", "  rm -rf build", "Do you want to proceed?",
            "❯ 1. Yes", "  2. Yes, and don't ask again for rm commands", "  3. No, and tell Claude what to do differently", "Esc to cancel",
        ]))
    }

    func testNothingOpen() {
        XCTAssertNil(AskDialog.page(in: ["❯ ", rule, "  ? for shortcuts"]))
    }

    func testShowsMatchesTheDialogsQuestionOnly() throws {
        let lines = try fixtureLines()
        XCTAssertTrue(AskDialog.shows("Favourite color?", in: lines))
        // The recorded prompt line above the dialog names this question too; that is not the dialog asking it.
        XCTAssertTrue(lines.joined(separator: " ").contains("Favourite fruit?"))
        XCTAssertFalse(AskDialog.shows("Favourite fruit?", in: lines))
        let wrapped = [rule, "Which environment should", "the release go   to first, and", "❯ 1. Staging", "  2. Type something.", rule, "  3. Chat about this", footer]
        XCTAssertTrue(AskDialog.shows("Which environment should the release go\nto first", in: wrapped))
        XCTAssertFalse(AskDialog.shows("Which environment", in: [rule, "Review your answers", "❯ 1. Submit answers", footer]))
    }

    /// The keys for a screen-read question are the ones the terminal takes: a digit per option, the
    /// number after the last option for typed text.
    func testKeysForAReadQuestion() throws {
        let page = try XCTUnwrap(AskDialog.page(in: try fixtureLines()))
        let item = PendingQuestion.Item(header: "Color", question: page.question, options: page.options, details: page.details, multiSelect: false)
        XCTAssertEqual(ReplySender.keys(for: .option(2), in: item), ["2"])
        XCTAssertEqual(ReplySender.keys(for: .text("teal"), in: item), ["3", "teal", "\r"])
    }

    /// On a multi-select page Tab moves to the next question (live, 2.1.319); it doesn't walk down to "Next".
    func testMultiSelectTicksThenTabsOn() {
        let pets = PendingQuestion.Item(header: "Pets", question: "Which pets?", options: ["Cat", "Dog", "Fish"], multiSelect: true)
        XCTAssertEqual(ReplySender.pageKeys(for: .options([1, 3]), in: pets), ["1", "3", "\t"])
        XCTAssertEqual(ReplySender.pageKeys(for: .option(2), in: pets), ["2"])
        XCTAssertEqual(ReplySender.pageKeys(for: .text("x"), in: pets), ["4", "x", "\r"])
    }
}

/// An AskUserQuestion that hasn't been answered, as the transcript carries it (the tool call's input holds
/// every question with its options and descriptions), until the call's result arrives.
final class PendingQuestionTranscriptTests: XCTestCase {
    private func assistant(id: String, questions: String) -> Data {
        Data("""
        {"type":"assistant","message":{"content":[{"type":"tool_use","id":"\(id)","name":"AskUserQuestion","input":{"questions":\(questions)}}]}}
        """.utf8)
    }

    private func result(id: String) -> Data {
        Data("""
        {"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"\(id)","content":"The user answered: x"}]}}
        """.utf8)
    }

    private let two = """
    [{"question":"Which pets?","header":"Pets","multiSelect":true,"options":[{"label":"Cat","description":"Purrs"},{"label":"Dog","description":"Barks","preview":"woof"}]},
     {"question":"Where?","header":"Place","multiSelect":false,"options":[{"label":"Home","description":""},{"label":"Away","description":"Elsewhere"}]}]
    """

    func testUnansweredQuestionHasEveryItem() throws {
        var parser = TranscriptReader.SnapshotParser()
        parser.consume(assistant(id: "toolu_1", questions: two))
        let q = try XCTUnwrap(parser.snapshot().question)
        XCTAssertEqual(q.toolUseID, "toolu_1")
        XCTAssertEqual(q.items.map(\.question), ["Which pets?", "Where?"])
        XCTAssertEqual(q.items.map(\.header), ["Pets", "Place"])
        XCTAssertEqual(q.items[0].options, ["Cat", "Dog"])
        XCTAssertEqual(q.items[0].details, ["Purrs", "Barks"])
        XCTAssertEqual(q.items.map(\.multiSelect), [true, false])
        XCTAssertFalse(q.answerable, "a multi-select question can't be answered from the notch")
    }

    func testAnsweredQuestionIsGone() {
        var parser = TranscriptReader.SnapshotParser()
        parser.consume(assistant(id: "toolu_1", questions: two))
        parser.consume(result(id: "toolu_1"))
        XCTAssertNil(parser.snapshot().question)
    }

    func testAnotherToolsResultDoesNotAnswerIt() {
        var parser = TranscriptReader.SnapshotParser()
        parser.consume(assistant(id: "toolu_1", questions: two))
        parser.consume(result(id: "toolu_other"))
        XCTAssertNotNil(parser.snapshot().question)
    }

    func testALaterQuestionReplacesAnAnsweredOne() throws {
        var parser = TranscriptReader.SnapshotParser()
        parser.consume(assistant(id: "toolu_1", questions: two))
        parser.consume(result(id: "toolu_1"))
        parser.consume(assistant(id: "toolu_2", questions: #"[{"question":"Again?","header":"A","multiSelect":false,"options":[{"label":"Yes"},{"label":"No"}]}]"#))
        let q = try XCTUnwrap(parser.snapshot().question)
        XCTAssertEqual(q.toolUseID, "toolu_2")
        XCTAssertEqual(q.items[0].options, ["Yes", "No"])
        XCTAssertEqual(q.items[0].details, ["", ""])
        XCTAssertTrue(q.answerable)
    }

    func testInputWithoutQuestionsIsIgnored() {
        var parser = TranscriptReader.SnapshotParser()
        parser.consume(Data(#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"AskUserQuestion","input":{}}]}}"#.utf8))
        XCTAssertNil(parser.snapshot().question)
    }
}
