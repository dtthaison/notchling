import AppKit

/// After the Mac wakes, tells background agents whose reply was cut off by sleep to carry on.
///
/// Closing the lid mid-reply ends the turn with "API Error: Your computer went to sleep mid-response"
/// (an `isApiErrorMessage` entry with `streamCutCause` "stream_suspended"), and the session then sits
/// idle until someone writes to it. Only background jobs can be written to (`claude attach`); terminal
/// sessions belong to their window, and `claude -p` runs have ended.
final class WakeResumer {
    static let prompt = "The Mac went to sleep and cut off your last reply. Continue where you stopped."
    /// Give the network and the background service time to come back before writing.
    private static let settle: TimeInterval = 20
    /// Look for cut-off replies this long after a wake; later ones weren't caused by this sleep.
    private static let window: TimeInterval = 30 * 60

    private var wokeAt: Date?
    /// Error entries already answered, so each cut-off gets one nudge.
    private var handled = Set<String>()
    private var sending = Set<String>()
    private var observer: NSObjectProtocol?

    var enabled: Bool {
        get { UserDefaults.standard.object(forKey: "continueAfterSleep") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "continueAfterSleep") }
    }

    init() {
        observer = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            self?.wokeAt = Date()
        }
    }

    deinit {
        if let observer { NSWorkspace.shared.notificationCenter.removeObserver(observer) }
    }

    /// Called from the polling loop with the current agents.
    func check(_ agents: [Agent]) {
        guard enabled, let woke = wokeAt else { return }
        let since = Date().timeIntervalSince(woke)
        guard since >= Self.settle else { return }
        guard since <= Self.window else { wokeAt = nil; return }

        for agent in agents where agent.group != .working {
            guard case .job(let jobID) = agent.target, !sending.contains(agent.id),
                  let cut = TranscriptReader.sleepCut(for: agent), !handled.contains(cut) else { continue }
            handled.insert(cut)
            sending.insert(agent.id)
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                try? ReplySender.send(Self.prompt, toJob: jobID)
                DispatchQueue.main.async { self?.sending.remove(agent.id) }
            }
        }
    }
}
