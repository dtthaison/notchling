import XCTest
@testable import ConsoleCore

final class BridgeRouterTests: XCTestCase {
    private func send(_ router: BridgeRouter, _ connection: BridgeConnection, _ json: String) async {
        await router.handle(json, from: connection)
    }

    // MARK: - Auth gating

    func testNothingIsServedBeforeHello() async throws {
        let rig = try makeBridgeRig()
        let (connection, io) = rig.connect()
        await send(rig.router, connection, #"{"id":7,"op":"sessions.list"}"#)
        let reply = try io.json(0)
        XCTAssertEqual(reply.errorCode, "unauthorized")
        XCTAssertEqual(reply["id"] as? Int, 7)
        XCTAssertTrue(io.isClosed)
        XCTAssertEqual(rig.backend.sessionCalls, 0, "the backend was never asked")
    }

    func testGarbageBeforeAuthClosesTheConnection() async throws {
        let rig = try makeBridgeRig()
        let (connection, io) = rig.connect()
        await send(rig.router, connection, "not json")
        XCTAssertEqual(try io.json(0).errorCode, "invalid")
        XCTAssertTrue(io.isClosed)
    }

    func testHelloWithAWrongTokenIsRejectedAndTheRightOneWorks() async throws {
        let rig = try makeBridgeRig()
        let (_, token) = try rig.core.devices.register(name: "phone", platform: "ios")
        let (bad, badIO) = rig.connect()
        await send(rig.router, bad, #"{"id":"a","op":"hello","token":"nope"}"#)
        XCTAssertEqual(try badIO.json(0).errorCode, "unauthorized")
        XCTAssertEqual(try badIO.json(0)["id"] as? String, "a")
        XCTAssertTrue(badIO.isClosed)

        let (good, goodIO) = rig.connect(from: "192.168.1.21")
        await send(rig.router, good, #"{"id":1,"op":"hello","token":"\#(token)"}"#)
        XCTAssertTrue(try goodIO.json(0).isOK)
        XCTAssertTrue(good.isAuthenticated)
        await send(rig.router, good, #"{"id":2,"op":"sessions.list"}"#)
        XCTAssertTrue(try goodIO.json(1).isOK)
    }

    func testFiveBadHellosLockTheAddress() async throws {
        let rig = try makeBridgeRig()
        for _ in 0..<5 {
            let (connection, _) = rig.connect(from: "192.168.1.50")
            await send(rig.router, connection, #"{"id":1,"op":"hello","token":"bad"}"#)
        }
        let (locked, io) = rig.connect(from: "192.168.1.50")
        await send(rig.router, locked, #"{"id":1,"op":"hello","token":"bad"}"#)
        XCTAssertTrue(io.texts.isEmpty, "a locked address gets silence, then the door")
        XCTAssertTrue(io.isClosed)

        let (other, otherIO) = rig.connect(from: "192.168.1.51")
        await send(rig.router, other, #"{"id":1,"op":"hello","token":"bad"}"#)
        XCTAssertEqual(try otherIO.json(0).errorCode, "unauthorized", "another address is unaffected")
    }

    func testPairWithTheCodeNeedsApprovalAndReturnsTheTokenOnce() async throws {
        let rig = try makeBridgeRig()
        let code = rig.core.codes.issue().code
        let (connection, io) = rig.connect()
        await send(rig.router, connection, #"{"id":1,"op":"pair","code":"\#(code)","device":{"name":"p4-test","platform":"ios"}}"#)
        let reply = try io.json(0)
        XCTAssertTrue(reply.isOK)
        let token = try XCTUnwrap(reply.dataObject["token"] as? String)
        XCTAssertEqual(rig.backend.approvalCalls, 1)
        XCTAssertEqual(rig.core.devices.devices.map(\.name), ["p4-test"])
        XCTAssertNotNil(rig.core.devices.authenticate(token: token))
        XCTAssertTrue(connection.isAuthenticated)

        let (second, secondIO) = rig.connect()
        await send(rig.router, second, #"{"id":1,"op":"pair","code":"\#(code)","device":{"name":"again","platform":"ios"}}"#)
        XCTAssertEqual(try secondIO.json(0).errorCode, "unauthorized", "the code was one-time")
        XCTAssertEqual(rig.core.devices.devices.count, 1)
        XCTAssertEqual(rig.core.audit.recent().first?.op, "pair")
    }

    func testPairDeniedOnTheMacRegistersNothing() async throws {
        let rig = try makeBridgeRig()
        rig.backend.approve = false
        let code = rig.core.codes.issue().code
        let (connection, io) = rig.connect()
        await send(rig.router, connection, #"{"id":1,"op":"pair","code":"\#(code)","device":{"name":"x","platform":"ios"}}"#)
        XCTAssertEqual(try io.json(0).errorCode, "unauthorized")
        XCTAssertTrue(io.isClosed)
        XCTAssertTrue(rig.core.devices.devices.isEmpty)
    }

    func testPairWithAWrongOrExpiredCodeFailsWithoutAskingTheMac() async throws {
        var clock = fixtureNow
        let rig = try makeBridgeRig(clock: { clock })
        let code = rig.core.codes.issue(now: fixtureNow).code
        let (wrong, wrongIO) = rig.connect()
        await send(rig.router, wrong, #"{"id":1,"op":"pair","code":"ZZZZZZZZ","device":{"name":"x","platform":"ios"}}"#)
        XCTAssertEqual(try wrongIO.json(0).errorCode, "unauthorized")

        clock = fixtureNow.addingTimeInterval(301)
        let (late, lateIO) = rig.connect(from: "192.168.1.60")
        await send(rig.router, late, #"{"id":1,"op":"pair","code":"\#(code)","device":{"name":"x","platform":"ios"}}"#)
        XCTAssertEqual(try lateIO.json(0).errorCode, "unauthorized")
        XCTAssertEqual(rig.backend.approvalCalls, 0)
    }

    func testRevokeClosesLiveConnectionsAndRejectsTheToken() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, token) = try await rig.login()
        let device = try XCTUnwrap(connection.device)
        try rig.core.revoke(deviceId: device.id)
        XCTAssertTrue(io.isClosed)
        XCTAssertNil(rig.core.devices.authenticate(token: token))

        let (again, againIO) = rig.connect(from: "192.168.1.70")
        await send(rig.router, again, #"{"id":1,"op":"hello","token":"\#(token)"}"#)
        XCTAssertEqual(try againIO.json(0).errorCode, "unauthorized")
        XCTAssertEqual(rig.core.audit.recent().first?.op, "revoke")
    }

    // MARK: - Reads

    func testSessionsListEncodesSubagentsAndPending() async throws {
        let rig = try makeBridgeRig()
        let prompt = PendingPrompt(id: "r1", sessionId: "s1", kind: .permission, toolName: "Bash", summary: "ls", created: fixtureNow)
        rig.backend.sessionList = [makeSession("s1", pending: prompt, state: .needsInput), makeSession("s2", control: .viewOnly)]
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":5,"op":"sessions.list"}"#)
        let data = try io.lastJSON().dataObject
        let sessions = try XCTUnwrap(data["sessions"] as? [[String: Any]])
        XCTAssertEqual(sessions.count, 2)
        XCTAssertEqual(sessions[0]["state"] as? String, "needsInput")
        XCTAssertEqual((sessions[0]["pending"] as? [String: Any])?["requestId"] as? String, "r1")
        XCTAssertEqual(sessions[1]["control"] as? String, "viewOnly")
    }

    func testChatGetValidationLimitAndBefore() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("s1")]
        rig.backend.chat = (0..<10).map { makeMessage("m\($0)") }
        let (connection, io, _) = try await rig.login()

        await send(rig.router, connection, #"{"id":1,"op":"chat.get"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await send(rig.router, connection, #"{"id":2,"op":"chat.get","sessionId":"nope"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")

        await send(rig.router, connection, #"{"id":3,"op":"chat.get","sessionId":"s1","limit":3}"#)
        var data = try io.lastJSON().dataObject
        var messages = try XCTUnwrap(data["messages"] as? [[String: Any]])
        XCTAssertEqual(messages.map { $0["text"] as? String }, ["m7", "m8", "m9"])
        XCTAssertEqual(data["hasMore"] as? Bool, true)

        let before = rig.backend.chat[5].id.uuidString
        await send(rig.router, connection, #"{"id":4,"op":"chat.get","sessionId":"s1","limit":999,"before":"\#(before)"}"#)
        data = try io.lastJSON().dataObject
        messages = try XCTUnwrap(data["messages"] as? [[String: Any]])
        XCTAssertEqual(messages.count, 5, "limit is capped at 200, and `before` cuts the list")
        XCTAssertEqual(data["hasMore"] as? Bool, false)

        await send(rig.router, connection, #"{"id":5,"op":"chat.get","sessionId":"s1","before":"missing"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
    }

    func testVizGetReturnsSuggestionAndWebURL() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("s1")]
        rig.backend.vizResult = BridgeViz(suggestedTab: "web", webURL: "http://localhost:5180",
                                          specs: ["flow": VizSpec(kind: .flow, empty: .init(title: "T")), "chart": VizSpec(kind: .chart, empty: .init(title: "C"))])
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"viz.get","sessionId":"s1"}"#)
        let data = try io.lastJSON().dataObject
        XCTAssertEqual(data["suggested"] as? String, "web")
        XCTAssertEqual(data["webURL"] as? String, "http://localhost:5180")
        XCTAssertEqual(Set(data.keys), ["suggested", "webURL", "flow", "chart"], "no entity unless asked")
    }

    func testWebSnapshotRejectsRemoteURLsAndSendsJSONThenBinary() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("s1")]
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"web.snapshot","sessionId":"s1","url":"https://example.com"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await send(rig.router, connection, #"{"id":2,"op":"web.snapshot","sessionId":"s1","url":"http://169.254.169.254/"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")

        await send(rig.router, connection, #"{"id":3,"op":"web.snapshot","sessionId":"s1","url":"http://localhost:3000","width":400}"#)
        XCTAssertEqual(io.binaries.count, 1)
        let frame = io.binaries[0]
        XCTAssertEqual(Array(frame.prefix(4)), Array("WEB1".utf8))
        XCTAssertEqual(Array(frame[4..<8]), [0, 0, 0, 3], "request id, big-endian")
        XCTAssertEqual(Array(frame.dropFirst(8)), [0xFF, 0xD8, 0xFF, 0xD9])
        let data = try io.json(io.texts.count - 2).dataObject
        XCTAssertEqual(data["binary"] as? Bool, true)
    }

    // MARK: - Writes

    func testMessageToAViewOnlySessionIsRefusedAndNeverReachesTheBackend() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("v", control: .viewOnly)]
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"session.message","sessionId":"v","text":"hello"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "view_only")
        XCTAssertTrue(rig.backend.sent.isEmpty)
        XCTAssertTrue(rig.core.audit.recent().isEmpty, "a refused message is not an executed one")
    }

    func testMessageToAnOwnedSessionIsSentAndAuditedWithoutItsText() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p", control: .consolePTY), makeSession("j", control: .job("abc"))]
        let (connection, io, _) = try await rig.login()
        let secret = "run the secret migration"
        await send(rig.router, connection, #"{"id":1,"op":"session.message","sessionId":"p","text":"\#(secret)"}"#)
        XCTAssertTrue(try io.lastJSON().isOK)
        await send(rig.router, connection, #"{"id":2,"op":"session.message","sessionId":"j","text":"to the job"}"#)
        XCTAssertEqual(rig.backend.sent.map(\.0), ["p", "j"])

        let entries = rig.core.audit.recent()
        XCTAssertEqual(entries.count, 2)
        XCTAssertEqual(entries[1].op, "message")
        XCTAssertEqual(entries[1].length, secret.count)
        XCTAssertEqual(entries[1].device, "test-phone")
        let file = try String(contentsOf: rig.directory.appendingPathComponent("remote-log.jsonl"), encoding: .utf8)
        XCTAssertFalse(file.contains(secret))
    }

    func testMessageValidation() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        let (connection, io, _) = try await rig.login()
        for body in [#"{"id":1,"op":"session.message","sessionId":"p"}"#, #"{"id":1,"op":"session.message","sessionId":"p","text":""}"#,
                     #"{"id":1,"op":"session.message","text":"hi"}"#] {
            await send(rig.router, connection, body)
            XCTAssertEqual(try io.lastJSON().errorCode, "invalid", body)
        }
        await send(rig.router, connection, #"{"id":2,"op":"session.message","sessionId":"ghost","text":"hi"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        let huge = String(repeating: "x", count: BridgeRouter.maxTextCharacters + 1)
        await send(rig.router, connection, #"{"id":3,"op":"session.message","sessionId":"p","text":"\#(huge)"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        XCTAssertTrue(rig.backend.sent.isEmpty)
    }

    func testASecondMessageToTheSameSessionWhileOneIsSendingIsBusy() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        rig.backend.sendDelay = 0.3
        let (connection, io, _) = try await rig.login()
        async let first: Void = send(rig.router, connection, #"{"id":1,"op":"session.message","sessionId":"p","text":"one"}"#)
        try await Task.sleep(nanoseconds: 100_000_000)
        await send(rig.router, connection, #"{"id":2,"op":"session.message","sessionId":"p","text":"two"}"#)
        _ = await first
        let replies = try (0..<io.texts.count).map { try io.json($0) }
        XCTAssertEqual(replies.first { $0["id"] as? Int == 2 }?.errorCode, "busy")
        XCTAssertTrue(replies.first { $0["id"] as? Int == 1 }?.isOK ?? false)
        XCTAssertEqual(rig.backend.sent.map(\.1), ["one"])
    }

    func testPromptAnswerForPermissionAndQuestions() async throws {
        let rig = try makeBridgeRig()
        rig.backend.prompts = [
            "perm": PendingPrompt(id: "perm", sessionId: "s1", kind: .permission, toolName: "Bash", summary: "ls", created: fixtureNow),
            "ask": PendingPrompt(id: "ask", sessionId: "s1", kind: .question([PromptQuestion(header: "H", question: "Q?", options: ["main", "develop"], multiSelect: false)]),
                                 toolName: "AskUserQuestion", summary: "Q?", created: fixtureNow),
            "multi": PendingPrompt(id: "multi", sessionId: "s1", kind: .question([PromptQuestion(header: "H", question: "Q?", options: ["a"], multiSelect: true)]),
                                   toolName: "AskUserQuestion", summary: "Q?", created: fixtureNow),
        ]
        let (connection, io, _) = try await rig.login()

        await send(rig.router, connection, #"{"id":1,"op":"prompt.answer","requestId":"perm"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await send(rig.router, connection, #"{"id":2,"op":"prompt.answer","requestId":"perm","allow":false}"#)
        XCTAssertTrue(try io.lastJSON().isOK)
        await send(rig.router, connection, #"{"id":3,"op":"prompt.answer","requestId":"ask","choice":2}"#)
        await send(rig.router, connection, #"{"id":4,"op":"prompt.answer","requestId":"ask","choice":"main"}"#)
        await send(rig.router, connection, #"{"id":5,"op":"prompt.answer","requestId":"ask","text":"something else"}"#)
        XCTAssertEqual(rig.backend.responses.map(\.1), [.deny, .answers(["develop"]), .answers(["main"]), .answers(["something else"])])

        for body in [#"{"id":6,"op":"prompt.answer","requestId":"ask","choice":3}"#, #"{"id":7,"op":"prompt.answer","requestId":"ask","choice":"nope"}"#,
                     #"{"id":8,"op":"prompt.answer","requestId":"ask"}"#, #"{"id":9,"op":"prompt.answer","requestId":"multi","choice":1}"#,
                     #"{"id":10,"op":"prompt.answer","requestId":"ask","allow":true}"#] {
            await send(rig.router, connection, body)
            XCTAssertEqual(try io.lastJSON().errorCode, "invalid", body)
        }
        await send(rig.router, connection, #"{"id":11,"op":"prompt.answer","requestId":"gone","allow":true}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        XCTAssertEqual(rig.core.audit.recent().filter { $0.op == "answer" }.count, 4)
    }

    func testSessionNewChecksTheFolderAndModel() async throws {
        let rig = try makeBridgeRig()
        let project = rig.home.appendingPathComponent("work/app")
        try FileManager.default.createDirectory(at: project, withIntermediateDirectories: true)
        let file = rig.home.appendingPathComponent("file.txt")
        try Data("x".utf8).write(to: file)
        let outside = FileManager.default.temporaryDirectory.appendingPathComponent("outside-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: outside) }
        try FileManager.default.createSymbolicLink(at: rig.home.appendingPathComponent("escape"), withDestinationURL: outside)
        let (connection, io, _) = try await rig.login()

        func call(_ cwd: String, model: String? = nil) async throws -> [String: Any] {
            let modelPart = model.map { #","model":"\#($0)""# } ?? ""
            await send(rig.router, connection, #"{"id":1,"op":"session.new","cwd":"\#(cwd)"\#(modelPart)}"#)
            return try io.lastJSON()
        }
        func expect(_ code: String?, _ cwd: String, model: String? = nil, line: UInt = #line) async throws {
            let reply = try await call(cwd, model: model)
            XCTAssertEqual(reply.errorCode, code, cwd, line: line)
            if code == nil { XCTAssertTrue(reply.isOK, line: line) }
        }
        try await expect("invalid", "relative/path")
        try await expect("invalid", outside.path)                       // outside the home folder
        try await expect("invalid", rig.home.path + "/../elsewhere")    // dot-dot is resolved before the check
        try await expect("invalid", rig.home.path + "/escape")          // a symlink cannot lead out of home
        try await expect("not_found", rig.home.path + "/missing")
        try await expect("not_found", file.path)                        // a file is not a folder
        try await expect("invalid", project.path, model: "gpt")
        XCTAssertTrue(rig.backend.newSessions.isEmpty)

        try await expect(nil, project.path)
        try await expect(nil, project.path, model: "default")
        try await expect(nil, project.path, model: "opus")
        XCTAssertEqual(rig.backend.newSessions.map(\.0.lastPathComponent), ["app", "app", "app"])
        XCTAssertEqual(rig.backend.newSessions.map(\.1), [nil, nil, "opus"])
        XCTAssertEqual(rig.core.audit.recent().filter { $0.op == "newSession" }.count, 3)
    }

    func testProjectsRecentAndUnknownOps() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"projects.recent"}"#)
        XCTAssertEqual(try io.lastJSON().dataObject["projects"] as? [String], ["/a", "/b"])
        await send(rig.router, connection, #"{"id":2,"op":"rm.everything"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await send(rig.router, connection, #"{"id":3,"op":"ping"}"#)
        XCTAssertTrue(try io.lastJSON().isOK)
    }

    // MARK: - Subscriptions

    func testSubscribeSendsSessionsNowAndClampsPhoneOptions() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("s1")]
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"subscribe","sessions":true,"phone":{"fps":60,"width":4000,"deviceWindowId":12}}"#)
        XCTAssertEqual(try io.json(1)["event"] as? String, "sessions", "the first push comes at once")
        XCTAssertEqual(rig.backend.phoneStarts, [PhoneOptions(deviceWindowId: 12, fps: 15, width: 600)])
        XCTAssertTrue(try io.lastJSON().isOK)
        XCTAssertTrue(connection.wantsSessions)

        await send(rig.router, connection, #"{"id":2,"op":"unsubscribe"}"#)
        XCTAssertFalse(connection.wantsSessions)
    }

    func testPhoneSubscribeErrorIsReportedAndSessionIdMustExist() async throws {
        let rig = try makeBridgeRig()
        rig.backend.phoneError = BridgeFailure(.internal, "Chưa có quyền ghi màn hình")
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"subscribe","phone":{"fps":10}}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "internal")
        await send(rig.router, connection, #"{"id":2,"op":"subscribe","sessionId":"ghost"}"#)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
    }

    func testHubPushesOnlyChanges() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("s1")]
        rig.backend.chat = [makeMessage("old")]
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"subscribe","sessions":true,"sessionId":"s1"}"#)
        let baseline = io.texts.count

        await rig.core.hub.tick(backend: rig.backend)
        XCTAssertEqual(io.texts.count, baseline, "nothing changed, nothing pushed")

        rig.backend.sessionList = [makeSession("s1", state: .done), makeSession("s2")]
        rig.backend.chat.append(makeMessage("new one", role: .assistant))
        await rig.core.hub.tick(backend: rig.backend)
        let pushed = try (baseline..<io.texts.count).map { try io.json($0) }
        XCTAssertEqual(pushed.compactMap { $0["event"] as? String }, ["sessions", "chat"])
        let chat = try XCTUnwrap(pushed.first { $0["event"] as? String == "chat" })
        XCTAssertEqual((chat["messages"] as? [[String: Any]])?.map { $0["text"] as? String }, ["new one"])
        XCTAssertEqual(chat["sessionId"] as? String, "s1")

        await rig.core.hub.tick(backend: rig.backend)
        XCTAssertEqual(io.texts.count, baseline + 2, "a second tick with no change is silent")
    }

    func testHubPushesPromptAppearingAndResolving() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("s1")]
        let (connection, io, _) = try await rig.login()
        await send(rig.router, connection, #"{"id":1,"op":"subscribe","sessions":true}"#)
        let baseline = io.texts.count

        let prompt = PendingPrompt(id: "r1", sessionId: "s1", kind: .permission, toolName: "Bash", summary: "ls", created: fixtureNow)
        rig.backend.sessionList = [makeSession("s1", pending: prompt, state: .needsInput)]
        await rig.core.hub.tick(backend: rig.backend)
        var events = try (baseline..<io.texts.count).map { try io.json($0)["event"] as? String }
        XCTAssertEqual(events, ["sessions", "prompt"])
        XCTAssertEqual(try io.lastJSON()["sessionId"] as? String, "s1")

        let afterAppear = io.texts.count
        rig.backend.sessionList = [makeSession("s1")]
        await rig.core.hub.tick(backend: rig.backend)
        events = try (afterAppear..<io.texts.count).map { try io.json($0)["event"] as? String }
        XCTAssertEqual(events, ["sessions", "prompt"])
        XCTAssertEqual(try io.lastJSON()["resolved"] as? Bool, true)
        XCTAssertEqual(try io.lastJSON()["requestId"] as? String, "r1")
    }

    // MARK: - Wire format

    func testResponseEncodingShapeAndIdEcho() throws {
        let ok = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(BridgeWire.ok(id: "abc", data: ["n": 1]).utf8)) as? [String: Any])
        XCTAssertEqual(ok["id"] as? String, "abc")
        XCTAssertEqual(ok["ok"] as? Bool, true)
        XCTAssertEqual((ok["data"] as? [String: Any])?["n"] as? Int, 1)

        let failure = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(BridgeWire.error(id: 9, BridgeFailure(.viewOnly, "chỉ xem")).utf8)) as? [String: Any])
        XCTAssertEqual(failure["id"] as? Int, 9)
        XCTAssertEqual(failure["ok"] as? Bool, false)
        XCTAssertEqual(failure.errorCode, "view_only")
        XCTAssertEqual(((failure["error"] as? [String: Any])?["message"]) as? String, "chỉ xem")

        XCTAssertEqual(BridgeWire.ok(id: 1, data: ["b": 2, "a": 1]), #"{"data":{"a":1,"b":2},"id":1,"ok":true}"#, "sorted keys, stable text")
        XCTAssertEqual(BridgeErrorCode.notReachable.rawValue, "not_reachable")
        XCTAssertEqual(try JSONSerialization.jsonObject(with: Data(BridgeWire.event("sessions", ["data": []]).utf8)) as? NSDictionary,
                       ["event": "sessions", "data": []] as NSDictionary)
    }

    func testPhoneFrameHeaderAndDropWhenBusy() throws {
        let frame = PhoneFrame.encode(sequence: 0x01020304, jpeg: Data([0xAA, 0xBB]))
        XCTAssertEqual(Array(frame), Array("PHN1".utf8) + [1, 2, 3, 4, 0xAA, 0xBB])

        let io = RecordingIO()
        io.holdBinaryCompletions = true
        let gate = FrameGate()
        XCTAssertTrue(gate.offer(jpeg: Data([1]), via: io))
        XCTAssertFalse(gate.offer(jpeg: Data([2]), via: io), "the first frame is still in flight")
        XCTAssertFalse(gate.offer(jpeg: Data([3]), via: io))
        XCTAssertEqual(io.binaries.count, 1, "never more than one frame buffered")
        XCTAssertEqual(gate.dropped, 2)

        io.releaseBinary()
        XCTAssertTrue(gate.offer(jpeg: Data([4]), via: io))
        XCTAssertEqual(Array(io.binaries[1].dropFirst(4).prefix(4)), [0, 0, 0, 2], "sequence counts frames actually sent")
    }

    func testRecentProjectDirectoryNameDecoding() {
        let existing: Set<String> = ["/Users", "/Users/me", "/Users/me/my-app", "/Users/me/docs", "/Users/me/docs/tools"]
        let exists: (String) -> Bool = { existing.contains($0) }
        XCTAssertEqual(RecentProjects.decode(directoryName: "-Users-me-my-app", directoryExists: exists), "/Users/me/my-app")
        XCTAssertEqual(RecentProjects.decode(directoryName: "-Users-me-docs-tools", directoryExists: exists), "/Users/me/docs/tools")
        XCTAssertNil(RecentProjects.decode(directoryName: "-Users-me-gone", directoryExists: exists))
        XCTAssertNil(RecentProjects.decode(directoryName: "no-leading-dash", directoryExists: exists))
    }
}
