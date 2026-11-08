import ConsoleCore
import Foundation
import XCTest
@testable import Notchling

/// The recording phase before E3 (docs/recording.md): captures the terminal bytes of every reply behavior
/// while the Swift app can still produce them, as `<behavior>.bin` plus `<behavior>.json` (the keys sent,
/// when, and where each phase starts in the bytes). Opt-in and maintainer-run: `testRecordReplyFixtures`
/// returns at once, printing why, unless NOTCHLING_RECORD=1. Steps are in tools/record-reply-fixtures.sh.
///
///     NOTCHLING_RECORD=1                    turn it on
///     NOTCHLING_RECORD_JOB=<id>             a scratch background job (its folder is named *notchling-record*)
///     NOTCHLING_RECORD_OUT=<folder>         where the files go (never under ~/.claude)
///     NOTCHLING_RECORD_ASK_JOB=<id>         optional: a scratch job with an AskUserQuestion open
///     NOTCHLING_RECORD_ANSWER=1             optional: also answer that question (option|text|multi by _MODE)
///     NOTCHLING_RECORD_PERMISSION_JOB=<id>  optional: a scratch job blocked on a permission dialog
///     NOTCHLING_RECORD_TRUST_DIR=<folder>   optional: a never-trusted *notchling-record* folder (starts `claude`)
///     NOTCHLING_RECORD_TRANSCRIPT=<path>    optional: the main job's transcript, to read `popAll` back
///     NOTCHLING_RECORD_ONLY=a,b             optional: only these behaviors
///
/// Safety: every job and folder must be a scratch one (`RecordGuard`). It types real replies into those
/// sessions, which cost a model turn each, and never touches another session, Keychain or a login.
/// It owns its attach (a forkpty with the same size, TERM and env the app uses, teeing every byte read)
/// because `ReplySender` and `TerminalSession` keep the bytes to themselves; the keys, pauses, parsers and
/// dialog check it uses are the app's own (`ReplySender.clearInput`, `pageKeys`, `dialogOpen`, `AskDialog`).
final class RecordReplyFixturesTests: XCTestCase {
    func testRecordReplyFixtures() throws {
        let env = ProcessInfo.processInfo.environment
        guard env["NOTCHLING_RECORD"] == "1" else {
            print("RECORD off: set NOTCHLING_RECORD=1, NOTCHLING_RECORD_JOB and NOTCHLING_RECORD_OUT (tools/record-reply-fixtures.sh)")
            return
        }
        guard let job = env["NOTCHLING_RECORD_JOB"], !job.isEmpty, let out = env["NOTCHLING_RECORD_OUT"], !out.isEmpty else {
            return XCTFail("NOTCHLING_RECORD=1 needs NOTCHLING_RECORD_JOB and NOTCHLING_RECORD_OUT")
        }
        do {
            try RecordGuard.requireScratchJob(job)
            let outDir = try RecordGuard.requireOutputFolder(out)
            let ask = try env["NOTCHLING_RECORD_ASK_JOB"].map { try RecordGuard.requireScratchJob($0) }
            let permission = try env["NOTCHLING_RECORD_PERMISSION_JOB"].map { try RecordGuard.requireScratchJob($0) }
            let trust = try env["NOTCHLING_RECORD_TRUST_DIR"].map { try RecordGuard.requireScratchFolder($0) }
            let recorder = Recorder(outDir: outDir, job: job, askJob: ask, permissionJob: permission, trustDir: trust,
                                    transcript: env["NOTCHLING_RECORD_TRANSCRIPT"],
                                    answerMode: env["NOTCHLING_RECORD_ANSWER"] == "1" ? (env["NOTCHLING_RECORD_ANSWER_MODE"] ?? "option") : nil)
            let only = env["NOTCHLING_RECORD_ONLY"].map { Set($0.split(separator: ",").map(String.init)) }
            for behavior in recorder.behaviors where only?.contains(behavior.name) ?? true {
                do {
                    guard let run = try behavior.run() else { print("RECORD skipped \(behavior.name): \(behavior.needs)"); continue }
                    print("RECORD \(behavior.name): \(run) bytes")
                } catch { XCTFail("\(behavior.name): \(error)") }
            }
        } catch { XCTFail("refused: \(error)") }
    }

    // MARK: the scratch rule, always checked

    func testScratchFolderRule() {
        XCTAssertTrue(RecordGuard.isScratchFolder("/tmp/notchling-record-1"))
        XCTAssertTrue(RecordGuard.isScratchFolder("/Users/me/notchling-record"))
        XCTAssertFalse(RecordGuard.isScratchFolder("/Users/me/work/app"))
        XCTAssertFalse(RecordGuard.isScratchFolder("/tmp/notchling-record/sub"), "only the folder's own name counts")
        XCTAssertFalse(RecordGuard.isScratchFolder("/tmp/notchling-record/../real"), "dot-dot is resolved first")
        XCTAssertFalse(RecordGuard.isScratchFolder("notchling-record"), "a relative path is not a place")
        XCTAssertFalse(RecordGuard.isScratchFolder(""))
    }

    func testJobIDsAreSimpleNames() {
        XCTAssertTrue(RecordGuard.isJobID("c1f2a3b4"))
        XCTAssertFalse(RecordGuard.isJobID("../x"))
        XCTAssertFalse(RecordGuard.isJobID("a/b"))
        XCTAssertFalse(RecordGuard.isJobID(""))
    }

    func testOutputFolderNeverInsideClaudeConfig() {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        XCTAssertThrowsError(try RecordGuard.requireOutputFolder(home + "/.claude/rec"))
        XCTAssertThrowsError(try RecordGuard.requireOutputFolder(home + "/.claude-lanes/a/rec"))
        XCTAssertThrowsError(try RecordGuard.requireOutputFolder("relative/rec"))
    }

    func testEveryBehaviorHasAFileSafeUniqueName() {
        let names = Recorder.behaviorNames
        XCTAssertEqual(Set(names).count, names.count)
        for name in names {
            XCTAssertNotNil(name.range(of: #"^[a-z0-9]+(-[a-z0-9]+)*$"#, options: .regularExpression), name)
        }
    }

    func testRecordedKeysAreTheAppsOwn() {
        // The recorder replays these; if the app changes them the fixtures must be recorded again.
        XCTAssertEqual(ReplySender.clearInput, String(repeating: "\u{05}\u{15}\u{7F}", count: 20) + "\u{05}\u{15}")
        XCTAssertEqual(ConsoleCore.TerminalKeys.bracketed("a"), "\u{1b}[200~a\u{1b}[201~")
    }
}

// MARK: - the scratch rule

enum RecordGuard {
    enum Refusal: Error, CustomStringConvertible {
        case notScratch(String), noSuchJob(String), badID(String), badOutput(String)
        var description: String {
            switch self {
            case .notScratch(let what): return "\(what) is not a scratch folder (its name must contain notchling-record)"
            case .noSuchJob(let id): return "no job \(id) found under any config folder"
            case .badID(let id): return "\(id) is not a job id"
            case .badOutput(let why): return "output folder refused: \(why)"
            }
        }
    }

    static let marker = "notchling-record"

    /// An absolute folder whose own name contains the marker, after `..` is resolved.
    static func isScratchFolder(_ path: String) -> Bool {
        guard path.hasPrefix("/") else { return false }
        return URL(fileURLWithPath: path).standardizedFileURL.lastPathComponent.contains(marker)
    }

    static func isJobID(_ id: String) -> Bool {
        id.range(of: #"^[A-Za-z0-9_-]+$"#, options: .regularExpression) != nil
    }

    static func requireScratchFolder(_ path: String) throws -> String {
        guard isScratchFolder(path) else { throw Refusal.notScratch(path) }
        return URL(fileURLWithPath: path).standardizedFileURL.path
    }

    /// The job must exist and have been started in a scratch folder: its `cwd` in `jobs/<id>/state.json`.
    @discardableResult
    static func requireScratchJob(_ id: String) throws -> String {
        guard isJobID(id) else { throw Refusal.badID(id) }
        for (dir, _) in LaneStore.configDirs() {
            let state = dir.appendingPathComponent("jobs").appendingPathComponent(id).appendingPathComponent("state.json")
            guard let data = try? Data(contentsOf: state),
                  let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { continue }
            let cwd = json["cwd"] as? String ?? ""
            guard isScratchFolder(cwd) else { throw Refusal.notScratch(cwd.isEmpty ? "job \(id)'s folder" : cwd) }
            return id
        }
        throw Refusal.noSuchJob(id)
    }

    /// An absolute folder, created if missing, that is not Claude Code's own config.
    static func requireOutputFolder(_ path: String) throws -> URL {
        guard path.hasPrefix("/") else { throw Refusal.badOutput("\(path) is not absolute") }
        let url = URL(fileURLWithPath: path).standardizedFileURL
        let home = FileManager.default.homeDirectoryForCurrentUser.standardizedFileURL.path
        for forbidden in [home + "/.claude", home + "/.claude-lanes", home + "/.claude.json"]
        where url.path == forbidden || url.path.hasPrefix(forbidden + "/") {
            throw Refusal.badOutput("\(url.path) is inside \(forbidden)")
        }
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}

// MARK: - one attach, every byte kept

/// `claude` in a forkpty (what `TerminalSession` does), with each byte read kept and each key written logged.
final class Capture {
    let screen: TerminalScreen
    private let master: Int32
    private let pid: pid_t
    private(set) var bytes: [UInt8] = []
    private var steps: [[String: Any]] = []
    private var phases: [[String: Any]] = []
    private let began = Date()
    private let program: String
    private let argv: [String]
    /// Answers `CSI c`, `CSI > 0 q` and `CSI > c` the way a terminal would, to record what that did.
    var answerQueries = false

    enum Failure: Error { case fork(Int32) }

    init(program: String, args: [String], folder: String, env: [String: String], rows: Int, cols: Int) throws {
        self.program = program
        argv = args
        screen = TerminalScreen(rows: rows, cols: cols)
        let cProgram = strdup(program)
        let cFolder = strdup(folder)
        let cArgs = ([program] + args).map { strdup($0) } + [nil]
        let cEnv = env.map { strdup("\($0.key)=\($0.value)") } + [nil]
        defer { ([cProgram, cFolder] + cArgs + cEnv).forEach { free($0) } }
        var noSignals = sigset_t()
        sigemptyset(&noSignals)
        let maxFD = min(getdtablesize(), 4096)
        var size = winsize(ws_row: UInt16(rows), ws_col: UInt16(cols), ws_xpixel: 0, ws_ypixel: 0)
        var opened: Int32 = -1
        let forked = forkpty(&opened, nil, nil, &size)
        if forked == 0 {
            sigprocmask(SIG_SETMASK, &noSignals, nil)
            var other: Int32 = 3
            while other < maxFD { Darwin.close(other); other += 1 }
            if let cFolder { _ = chdir(cFolder) }
            execve(cProgram, cArgs, cEnv)
            _exit(127)
        }
        guard forked > 0 else { throw Failure.fork(errno) }
        pid = forked
        master = opened
    }

    deinit { close() }

    /// Reads until nothing arrives for `quiet` seconds (false if nothing came before `timeout`).
    @discardableResult
    func pump(quiet: TimeInterval, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        var last: Date?
        while Date() < deadline {
            if readOnce(waitMs: 100) { last = Date() } else if let last, Date().timeIntervalSince(last) >= quiet { return true }
        }
        return last != nil
    }

    /// Reads for `seconds`, whatever arrives (the app's `Thread.sleep` between keys, but still draining).
    func pause(_ seconds: TimeInterval) {
        let end = Date().addingTimeInterval(seconds)
        while Date() < end { _ = readOnce(waitMs: 50) }
    }

    /// Reads until `done` holds on the screen, or `timeout`.
    @discardableResult
    func until(_ timeout: TimeInterval, _ done: (TerminalScreen) -> Bool) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        repeat {
            pump(quiet: 0.3, timeout: 0.9)
            if done(screen) { return true }
        } while Date() < end
        return false
    }

    var text: String { screen.lines.joined(separator: "\n") }
    var working: Bool { text.contains("esc to interrupt") }

    private func readOnce(waitMs: Int32) -> Bool {
        var pfd = pollfd(fd: master, events: Int16(POLLIN), revents: 0)
        guard poll(&pfd, 1, waitMs) > 0 else { return false }
        var buffer = [UInt8](repeating: 0, count: 65536)
        let n = read(master, &buffer, buffer.count)
        guard n > 0 else { return false }
        let chunk = Array(buffer[0..<n])
        bytes += chunk
        screen.feed(chunk)
        let replies = screen.takeReplies()
        if !replies.isEmpty { raw(replies, label: "screen reply") }
        if answerQueries { answerDeviceQueries(in: chunk) }
        return true
    }

    private func answerDeviceQueries(in chunk: [UInt8]) {
        let table: [(query: String, answer: String)] = [
            ("\u{1b}[c", "\u{1b}[?62;c"), ("\u{1b}[>0q", "\u{1b}P>|xterm(388)\u{1b}\\"), ("\u{1b}[>c", "\u{1b}[>0;276;0c"),
        ]
        let text = String(decoding: chunk, as: UTF8.self)
        for entry in table where text.contains(entry.query) { raw(Array(entry.answer.utf8), label: "answered " + entry.query.debugDescription) }
    }

    /// Writes keys, logged by label and time.
    func write(_ keys: String, _ label: String) { raw(Array(keys.utf8), label: label) }

    private func raw(_ data: [UInt8], label: String) {
        steps.append(["tMs": Int(Date().timeIntervalSince(began) * 1000), "label": label, "offset": bytes.count,
                      "hex": data.map { String(format: "%02x", $0) }.joined()])
        var offset = 0
        while offset < data.count {
            let n = data[offset...].withUnsafeBufferPointer { Darwin.write(master, $0.baseAddress, $0.count) }
            if n <= 0 { return }
            offset += n
        }
    }

    /// Marks where a phase starts in the bytes.
    func phase(_ name: String) { phases.append(["name": name, "offset": bytes.count, "tMs": Int(Date().timeIntervalSince(began) * 1000)]) }

    func resize(rows: Int, cols: Int) {
        var size = winsize(ws_row: UInt16(rows), ws_col: UInt16(cols), ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)
        screen.resize(rows: rows, cols: cols)
        steps.append(["tMs": Int(Date().timeIntervalSince(began) * 1000), "label": "resize \(rows)x\(cols)", "offset": bytes.count, "hex": ""])
    }

    /// Writes `<name>.bin` and `<name>.json`; returns the byte count.
    @discardableResult
    func save(_ name: String, to dir: URL, notes: [String: Any] = [:]) throws -> Int {
        try Data(bytes).write(to: dir.appendingPathComponent(name + ".bin"))
        var meta: [String: Any] = [
            "behavior": name, "rows": screen.rows, "cols": screen.cols, "program": program, "arguments": argv,
            "recordedAt": ISO8601DateFormatter().string(from: Date()), "byteCount": bytes.count,
            "claudeCode": text.range(of: #"v\d+\.\d+\.\d+"#, options: .regularExpression).map { String(text[$0]) } ?? "",
            "phases": phases, "steps": steps, "screenAtEnd": screen.lines,
        ]
        for (key, value) in notes { meta[key] = value }
        let json = try JSONSerialization.data(withJSONObject: meta, options: [.prettyPrinted, .sortedKeys])
        try json.write(to: dir.appendingPathComponent(name + ".json"))
        return bytes.count
    }

    /// Detaches, as `TerminalSession.close` does: the attach client exits, the session goes on.
    func close() {
        guard !closed else { return }
        closed = true
        if pid > 0 { kill(pid, SIGTERM); var status: Int32 = 0; waitpid(pid, &status, WNOHANG) }
        Darwin.close(master)
    }

    private var closed = false
}

// MARK: - the behaviors

struct Behavior {
    let name: String
    /// What a missing prerequisite is, printed when the behavior is skipped.
    let needs: String
    /// Bytes recorded, or nil when a prerequisite is missing.
    let run: () throws -> Int?
}

final class Recorder {
    let outDir: URL, job: String, askJob: String?, permissionJob: String?, trustDir: String?, transcript: String?
    let answerMode: String?

    init(outDir: URL, job: String, askJob: String?, permissionJob: String?, trustDir: String?, transcript: String?, answerMode: String?) {
        (self.outDir, self.job, self.askJob, self.permissionJob, self.trustDir, self.transcript, self.answerMode)
            = (outDir, job, askJob, permissionJob, trustDir, transcript, answerMode)
    }

    /// A turn long enough to still be working when the next key arrives, without tools or permission prompts.
    static let slowPrompt = "Write a detailed 1200 word essay about ocean tides. Do not use any tools."
    static let arrowLeft = "\u{1b}[D", arrowRight = "\u{1b}[C", arrowUp = "\u{1b}[A", arrowDown = "\u{1b}[B"

    /// The names, in the order they run; the doc's table lists the same ones.
    static let behaviorNames = [
        "attach-idle", "clear-input-draft", "reply-paced", "reply-burst-negative", "ctrl-k-burst-negative",
        "paste-multiline-clear", "esc-working", "esc-idle-twice-negative", "queued-while-working", "take-back-queued",
        "attach-no-color-dropped", "attach-no-color-kept-negative", "attach-queries-answered-negative", "terminal-resize-forkpty",
        "dialog-permission", "question-read-tabs", "question-answer", "trust-prompt",
    ]

    var behaviors: [Behavior] {
        let ask = "NOTCHLING_RECORD_ASK_JOB"
        let all: [String: (needs: String, run: () throws -> Int?)] = [
            "attach-idle": ("", attachIdle), "clear-input-draft": ("", clearInputDraft), "reply-paced": ("", replyPaced),
            "reply-burst-negative": ("", replyBurst), "ctrl-k-burst-negative": ("", ctrlKBurst),
            "paste-multiline-clear": ("", pasteMultiline), "esc-working": ("", escWorking),
            "esc-idle-twice-negative": ("", escIdleTwice), "queued-while-working": ("", queuedWhileWorking),
            "take-back-queued": ("", takeBackQueued), "attach-no-color-dropped": ("", { try self.noColor(keep: false) }),
            "attach-no-color-kept-negative": ("", { try self.noColor(keep: true) }),
            "attach-queries-answered-negative": ("", queriesAnswered), "terminal-resize-forkpty": ("", resizeForkpty),
            "dialog-permission": ("NOTCHLING_RECORD_PERMISSION_JOB", dialogPermission),
            "question-read-tabs": (ask, questionReadTabs), "question-answer": (ask + " and NOTCHLING_RECORD_ANSWER=1", questionAnswer),
            "trust-prompt": ("NOTCHLING_RECORD_TRUST_DIR", trustPrompt),
        ]
        return Self.behaviorNames.map { Behavior(name: $0, needs: all[$0]!.needs, run: all[$0]!.run) }
    }

    // MARK: attaching

    func environment(job: String, keepNoColor: Bool = false) -> [String: String] {
        var env = AgentMonitor.environment(forJob: job)
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["NO_COLOR"] = keepNoColor ? "1" : nil
        return env
    }

    /// `claude attach <job>` at the size the app uses (ReplySender: 50x200; Chat's terminal: 30x100).
    func attach(_ job: String, rows: Int = 50, cols: Int = 200, keepNoColor: Bool = false, answerQueries: Bool = false) throws -> Capture {
        let capture = try Capture(program: ReplySender.claudePath, args: ["attach", job],
                                  folder: FileManager.default.homeDirectoryForCurrentUser.path,
                                  env: environment(job: job, keepNoColor: keepNoColor), rows: rows, cols: cols)
        capture.answerQueries = answerQueries
        capture.phase("attach")
        guard capture.pump(quiet: 0.5, timeout: 15) else { throw Failure.noScreen }
        return capture
    }

    enum Failure: Error { case noScreen, dialogOpen, noQuestion }

    /// Waits (unrecorded) until the job is not working, so each behavior starts from idle.
    func ensureIdle(_ job: String) throws {
        let c = try attach(job)
        _ = c.until(120) { _ in !c.working }
        c.close()
    }

    /// The reply the app sends: clear, bracketed paste, Enter, with its pauses.
    func send(_ text: String, on c: Capture) {
        c.write(ReplySender.clearInput, "clearInput"); c.pause(0.15)
        c.write(ConsoleCore.TerminalKeys.bracketed(text), "bracketed paste"); c.pause(0.15)
        c.write("\r", "enter")
    }

    func finish(_ c: Capture, _ name: String, notes: [String: Any] = [:]) throws -> Int {
        defer { c.close() }
        return try c.save(name, to: outDir, notes: notes)
    }

    // MARK: reply behaviors on the main job

    func attachIdle() throws -> Int? {
        try ensureIdle(job)
        return try finish(try attach(job), "attach-idle")
    }

    /// A draft left in the box, then the clear keys: the box ends empty.
    func clearInputDraft() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        c.phase("draft")
        c.write(ConsoleCore.TerminalKeys.bracketed("half a message"), "bracketed paste (draft)")
        c.pump(quiet: 0.4, timeout: 3)
        c.phase("cleared")
        c.write(ReplySender.clearInput, "clearInput")
        c.pump(quiet: 0.4, timeout: 3)
        return try finish(c, "clear-input-draft", notes: ["boxEmptyAtEnd": !c.text.contains("half a message")])
    }

    /// The normal reply: pauses between the clear keys, the paste and Enter. Runs a real turn.
    func replyPaced() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        c.phase("send")
        send("Reply with just OK.", on: c)
        c.phase("turn")
        c.pump(quiet: 1.5, timeout: 60)
        return try finish(c, "reply-paced")
    }

    /// Negative: the same keys in one write, with a draft present. The clear keys read as part of the paste
    /// ("Removed N invisible characters") and Enter did not run it.
    func replyBurst() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        c.write(ConsoleCore.TerminalKeys.bracketed("stale draft"), "bracketed paste (draft)")
        c.pump(quiet: 0.4, timeout: 3)
        c.phase("burst")
        c.write(ReplySender.clearInput + ConsoleCore.TerminalKeys.bracketed("Reply with just OK.") + "\r", "one burst")
        c.pump(quiet: 1.5, timeout: 30)
        let note = ["invisibleNoticeSeen": c.text.contains("invisible")] as [String: Any]
        c.write(ReplySender.clearInput, "clearInput (cleanup)")
        c.pump(quiet: 0.4, timeout: 3)
        return try finish(c, "reply-burst-negative", notes: note)
    }

    /// Negative: Ctrl+K in a burst lands as a literal vertical tab.
    func ctrlKBurst() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        c.write(ConsoleCore.TerminalKeys.bracketed("draft one"), "bracketed paste (draft)")
        c.pump(quiet: 0.4, timeout: 3)
        c.phase("ctrl-k burst")
        c.write("\u{05}\u{0B}\u{05}\u{0B}", "Ctrl+E Ctrl+K x2 in one write")
        c.pump(quiet: 0.5, timeout: 3)
        c.write(ReplySender.clearInput, "clearInput (cleanup)")
        c.pump(quiet: 0.4, timeout: 3)
        return try finish(c, "ctrl-k-burst-negative")
    }

    /// A multi-line paste stays in the box (a placeholder), is not submitted, and the clear keys remove it.
    func pasteMultiline() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        c.phase("paste")
        c.write(ConsoleCore.TerminalKeys.bracketed("line one\nline two\nline three"), "bracketed paste (3 lines)")
        c.pump(quiet: 0.5, timeout: 3)
        c.phase("cleared")
        c.write(ReplySender.clearInput, "clearInput")
        c.pump(quiet: 0.5, timeout: 3)
        return try finish(c, "paste-multiline-clear", notes: ["boxEmptyAtEnd": !c.text.contains("line one")])
    }

    /// Esc stops a working session's turn (ReplySender.interrupt: after a short settle).
    func escWorking() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        send(Recorder.slowPrompt, on: c)
        c.phase("working")
        let seen = c.until(10) { _ in c.working }
        c.pump(quiet: 0.5, timeout: 1.2)
        c.phase("esc")
        c.write("\u{1b}", "Esc")
        c.pump(quiet: 0.8, timeout: 6)
        return try finish(c, "esc-working", notes: ["workingMarkerSeen": seen, "workingAfterEsc": c.working])
    }

    /// Negative: Esc twice on an idle session opens the rewind menu, which is why Esc is only for a working one.
    func escIdleTwice() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        c.phase("esc 1")
        c.write("\u{1b}", "Esc"); c.pump(quiet: 0.4, timeout: 2)
        c.phase("esc 2")
        c.write("\u{1b}", "Esc"); c.pump(quiet: 0.6, timeout: 3)
        let note = ["rewindShown": c.text.lowercased().contains("rewind")] as [String: Any]
        c.phase("esc 3 (closes it)")
        c.write("\u{1b}", "Esc"); c.pump(quiet: 0.4, timeout: 2)
        return try finish(c, "esc-idle-twice-negative", notes: note)
    }

    /// A reply sent while the session works is queued (a `queue-operation` enqueue in the transcript).
    func queuedWhileWorking() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        send(Recorder.slowPrompt, on: c)
        c.phase("working")
        c.until(10) { _ in c.working }
        c.phase("queued reply")
        send("queued second message", on: c)
        c.pump(quiet: 0.8, timeout: 4)
        c.phase("interrupt")
        c.write("\u{1b}", "Esc"); c.pump(quiet: 0.8, timeout: 6)
        return try finish(c, "queued-while-working")
    }

    /// ReplySender.takeBackQueued: clear, Up pulls the queue into the box, clear again. `popAll` is read back
    /// from the transcript when NOTCHLING_RECORD_TRANSCRIPT is set.
    func takeBackQueued() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job)
        send(Recorder.slowPrompt, on: c)
        c.until(10) { _ in c.working }
        send("queued to take back", on: c)
        c.pump(quiet: 0.8, timeout: 4)
        c.phase("clear before Up")
        c.write(ReplySender.clearInput, "clearInput"); c.pause(0.3)
        let pressed = Date()
        c.phase("up")
        c.write(Recorder.arrowUp, "Up")
        var popped: Bool?
        if let transcript {
            popped = false
            let deadline = Date().addingTimeInterval(3)
            while Date() < deadline, popped == false {
                c.pump(quiet: 0.3, timeout: 0.5)
                popped = ReplySender.poppedAll("queued to take back", in: transcript, since: pressed.addingTimeInterval(-1))
            }
        } else { c.pump(quiet: 0.8, timeout: 3) }
        c.phase("clear after Up")
        c.write(ReplySender.clearInput, "clearInput"); c.pause(0.3)
        c.phase("interrupt")
        c.write("\u{1b}", "Esc"); c.pump(quiet: 0.8, timeout: 6)
        var notes: [String: Any] = [:]
        if let popped { notes["popAllInTranscript"] = popped }
        return try finish(c, "take-back-queued", notes: notes)
    }

    // MARK: terminal behaviors

    /// The app drops an inherited NO_COLOR for its terminal view.
    func noColor(keep: Bool) throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job, rows: 30, cols: 100, keepNoColor: keep)
        return try finish(c, keep ? "attach-no-color-kept-negative" : "attach-no-color-dropped", notes: ["noColorEnv": keep])
    }

    /// Negative: answering `CSI c` / `CSI > 0 q` cost Claude Code its colors; the answer landed in its input box.
    func queriesAnswered() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job, rows: 30, cols: 100, answerQueries: true)
        c.pump(quiet: 0.5, timeout: 3)
        return try finish(c, "attach-queries-answered-negative")
    }

    /// Chat's terminal on a resize: forkpty makes the child repaint for the new size (posix_spawn did not).
    func resizeForkpty() throws -> Int? {
        try ensureIdle(job)
        let c = try attach(job, rows: 30, cols: 100)
        c.phase("typed")
        for ch in "/plugin" { c.write(String(ch), "type " + String(ch)); c.pause(0.04) }
        c.pause(0.4)
        c.write("\r", "enter")
        let opened = c.until(15) { $0.lines.joined().contains("Discover plugins") }
        c.phase("resize 24x70")
        c.resize(rows: 24, cols: 70)
        let redrawn = c.until(5) { s in s.lines.contains { $0.hasSuffix("╮") && $0.count <= 70 } }
        c.phase("esc")
        c.write("\u{1b}", "Esc"); c.pump(quiet: 0.5, timeout: 5)
        return try finish(c, "terminal-resize-forkpty", notes: ["menuOpened": opened, "redrawnFor70Columns": redrawn])
    }

    // MARK: dialogs

    /// A permission dialog on screen: the dialog check must see it, and a reply must not be typed.
    func dialogPermission() throws -> Int? {
        guard let permissionJob else { return nil }
        let c = try attach(permissionJob)
        let open = ReplySender.dialogOpen(c.screen)
        return try finish(c, "dialog-permission", notes: ["dialogOpen": open, "isAskUserQuestion": AskDialog.page(in: c.screen.lines) != nil])
    }

    /// ReplySender.readQuestion: the first page, Right through the tabs (nothing is answered), Left back.
    func questionReadTabs() throws -> Int? {
        guard let askJob else { return nil }
        let c = try attach(askJob)
        guard ReplySender.dialogOpen(c.screen), let first = AskDialog.page(in: c.screen.lines) else {
            _ = try finish(c, "question-read-tabs", notes: ["questionOpen": false])
            throw Failure.noQuestion
        }
        let total = max(first.tabs.count, 1)
        c.phase("page 1")
        var moved = 0
        for _ in 0..<total {
            let before = AskDialog.page(in: c.screen.lines)?.question
            c.write(Recorder.arrowRight, "Right")
            moved += 1
            c.until(2.5) { s in AskDialog.page(in: s.lines)?.question != before || AskDialog.isReview(s.lines) }
            c.phase("right \(moved)")
        }
        for i in 0..<moved {
            c.write(Recorder.arrowLeft, "Left")
            c.pause(0.15)
            c.phase("left \(i + 1)")
        }
        c.until(2.5) { AskDialog.shows(first.question, in: $0.lines) }
        return try finish(c, "question-read-tabs", notes: ["questionOpen": true, "tabs": first.tabs,
                                                           "backOnFirst": AskDialog.shows(first.question, in: c.screen.lines)])
    }

    /// ReplySender.answer: the page keys with the screen checked after each, then "1" on the review screen.
    /// Consumes the question. Mode: option (digit 1), text (the "Type something" row), multi (options 1 and 2).
    func questionAnswer() throws -> Int? {
        guard let askJob, let mode = answerMode else { return nil }
        let c = try attach(askJob)
        guard ReplySender.dialogOpen(c.screen) else { throw Failure.noQuestion }
        var answered = 0
        while answered < 8, !AskDialog.isReview(c.screen.lines), let page = AskDialog.page(in: c.screen.lines), !page.onReview {
            let item = PendingQuestion.Item(header: page.header, question: page.question, options: page.options,
                                            details: page.details, multiSelect: page.multiSelect)
            let choice: ReplySender.Choice = page.multiSelect ? .options([1, 2]) : (mode == "text" ? .text("Somewhere else") : .option(1))
            c.phase("question \(answered + 1): \(mode)")
            for key in ReplySender.pageKeys(for: choice, in: item) {
                c.write(key, "answer key")
                c.pause(0.15)
            }
            answered += 1
            let before = page.question
            c.until(4) { s in AskDialog.isReview(s.lines) || !ReplySender.dialogOpen(s) || AskDialog.page(in: s.lines)?.question != before }
        }
        if AskDialog.isReview(c.screen.lines) {
            c.phase("review")
            c.write("1", "submit")
            c.until(4) { !ReplySender.dialogOpen($0) }
        }
        return try finish(c, "question-answer-" + mode, notes: ["answered": answered, "dialogGoneAtEnd": !ReplySender.dialogOpen(c.screen)])
    }

    /// ReplySender.trustFolder: the trust prompt of a never-trusted folder; "No, exit" is first, so Down, Enter.
    /// This starts `claude` there and trusts the scratch folder in Claude Code's own list.
    func trustPrompt() throws -> Int? {
        guard let trustDir else { return nil }
        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["NO_COLOR"] = nil
        let c = try Capture(program: ReplySender.claudePath, args: [], folder: trustDir, env: env, rows: 50, cols: 200)
        c.phase("prompt")
        let asking = c.until(20) { $0.lines.contains { $0.contains("Yes, I trust this folder") } }
        c.phase("down")
        c.write(Recorder.arrowDown, "Down"); c.pause(0.3)
        c.phase("enter")
        c.write("\r", "Enter")
        c.pump(quiet: 1, timeout: 8)
        c.phase("quit")
        for _ in 0..<2 { c.write("\u{03}", "Ctrl+C"); c.pause(0.3) }
        c.pump(quiet: 0.5, timeout: 3)
        return try finish(c, "trust-prompt", notes: ["promptSeen": asking])
    }
}
