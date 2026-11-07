import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    /// Where a new background session can run: the main login, each lane, then each free saved account
    /// (picking one makes a lane for it). `accountID` nil means the main login. Titled by email only:
    /// where a session runs is Notchling's business, and a lane's name goes stale after a switch.
    struct SessionTarget { let title: String; let accountID: String? }

    var sessionTargets: [SessionTarget] {
        guard let accounts else { return [] }
        var out = [SessionTarget(title: accounts.active?.email ?? "Main Login", accountID: nil)]
        out += LaneStore.all().map { SessionTarget(title: $0.email, accountID: $0.accountID) }
        out += freeAccounts.map { SessionTarget(title: $0.email, accountID: $0.id) }
        return out
    }

    /// Project folders for a new session: those of the sessions in the list (newest first), then ones added
    /// with Add Project Folder…, that still exist.
    var projectFolders: [String] {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        var out: [String] = []
        let live = notch.agents.sorted { $0.since > $1.since }.map(\.cwd)
        for folder in live + (UserDefaults.standard.stringArray(forKey: "chatProjectFolders") ?? [])
        where !folder.isEmpty && folder != home && !out.contains(folder) && FileManager.default.fileExists(atPath: folder) {
            out.append(folder)
        }
        return out
    }

    func addProjectFolder(_ folder: String) {
        var saved = UserDefaults.standard.stringArray(forKey: "chatProjectFolders") ?? []
        saved.removeAll { $0 == folder }
        saved.insert(folder, at: 0)
        UserDefaults.standard.set(Array(saved.prefix(30)), forKey: "chatProjectFolders")
    }

    /// The folder's name, with its parent when another listed folder has the same name.
    static func folderTitle(_ folder: String, among all: [String]) -> String {
        let url = URL(fileURLWithPath: folder)
        let name = url.lastPathComponent
        let clash = all.contains { $0 != folder && URL(fileURLWithPath: $0).lastPathComponent == name }
        return clash ? url.deletingLastPathComponent().lastPathComponent + "/" + name : name
    }

    /// Starts an idle background session (`claude --bg`) in `folder` on the target's account, in its lane,
    /// and selects it once it shows up. Errors go to `errorKey`'s banner, or an alert without one.
    /// `done` (with nil on success, else what went wrong) replaces the alert: New Session shows it in the
    /// sheet. `trust` first accepts Claude Code's trust prompt for the folder.
    func newBackgroundSession(in folder: String, on accountID: String?, errorKey: String? = nil,
                              trust: Bool = false, done: ((String?) -> Void)? = nil) {
        let fail: (String) -> Void = { [weak self] text in
            if let done { done(text); return }
            if let errorKey { self?.accountErrors[errorKey] = text; return }
            let alert = NSAlert()
            alert.messageText = "Couldn't start the session"
            alert.informativeText = text
            alert.runModal()
        }
        if let errorKey { accountErrors[errorKey] = nil }
        guard let accounts else { return fail("Claude Code accounts aren't loaded yet.") }
        let start: (Lane?) -> Void = { [weak self] lane in
            DispatchQueue.global(qos: .userInitiated).async {
                if trust, !ReplySender.trustFolder(folder, configDir: lane?.path) {
                    DispatchQueue.main.async { fail("Couldn't trust the folder. Run claude there once and accept its prompt.") }
                    return
                }
                let result = Self.startBackground(in: folder, configDir: lane?.path)
                DispatchQueue.main.async {
                    switch result {
                    case .success(let shortID): done?(nil); self?.selectWhenListed(shortID)
                    case .failure(let error): fail(error.localizedDescription)
                    }
                    self?.refreshLanes(force: true)
                }
            }
        }
        guard let accountID, accountID != accounts.activeID else { return start(nil) }
        accounts.openLane(for: accountID) { result in
            switch result {
            case .success(let lane): start(lane)
            case .failure(let error): fail(error.localizedDescription)
            }
        }
    }

    /// Moves one background session to another account: stops it (`claude stop` keeps the conversation),
    /// then resumes the same conversation in the background with the target's config folder (`accountID`
    /// nil is the main login; a free account gets a lane). Every other session stays where it is. The
    /// prompt cache doesn't follow (it belongs to the old account), so the first reply re-reads it all.
    func moveSession(_ agent: Agent, to accountID: String?, title: String) {
        guard case .job(let shortID) = agent.target, let accounts, let transcript = agent.transcriptPath else { return }
        let conversation = URL(fileURLWithPath: transcript).deletingPathExtension().lastPathComponent
        let alert = NSAlert()
        alert.messageText = "Move “\(agent.name)” to \(title)?"
        alert.informativeText = (agent.group == .working ? "It's working now; the current step is cut off. " : "")
            + "It stops and resumes the same conversation on that account. Other sessions stay where they are. "
            + "The first reply there re-reads the whole conversation, since the prompt cache stays with the old account."
        alert.addButton(withTitle: "Move")
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }

        let id = agent.id
        accountErrors[id] = nil
        let flags = Self.launchFlags(forJob: shortID, name: agent.name)
        let cwd = agent.cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser.path : agent.cwd
        let move: (Lane?) -> Void = { [weak self] lane in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = Self.stopJob(shortID).flatMap {
                    Self.startBackground(in: cwd, configDir: lane?.path,
                                         arguments: Self.resumeArguments(conversation, job: shortID, in: lane?.path, flags: flags))
                }
                DispatchQueue.main.async {
                    switch result {
                    case .success(let newID): self?.selectWhenListed(newID)
                    case .failure(let error): self?.accountErrors[id] = "Couldn't move it: \(error.localizedDescription)"
                    }
                    self?.refreshLanes(force: true)
                }
            }
        }
        guard let accountID, accountID != accounts.activeID else { return move(nil) }
        accounts.openLane(for: accountID) { [weak self] result in
            switch result {
            case .success(let lane): move(lane)
            case .failure(let error): self?.accountErrors[id] = error.localizedDescription
            }
        }
    }

    /// The new job's short id from `claude --bg` output: "backgrounded · 6ce4c19c · name (idle — …)", or
    /// since 2.1.318 "backgrounded · 6220cc2b (idle — …)", maybe after a note line.
    static func jobID(inLaunchOutput text: String) -> String? {
        for line in text.split(separator: "\n") {
            let parts = line.components(separatedBy: " · ")
            guard parts.count > 1,
                  let id = parts[1].split(whereSeparator: \.isWhitespace).first.map(String.init) else { continue }
            if id.count == 8, id.allSatisfy(\.isHexDigit) { return id }
        }
        return nil
    }

    /// `--resume` for a session moving into `configDir`. Where that folder already has a background job
    /// for the conversation (the copy left there by an earlier move), Claude Code continues that job with
    /// its own saved options, and since 2.1.318 any flags passed start a separate copy instead; so flags
    /// go along only when there's no job to continue.
    static func resumeArguments(_ conversation: String, job shortID: String, in configDir: String?, flags: [String]) -> [String] {
        let dir = configDir.map { URL(fileURLWithPath: $0) } ?? LaneStore.mainDir
        let existing = FileManager.default.fileExists(atPath: dir.appendingPathComponent("jobs/\(shortID)/state.json").path)
        return (existing ? [] : flags) + ["--bg", "--resume", conversation]
    }

    /// The flags the job was started with (name, model, permission mode), so the moved session keeps them.
    static func launchFlags(forJob shortID: String, name: String) -> [String] {
        let dir = AgentMonitor.configDir(forJob: shortID).map { URL(fileURLWithPath: $0) } ?? LaneStore.mainDir
        let url = dir.appendingPathComponent("jobs/\(shortID)/state.json")
        guard let data = try? Data(contentsOf: url),
              let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let flags = obj["respawnFlags"] as? [String] else { return ["--name", name] }
        // Never carry a resume or background flag twice.
        var out: [String] = []
        var skipNext = false
        for flag in flags {
            if skipNext { skipNext = false; continue }
            if flag == "--resume" || flag == "-r" { skipNext = true; continue }
            if flag == "--bg" || flag == "--background" { continue }
            out.append(flag)
        }
        return out
    }

    /// Runs `p` with its output in a temporary file rather than a pipe, and waits for it to exit (at most
    /// `timeout`). The first `claude --bg` in a lane starts a background host that inherits the output, so
    /// reading a pipe to its end would wait as long as that host lives.
    static func runCapturing(_ p: Process, timeout: TimeInterval) -> Result<Data, Error> {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-\(UUID().uuidString).out")
        guard FileManager.default.createFile(atPath: url.path, contents: nil),
              let handle = try? FileHandle(forWritingTo: url) else {
            return .failure(AccountError("Couldn't make a file for claude's output."))
        }
        defer { try? FileManager.default.removeItem(at: url) }
        p.standardOutput = handle
        p.standardError = handle
        p.standardInput = FileHandle.nullDevice
        let exited = DispatchSemaphore(value: 0)
        p.terminationHandler = { _ in exited.signal() }
        do { try p.run() } catch { try? handle.close(); return .failure(error) }
        try? handle.close()
        if exited.wait(timeout: .now() + timeout) == .timedOut {
            p.terminate()
            return .failure(AccountError("claude \(p.arguments?.first ?? "") didn't finish in \(Int(timeout)) s."))
        }
        return .success((try? Data(contentsOf: url)) ?? Data())
    }

    static func stopJob(_ shortID: String) -> Result<Void, Error> {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
        p.arguments = ["stop", shortID]
        p.environment = AgentMonitor.environment(forJob: shortID)
        let data: Data
        switch runCapturing(p, timeout: 60) {
        case .success(let d): data = d
        case .failure(let e): return .failure(e)
        }
        guard p.terminationStatus == 0 else {
            let message = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            return .failure(AccountError(message.isEmpty ? "claude stop exited with code \(p.terminationStatus)." : message))
        }
        // Let the host let go of the transcript before the resume opens it.
        Thread.sleep(forTimeInterval: 1.5)
        return .success(())
    }

    static func startBackground(in folder: String, configDir: String?,
                                        arguments: [String] = ["--bg"]) -> Result<String, Error> {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
        p.arguments = arguments
        p.currentDirectoryURL = URL(fileURLWithPath: folder)
        var env = ProcessInfo.processInfo.environment
        env[LaneStore.variable] = configDir
        p.environment = env
        let data: Data
        switch runCapturing(p, timeout: 60) {
        case .success(let d): data = d
        case .failure(let e): return .failure(e)
        }
        let text = String(decoding: data, as: UTF8.self)
        if let id = jobID(inLaunchOutput: text) { return .success(id) }
        let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return .failure(AccountError(message.isEmpty ? "claude --bg exited with code \(p.terminationStatus)." : message))
    }

    /// Opens the new background session `shortID` in Chat once the session list has it (up to 30 s). Its
    /// id starts as the short id and becomes the session id once Claude Code writes it, so keep following
    /// it until then, unless you've picked another session meanwhile.
    private func selectWhenListed(_ shortID: String, tries: Int = 30, picked: String? = nil) {
        if let picked, selection != picked { return }
        var chosen = picked
        if let agent = notch.agents.first(where: { if case .job(let id) = $0.target { return id == shortID }; return false }) {
            if selection != agent.id {
                selection = agent.id
                focusRequest += 1
            }
            chosen = agent.id
            if agent.id != shortID { return }
        }
        guard tries > 0 else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            self?.selectWhenListed(shortID, tries: tries - 1, picked: chosen)
        }
    }
}
