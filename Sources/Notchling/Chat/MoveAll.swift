import AppKit

/// What putting every session on one account does. An account is live in one place only, so the sessions
/// can't follow it into a lane: background sessions in lanes stop and resume on the main login, the lane
/// holding the account closes (giving the login back), and the main login switches to it.
struct MoveAllPlan: Equatable {
    struct Job: Equatable {
        let agentID: String
        let jobID: String
        let name: String
        let cwd: String
        let transcript: String
    }

    /// Background sessions in lanes, to stop and resume on the main login.
    var jobs: [Job] = []
    /// The lane that holds the account, closed so the main login can take it.
    var closeLane: String?
    /// The account the main login switches to; nil when it's on it already.
    var switchMain: String?
    /// Terminal sessions in the lane that closes: they lose their login, and can't be moved.
    var signedOut: [String] = []

    var isEmpty: Bool { jobs.isEmpty && closeLane == nil && switchMain == nil }

    /// `target` nil means the main login's own account.
    static func make(agents: [Agent], lanes: [Lane], activeID: String?, target: String?) -> MoveAllPlan {
        let target = target == activeID ? nil : target
        let lanePaths = Set(lanes.map(\.path))
        var plan = MoveAllPlan()
        plan.switchMain = target
        let closing = target.flatMap { id in lanes.first { $0.accountID == id } }
        plan.closeLane = closing?.name
        for agent in agents {
            guard let dir = agent.configDir, lanePaths.contains(dir) else { continue }
            switch agent.target {
            case .job(let jobID):
                // A stopped session stays where it is; it was stopped on purpose.
                guard let transcript = agent.transcriptPath, agent.label != "Stopped" else { continue }
                plan.jobs.append(Job(agentID: agent.id, jobID: jobID, name: agent.name, cwd: agent.cwd, transcript: transcript))
            case .process:
                if dir == closing?.path { plan.signedOut.append(agent.name) }
            case .resume, .codex, .antigravity:
                break
            }
        }
        return plan
    }
}

extension ChatModel {
    /// Puts every session on `accountID` (nil: the main login's account), after asking. `from` is the
    /// session whose menu it came from, where a failure that isn't one session's shows.
    func moveAll(to accountID: String?, email: String, from agent: Agent) {
        guard let accounts else { return }
        let plan = MoveAllPlan.make(agents: notch.agents, lanes: LaneStore.all(), activeID: accounts.activeID, target: accountID)
        guard !plan.isEmpty else { return }

        let alert = NSAlert()
        alert.messageText = "Move every session to \(email)?"
        var lines: [String] = []
        if !plan.jobs.isEmpty {
            let n = plan.jobs.count
            lines.append("\(n) background session\(n == 1 ? "" : "s") in lanes stop\(n == 1 ? "s" : "") and resume on the main login; a working one is cut off mid-step.")
        }
        if plan.switchMain != nil { lines.append("The main login switches to \(email), and its sessions go with it.") }
        if let lane = plan.closeLane {
            lines.append("The lane holding \(email) closes (\(lane)).")
            if !plan.signedOut.isEmpty {
                lines.append("Terminal sessions in it are signed out: \(plan.signedOut.joined(separator: ", ")).")
            }
        }
        lines.append("Each moved session re-reads its whole conversation on its first reply.")
        alert.informativeText = lines.joined(separator: " ")
        alert.addButton(withTitle: "Move All")
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }

        let fromID = agent.id
        accountErrors[fromID] = nil
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var errors: [String: String] = [:]
            // 1. Stop each lane session, keeping how it was started.
            var stopped: [(job: MoveAllPlan.Job, flags: [String])] = []
            for job in plan.jobs {
                let flags = Self.launchFlags(forJob: job.jobID, name: job.name)
                switch Self.stopJob(job.jobID) {
                case .success: stopped.append((job, flags))
                case .failure(let error): errors[job.agentID] = "Couldn't move it: \(error.localizedDescription)"
                }
            }
            // 2. Give the account back from its lane, 3. then switch the main login to it.
            let wait = DispatchSemaphore(value: 0)
            if let lane = plan.closeLane {
                DispatchQueue.main.async { accounts.closeLane(lane) { wait.signal() } }
                wait.wait()
            }
            if let id = plan.switchMain {
                var failure: Error?
                DispatchQueue.main.async { accounts.switchTo(id) { failure = $0; wait.signal() } }
                wait.wait()
                // The sessions still resume, on the main login's current account, rather than stay stopped.
                if let failure { errors[fromID] = "Couldn't switch the main login: \(failure.localizedDescription)" }
            }
            // 4. Resume each one on the main login.
            for (job, flags) in stopped {
                let conversation = URL(fileURLWithPath: job.transcript).deletingPathExtension().lastPathComponent
                let cwd = job.cwd.isEmpty ? home : job.cwd
                if case .failure(let error) = Self.startBackground(in: cwd, configDir: nil,
                                                                   arguments: Self.resumeArguments(conversation, job: job.jobID, in: nil, flags: flags)) {
                    errors[job.agentID] = "Stopped, but couldn't resume it on the main login: \(error.localizedDescription)"
                }
            }
            DispatchQueue.main.async {
                guard let self else { return }
                for (id, text) in errors { self.accountErrors[id] = text }
                self.refreshLanes(force: true)
            }
        }
    }
}
