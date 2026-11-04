import XCTest
import ConsoleCore
@testable import Notchling

final class HostReplyTests: XCTestCase {
    private func terminalAgent(pid: pid_t) -> Agent {
        Agent(id: "s\(pid)", name: "t", project: "p", cwd: "/tmp", group: .needsInput, label: "", detail: nil,
              since: Date(), target: .process(pid))
    }

    override func tearDown() {
        HostTerminals.owns = { _ in false }
        HostTerminals.send = { _, _ in }
        HostTerminals.keys = { _, _ in }
        super.tearDown()
    }

    func testATerminalSessionTakesRepliesOnlyWhenTheHostOwnsIt() {
        HostTerminals.owns = { $0 == 42 }
        XCTAssertTrue(terminalAgent(pid: 42).canType)
        XCTAssertFalse(terminalAgent(pid: 43).canType)
        XCTAssertFalse(terminalAgent(pid: 42).canReply, "the notch's own reply path is unchanged")
    }

    func testAReplyToAnOwnedTerminalGoesThroughTheHost() throws {
        var sent: [(String, pid_t)] = []
        HostTerminals.owns = { $0 == 42 }
        HostTerminals.send = { text, pid in sent.append((text, pid)) }
        try ReplySender.send("hello\nthere", to: terminalAgent(pid: 42))
        XCTAssertEqual(sent.count, 1)
        XCTAssertEqual(sent.first?.0, "hello\nthere")
        XCTAssertEqual(sent.first?.1, 42)
    }

    func testAReplyToATerminalTheHostDoesNotOwnIsRefused() {
        HostTerminals.owns = { _ in false }
        XCTAssertThrowsError(try ReplySender.send("hi", to: terminalAgent(pid: 7)))
    }

    private func single(_ options: [String] = ["Red", "Green", "Blue"]) -> PendingQuestion.Item {
        PendingQuestion.Item(header: "Color", question: "Pick a color", options: options, multiSelect: false)
    }

    private func multi(_ options: [String] = ["Cat", "Dog", "Fish", "Bird"]) -> PendingQuestion.Item {
        PendingQuestion.Item(header: "Pets", question: "Pick pets", options: options, multiSelect: true)
    }

    func testAnswerKeysForASingleSelectQuestion() {
        let q = PendingQuestion(toolUseID: "t", items: [single()])
        XCTAssertEqual(ReplySender.answerKeys([.option(2)], to: q), ["2"])
    }

    func testAnswerKeysForAMultiSelectQuestionTabToSubmitThenReview() {
        let q = PendingQuestion(toolUseID: "t", items: [multi()])
        XCTAssertEqual(ReplySender.answerKeys([.options([1, 3])], to: q),
                       ["1", "3", "\t", "\t", "\t", "\t", "\t", "\r", "1"])
    }

    func testAnswerKeysForSeveralQuestions() {
        let q = PendingQuestion(toolUseID: "t", items: [single(), multi()])
        XCTAssertEqual(ReplySender.answerKeys([.option(2), .options([1, 3])], to: q),
                       ["2", "1", "3"] + Array(repeating: "\t", count: 5) + ["\r", "1"])
    }

    func testAnswerKeysForTypedTextFlattenNewlines() {
        let q = PendingQuestion(toolUseID: "t", items: [single()])
        XCTAssertEqual(ReplySender.answerKeys([.text("hi\nthere")], to: q), ["4", "hi there", "\r"])
    }

    func testHookAnswersJoinMultiSelectAndSkipOutOfRange() {
        let q = PendingQuestion(toolUseID: "hook:1", items: [single(), multi()])
        let answers = ReplySender.hookAnswers([.option(2), .options([1, 3, 9])], for: q)
        XCTAssertEqual(answers["Pick a color"], "Green")
        XCTAssertEqual(answers["Pick pets"], "Cat, Fish")
        XCTAssertTrue(ReplySender.hookAnswers([.option(7)], for: PendingQuestion(toolUseID: "h", items: [single()])).isEmpty)
    }

    @MainActor
    func testCanAnswerInAHostTerminal() {
        HostTerminals.owns = { $0 == 42 }
        let model = NotchModel()
        let one = PendingQuestion(toolUseID: "tu", items: [single()])
        let many = PendingQuestion(toolUseID: "tu", items: [multi()])
        XCTAssertTrue(model.canAnswer(terminalAgent(pid: 42), one))
        XCTAssertFalse(model.canAnswer(terminalAgent(pid: 42), many))
        XCTAssertTrue(model.canAnswer(terminalAgent(pid: 42), many, multiSelect: true))
        XCTAssertFalse(model.canAnswer(terminalAgent(pid: 43), one))
    }

    func testKeysIntoATerminalTheHostDoesNotOwnThrow() {
        XCTAssertThrowsError(try PhoneModel.keys(["1"], intoTerminalOwning: 99, in: OwnedSessions()))
    }

    func testTakeoverIsOfferedOnlyForTerminalsTheHostDoesNotOwnAndEndedSessions() {
        HostTerminals.owns = { $0 == 42 }
        XCTAssertTrue(ChatTakeover.offersTakeover(terminalAgent(pid: 43)))
        XCTAssertFalse(ChatTakeover.offersTakeover(terminalAgent(pid: 42)))
        var headless = terminalAgent(pid: 44)
        headless.host = "claude -p"
        XCTAssertFalse(ChatTakeover.offersTakeover(headless))
        let job = Agent(id: "j", name: "j", project: "p", cwd: "/tmp", group: .working, label: "", detail: nil,
                        since: Date(), target: .job("abc"))
        XCTAssertFalse(ChatTakeover.offersTakeover(job))
        let ended = Agent(id: "e", name: "e", project: "p", cwd: "/tmp", group: .completed, label: "", detail: nil,
                          since: Date(), target: .resume(sessionID: "e", cwd: "/tmp"))
        XCTAssertTrue(ChatTakeover.offersTakeover(ended))
    }

    func testTakeoverPromptsFollowTheState() {
        let available = ChatTakeover.prompt(for: .available, name: "x")
        XCTAssertEqual(available?.confirm, "Take Over")
        XCTAssertEqual(available?.destructive, false)
        let stoppable = ChatTakeover.prompt(for: .stoppable(busy: false), name: "x")
        XCTAssertEqual(stoppable?.confirm, "Stop Original and Take Over")
        XCTAssertEqual(stoppable?.destructive, true)
        let blocked = ChatTakeover.prompt(for: .unavailable("lý do"), name: "x")
        XCTAssertEqual(blocked?.message, "lý do")
        XCTAssertEqual(blocked?.confirm, "OK")
        XCTAssertNil(ChatTakeover.prompt(for: nil, name: "x"))
        XCTAssertTrue(ChatTakeover.busyPrompt(name: "x").destructive)
    }
}
