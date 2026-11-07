import AppKit
import Combine
import SwiftUI

/// The second opinions asked in Chat, per session: runs `Notchling --delegate … --json` (this app's own
/// executable, so Stop is a signal and the Codex and Antigravity engines stay the CLI's one copy), keeps
/// the answers in memory and in a small JSON file so they survive a relaunch.
final class SecondOpinionStore: ObservableObject {
    @Published private(set) var opinions: [SecondOpinion] = []

    private let fileURL: URL?
    private let keep = 100

    /// A running child and whether Cancel was pressed, shared with the thread that waits for it.
    private final class Job {
        let process = Process()
        let promptFile: String
        var cancelled = false
        init(promptFile: String) { self.promptFile = promptFile }
    }
    private var jobs: [String: Job] = [:]
    private var quitObserver: NSObjectProtocol?

    static var defaultFile: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Notchling/second-opinions.json")
    }

    /// `fileURL` nil keeps everything in memory (tests).
    init(fileURL: URL? = SecondOpinionStore.defaultFile) {
        self.fileURL = fileURL
        if let fileURL, let data = try? Data(contentsOf: fileURL),
           let saved = try? JSONDecoder().decode([SecondOpinion].self, from: data) {
            // A run that was going when the app quit has no process any more.
            opinions = saved.map { run in
                var run = run
                if run.status == .running { run.status = .failed; run.error = "Notchling quit before it finished." }
                return run
            }
        }
        quitObserver = NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil,
                                                              queue: .main) { [weak self] _ in self?.stopAll() }
    }

    deinit { if let quitObserver { NotificationCenter.default.removeObserver(quitObserver) } }

    func runs(session: String, anchors: [String]) -> [SecondOpinion] {
        opinions.filter { $0.session == session && anchors.contains($0.anchor) }.sorted { $0.started < $1.started }
    }

    // MARK: Running

    struct Request {
        var session: String
        var anchor: String
        var subject: String
        var tool: Delegate.Tool
        var model: String?
        var effort: String?
        var folder: String
        var prompt: String
    }

    @discardableResult
    func start(_ request: Request) -> SecondOpinion {
        let run = SecondOpinion(session: request.session, anchor: request.anchor, subject: request.subject,
                                tool: request.tool.rawValue, model: request.model, effort: request.effort, folder: request.folder)
        opinions.append(run)
        save()
        launch(run, prompt: request.prompt)
        return run
    }

    private func launch(_ run: SecondOpinion, prompt: String) {
        let file = NSTemporaryDirectory() + "notchling-second-opinion-\(run.id).txt"
        guard FileManager.default.createFile(atPath: file, contents: Data(prompt.utf8), attributes: [.posixPermissions: 0o600]) else {
            return fail(run.id, "Couldn't write the prompt.")
        }
        let job = Job(promptFile: file)
        let p = job.process
        p.executableURL = URL(fileURLWithPath: Bundle.main.executablePath ?? CommandLine.arguments[0])
        p.arguments = SecondOpinion.arguments(tool: run.toolValue, model: run.model, effort: run.effort, folder: run.folder, promptFile: file)
        p.currentDirectoryURL = URL(fileURLWithPath: run.folder)
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch {
            try? FileManager.default.removeItem(atPath: file)
            return fail(run.id, error.localizedDescription)
        }
        jobs[run.id] = job
        let id = run.id
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let data = out.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            let output = String(decoding: data, as: UTF8.self)
            DispatchQueue.main.async { self?.finished(id, output: output) }
        }
    }

    private func finished(_ id: String, output: String) {
        let job = jobs.removeValue(forKey: id)
        if let job { try? FileManager.default.removeItem(atPath: job.promptFile) }
        update(id) { $0.finish(with: SecondOpinionResult.parse(output), output: output, cancelled: job?.cancelled ?? false) }
    }

    private func fail(_ id: String, _ message: String) {
        update(id) { $0.status = .failed; $0.error = message }
    }

    /// Stops the run now: the card turns to Cancelled at once, and the tool's partial answer, if any,
    /// is added when the command has ended. The command interrupts its tool on SIGTERM.
    func cancel(_ id: String) {
        guard let job = jobs[id], !job.cancelled else { return }
        job.cancelled = true
        update(id) { $0.status = .cancelled }
        job.process.terminate()
        let pid = job.process.processIdentifier
        DispatchQueue.global().asyncAfter(deadline: .now() + 8) { if job.process.isRunning { kill(pid, SIGKILL) } }
    }

    func remove(_ id: String) {
        cancel(id)
        opinions.removeAll { $0.id == id }
        save()
    }

    private func stopAll() {
        for job in jobs.values {
            job.cancelled = true
            job.process.terminate()
        }
    }

    private func update(_ id: String, _ change: (inout SecondOpinion) -> Void) {
        guard let i = opinions.firstIndex(where: { $0.id == id }) else { return }
        var run = opinions[i]
        change(&run)
        opinions[i] = run
        save()
    }

    private func save() {
        guard let fileURL else { return }
        let done = opinions.filter { $0.status != .running }
        let running = opinions.filter { $0.status == .running }
        let list = Array((done.suffix(max(keep - running.count, 0)) + running).sorted { $0.started < $1.started })
        guard let data = try? JSONEncoder().encode(list) else { return }
        try? FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: fileURL, options: .atomic)
    }
}

// MARK: - Chat model

extension ChatModel {
    /// Opens the session the other tool made for a second opinion, in Chat beside the others.
    func openDelegatedSession(_ run: SecondOpinion) {
        guard let id = run.delegatedSession else { return }
        let tool: SessionTool = run.toolValue == .codex ? .codex : .antigravity
        let meta = SessionMeta(key: "\(tool.key):\(id)", toolKey: tool.key, sessionID: id,
                               title: "Second opinion: \(run.subject)", folder: run.folder, path: "", configDir: nil,
                               modified: Date().timeIntervalSince1970)
        selection = prepareSession(meta)
    }
}

// MARK: - Environment

/// What the Second Opinion actions need from the Chat window: the store, the session they're asked in
/// and how to open a delegated session. Nil where there's no Chat (the notch).
struct SecondOpinionContext {
    let store: SecondOpinionStore
    let session: String
    let folder: String
    let open: (SecondOpinion) -> Void
}

private struct SecondOpinionKey: EnvironmentKey { static let defaultValue: SecondOpinionContext? = nil }

extension EnvironmentValues {
    var secondOpinion: SecondOpinionContext? {
        get { self[SecondOpinionKey.self] }
        set { self[SecondOpinionKey.self] = newValue }
    }
}
