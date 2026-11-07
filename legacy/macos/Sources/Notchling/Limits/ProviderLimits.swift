import Foundation

/// The limit decisions that read a `UsageReport`, so Codex and Antigravity share Claude's rules
/// (the 90/100 steps, freshness, room, cool-down) without a second copy.
extension LimitLogic {
    typealias NamedWindow = (name: String, window: UsageWindow)

    /// The bars a watcher follows. Codex: its main group ("Codex": the 5h and week bars), not
    /// Code Review or a model-specific extra. Antigravity: every group, since each is a pool of its own.
    static func watchedBars(_ report: UsageReport, primaryGroup: String?) -> [(group: String?, bar: UsageReport.Bar)] {
        var groups = report.groups
        if let primaryGroup {
            groups = [groups.first { $0.name == primaryGroup } ?? groups.first].compactMap { $0 }
        }
        return groups.flatMap { g in g.bars.map { (g.name, $0) } }
    }

    /// The windows to announce and switch on, named the way a message reads them ("5-hour", "weekly",
    /// or "Gemini 3 Pro weekly" when the group matters).
    static func watchedWindows(_ report: UsageReport, primaryGroup: String?) -> [NamedWindow] {
        watchedBars(report, primaryGroup: primaryGroup).map { group, bar in
            let kind: String
            switch bar.kind {
            case .session: kind = "5-hour"
            case .week: kind = "weekly"
            case .other: kind = bar.label.lowercased()
            }
            let name = primaryGroup == nil ? [group, kind].compactMap { $0 }.joined(separator: " ") : kind
            return (name, bar.window)
        }
    }

    /// Steps every window's mark and returns the highest level that fired now (the later window wins a
    /// tie). Claude's watcher and the provider watchers both announce through this.
    static func announcement(marks: inout [String: Mark], keyPrefix: String, windows: [(name: String, window: UsageWindow?)],
                             skip90: Bool) -> (level: Int, name: String, window: UsageWindow)? {
        var fired: (level: Int, name: String, window: UsageWindow)?
        for (name, window) in windows {
            guard let window else { continue }
            let key = "\(keyPrefix)|\(name)"
            let r = step(marks[key] ?? .init(), percent: window.percent, resetsAt: window.resetsAt, skip90: skip90)
            marks[key] = r.mark
            guard let level = r.fire, level >= (fired?.level ?? 0) else { continue }
            fired = (level, name, window)
        }
        return fired
    }

    enum SwitchDecision: Equatable {
        /// Nothing pushes toward a switch, or one happened too recently.
        case stay
        /// A saved account has room, but the provider is working and a switch would interrupt it.
        case busy
        case noRoom
        case switchTo(String)
    }

    /// Whether the account in use should be switched, and to which. `canSwitchNow` runs only once a
    /// target exists (it may be a process listing).
    static func decideSwitch(windows: [NamedWindow], switchAt: Int, activeID: String?, candidates: [Candidate],
                             blockedUntil: [String: Date], lastSwitch: Date?, now: Date,
                             canSwitchNow: () -> Bool) -> SwitchDecision {
        guard windows.contains(where: { $0.window.percent >= Double(switchAt) }),
              maySwitch(lastSwitch: lastSwitch, now: now) else { return .stay }
        guard let target = pickTarget(candidates, activeID: activeID, now: now, blockedUntil: blockedUntil) else { return .noRoom }
        return canSwitchNow() ? .switchTo(target.id) : .busy
    }
}

extension LimitLogic.Candidate {
    /// From a report: `session` is the highest short or unknown window, `week` the highest weekly one.
    init(id: String, report: UsageReport?, primaryGroup: String?, needsLogin: Bool = false) {
        let bars = report.map { LimitLogic.watchedBars($0, primaryGroup: primaryGroup).map(\.bar) } ?? []
        func top(_ pick: (UsageReport.Bar) -> Bool) -> Double? { bars.filter(pick).map(\.percent).max() }
        self.init(id: id, fetched: report?.fetched ?? .distantPast,
                  session: top { $0.kind != .week }, week: top { $0.kind == .week }, needsLogin: needsLogin)
    }
}

extension CodexStore {
    /// A switch only runs while nothing would be interrupted: no CLI or Chat turn (`codex` processes),
    /// no ChatGPT app to quit. The managed app server alone doesn't count; it restarts quietly.
    static func idleForAutoSwitch(_ work: (app: Bool, server: Bool, sessions: Int)) -> Bool {
        !work.app && work.sessions == 0
    }
}

/// Watches the active account of a provider other than Claude Code. Near a limit it says so; with
/// Notify and Switch (only where the provider allows it) it moves to the saved account with the most
/// room. It reads limits the provider already fetched, never renews a token, and a switch is the
/// provider's own `switchTo`.
final class ProviderLimitWatcher {
    struct Setup {
        var label: String
        var modeKey: String
        var modes: [LimitMode]
        var defaultMode: LimitMode
        /// Codex: "Codex". Nil follows every group (Antigravity).
        var primaryGroup: String?
        var busyText: String
    }

    let provider: any AccountProvider
    let setup: Setup
    private let switchAt: () -> Int
    private let canSwitchNow: () -> Bool
    private let announce: (String) -> Void

    private var marks: [String: LimitLogic.Mark] = [:]
    private var blocked: [String: Date] = [:]
    private var lastSwitch: Date?
    private var lastNote: Date?
    private var busySince: Date?
    private var switching = false

    init(provider: any AccountProvider, setup: Setup, switchAt: @escaping () -> Int,
         canSwitchNow: @escaping () -> Bool = { true }, announce: @escaping (String) -> Void) {
        self.provider = provider
        self.setup = setup
        self.switchAt = switchAt
        self.canSwitchNow = canSwitchNow
        self.announce = announce
    }

    var mode: LimitMode {
        get {
            let stored = UserDefaults.standard.string(forKey: setup.modeKey).flatMap(LimitMode.init(rawValue:))
            return stored.flatMap { setup.modes.contains($0) ? $0 : nil } ?? setup.defaultMode
        }
        set { if setup.modes.contains(newValue) { UserDefaults.standard.set(newValue.rawValue, forKey: setup.modeKey) } }
    }

    /// Called from the polling loop.
    func check(now: Date = Date()) {
        guard mode != .off, !switching, let active = provider.state.active,
              let report = provider.usageReport(for: active.id),
              now.timeIntervalSince(report.fetched) <= LimitLogic.inUseFreshFor else { return }
        let windows = LimitLogic.watchedWindows(report, primaryGroup: setup.primaryGroup)
        let willSwitch = mode == .notifyAndSwitch
        let skip90 = willSwitch && switchAt() <= 90
        let fired = LimitLogic.announcement(marks: &marks, keyPrefix: active.id,
                                            windows: windows.map { ($0.name, Optional($0.window)) }, skip90: skip90)
        let text = fired.map { f -> String in
            let pct = Int(f.window.percent.rounded())
            return f.level >= 100
                ? "\(setup.label): \(active.email) has used its \(f.name) limit (\(pct)%)\(Self.resets(f.window))."
                : "\(setup.label): \(active.email) is at \(pct)% of its \(f.name) limit\(Self.resets(f.window))."
        }
        guard willSwitch else {
            if let text { announce(text) }
            return
        }

        let candidates = provider.state.accounts.map {
            LimitLogic.Candidate(id: $0.id, report: provider.usageReport(for: $0.id), primaryGroup: setup.primaryGroup,
                                 needsLogin: $0.needsLogin || !$0.switchable)
        }
        // The idle check may list processes; while busy, look again at most once a minute.
        let idle: () -> Bool = { [self] in
            if let since = busySince, now.timeIntervalSince(since) < 60 { return false }
            let ok = canSwitchNow()
            busySince = ok ? nil : now
            return ok
        }
        switch LimitLogic.decideSwitch(windows: windows, switchAt: switchAt(), activeID: active.id, candidates: candidates,
                                       blockedUntil: blocked, lastSwitch: lastSwitch, now: now, canSwitchNow: idle) {
        case .stay:
            if let text { announce(text) }
        case .busy:
            note(text, "\(setup.busyText) Switch accounts yourself when it's done.", now: now)
        case .noRoom:
            note(text, "No other \(setup.label) account has room left.", now: now)
        case .switchTo(let id):
            guard let to = provider.state.accounts.first(where: { $0.id == id }) else { return }
            let until = LimitLogic.blockedUntil(session: nil, week: nil, other: windows.map(\.window),
                                                at: Double(switchAt()), now: now)
            switching = true
            Task { @MainActor [weak self] in
                guard let self else { return }
                defer { self.switching = false }
                do {
                    try await self.provider.switchTo(id)
                    self.lastSwitch = Date()
                    self.blocked[active.id] = until
                    let room = self.provider.usageReport(for: id).map {
                        LimitLogic.watchedWindows($0, primaryGroup: self.setup.primaryGroup).map(\.window.percent).max() ?? 0
                    } ?? 0
                    self.announce("\(self.setup.label): switched from \(active.email) to \(to.email) (\(Int(room.rounded()))% used) because of the usage limit.")
                } catch {
                    self.announce("\(self.setup.label): couldn't switch accounts: \(error.localizedDescription)")
                }
            }
        }
    }

    /// The climb itself when it just crossed 90 or 100, with the reason no switch followed; otherwise
    /// the reason alone, at most once per half hour.
    private func note(_ text: String?, _ reason: String, now: Date) {
        if let text {
            lastNote = now
            announce("\(text) \(reason)")
        } else if lastNote.map({ now.timeIntervalSince($0) > LimitLogic.switchGap }) ?? true {
            lastNote = now
            announce("\(setup.label): \(reason)")
        }
    }

    private static func resets(_ window: UsageWindow) -> String {
        window.resetsAt.map { ", resets \(clock($0))" } ?? ""
    }

    private static func clock(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = Calendar.current.isDateInToday(date) ? "h:mm a" : "EEE h:mm a"
        return f.string(from: date)
    }
}
