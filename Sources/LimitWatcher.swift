import AppKit
import SwiftUI
import UserNotifications

/// What to do when the Claude Code account in use nears a usage limit.
enum LimitMode: String, CaseIterable {
    case off, notify, notifyAndSwitch

    var title: String {
        switch self {
        case .off: return "Off"
        case .notify: return "Notify"
        case .notifyAndSwitch: return "Notify and Switch"
        }
    }
}

/// The decisions, as pure functions so they can be checked without the app running.
enum LimitLogic {
    static let switchRange = 80...99
    static let defaultSwitchAt = 95
    /// An account must have limits fetched this recently, and both under `roomBelow`, to be switched to.
    static let freshFor: TimeInterval = 10 * 60
    static let roomBelow = 80.0
    static let switchGap: TimeInterval = 30 * 60

    /// One window's notification state: the highest level already announced, and the window it belongs to.
    struct Mark: Equatable {
        var level = 0
        var resetsAt: Date?
    }

    /// Which level (90 or 100) a reading has reached.
    static func level(_ percent: Double) -> Int { percent >= 100 ? 100 : percent >= 90 ? 90 : 0 }

    /// Announces each level once per window. The mark falls back when usage drops or the window resets,
    /// so the next climb is announced again. `skip90`: the account is about to be switched at 90, so a
    /// separate warning would only repeat it.
    static func step(_ mark: Mark, percent: Double, resetsAt: Date?, skip90: Bool) -> (mark: Mark, fire: Int?) {
        var m = mark
        if let a = m.resetsAt, let b = resetsAt, abs(a.timeIntervalSince(b)) > 120 { m.level = 0 }
        m.resetsAt = resetsAt
        let now = level(percent)
        if now < m.level { m.level = now }
        guard now > m.level else { return (m, nil) }
        m.level = now
        return (m, now == 90 && skip90 ? nil : now)
    }

    struct Candidate: Equatable {
        let id: String
        let fetched: Date
        let session: Double?
        let week: Double?
        var needsLogin = false
    }

    /// The saved account with the most room: fresh data, both windows under 80%, not the one in use, and
    /// not one we left that hasn't reset yet. Lowest max(session, week) wins; ties go to the first listed.
    static func pickTarget(_ all: [Candidate], activeID: String?, now: Date, blockedUntil: [String: Date]) -> Candidate? {
        all.filter { c in
            guard c.id != activeID, !c.needsLogin, now.timeIntervalSince(c.fetched) <= freshFor,
                  c.session != nil || c.week != nil,
                  (c.session ?? 0) < roomBelow, (c.week ?? 0) < roomBelow else { return false }
            return blockedUntil[c.id].map { $0 <= now } ?? true
        }
        .min { max($0.session ?? 0, $0.week ?? 0) < max($1.session ?? 0, $1.week ?? 0) }
    }

    /// At most one automatic switch per half hour.
    static func maySwitch(lastSwitch: Date?, now: Date) -> Bool {
        lastSwitch.map { now.timeIntervalSince($0) >= switchGap } ?? true
    }

    /// When the account we leave becomes usable again: the latest reset among its windows at or over `at`.
    static func blockedUntil(session: UsageWindow?, week: UsageWindow?, other: [UsageWindow] = [], at: Double, now: Date) -> Date {
        let over = ([session, week].compactMap { $0 } + other).filter { $0.percent >= at }.compactMap(\.resetsAt)
        return over.max() ?? now.addingTimeInterval(5 * 3600)
    }
}

/// Watches the account Claude Code is using. Near a limit it says so; with Notify and Switch it moves
/// Claude Code to the saved account with the most room, then restarts and nudges background agents that
/// stopped on the limit so they read the new login.
final class LimitWatcher {
    static let nudge = "Switched to another account after the usage limit. Continue where you stopped."
    /// Only limit errors this fresh start a switch; older ones were someone else's.
    private static let freshHit: TimeInterval = 10 * 60
    /// Errors this old still get their job restarted once a switch happens.
    private static let nudgeWindow: TimeInterval = 12 * 3600
    private static let transcriptEvery: TimeInterval = 15

    private let accounts: AccountStore
    /// Shows a message to the user: sound, notch peek, macOS notification.
    private let announce: (String) -> Void

    private var marks: [String: LimitLogic.Mark] = [:]
    private var blocked: [String: Date] = [:]
    private var lastSwitch: Date?
    private var switching = false
    private var lastNoRoom: Date?
    private var activeSeen: (id: String, since: Date)?
    private var lastTranscriptScan = Date.distantPast
    private var switchedHits = Set<String>()
    private var nudged = Set<String>()
    private var nudgedJobAt: [String: Date] = [:]
    private var working = Set<String>()

    init(accounts: AccountStore, announce: @escaping (String) -> Void) {
        self.accounts = accounts
        self.announce = announce
    }

    var mode: LimitMode {
        get { LimitMode(rawValue: UserDefaults.standard.string(forKey: "limitMode") ?? "") ?? .notifyAndSwitch }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "limitMode") }
    }

    var switchAt: Int {
        get {
            let v = UserDefaults.standard.object(forKey: "limitSwitchAt") as? Int ?? LimitLogic.defaultSwitchAt
            return min(max(v, LimitLogic.switchRange.lowerBound), LimitLogic.switchRange.upperBound)
        }
        set { UserDefaults.standard.set(newValue, forKey: "limitSwitchAt") }
    }

    /// Called from the polling loop.
    func check(_ agents: [Agent]) {
        guard mode != .off, let active = accounts.active else { return }
        let now = Date()
        if activeSeen?.id != active.id { activeSeen = (active.id, now) }
        let usage = accounts.usage[active.id]
        let fresh = usage.map { now.timeIntervalSince($0.fetched) <= 15 * 60 } ?? false

        var windows: [(name: String, window: UsageWindow?)] = [("5-hour", fresh ? usage?.session : nil),
                                                              ("weekly", fresh ? usage?.week : nil)]
        if fresh { windows += (usage?.other ?? []).map { ($0.name, $0.window) } }
        let skip90 = mode == .notifyAndSwitch && switchAt <= 90
        var fired: (level: Int, text: String)?
        for (name, window) in windows {
            guard let window else { continue }
            let key = "\(active.id)|\(name)"
            let r = LimitLogic.step(marks[key] ?? .init(), percent: window.percent, resetsAt: window.resetsAt, skip90: skip90)
            marks[key] = r.mark
            guard let level = r.fire, level >= (fired?.level ?? 0) else { continue }
            let pct = Int(window.percent.rounded())
            fired = (level, level >= 100
                ? "\(active.email) has used its \(name) limit (\(pct)%)\(Self.resets(window))."
                : "\(active.email) is at \(pct)% of its \(name) limit\(Self.resets(window)).")
        }

        // What pushes toward a switch: a window at the chosen point, or a background agent stopped on the limit.
        var reason: String?
        if let hit = windows.compactMap({ $0.window.map { ($0, $0.percent) } }).first(where: { $0.1 >= Double(switchAt) }) {
            reason = "\(Int(hit.1.rounded()))%"
        }
        let hits = limitHits(agents, now: now)
        if reason == nil, hits.contains(where: { isNew($0.hit, since: activeSeen?.since, now: now) }) {
            reason = "agent stopped on the limit"
        }

        if mode == .notifyAndSwitch, reason != nil {
            switch trySwitch(from: active, usage: usage, now: now) {
            case .started: return
            case .announced: break
            case .none: if let fired { announce(fired.text) }
            }
        } else if let fired {
            announce(fired.text)
        }

        if lastSwitch != nil { nudgeStopped(hits, now: now) }
    }

    // MARK: - Switching

    private enum Outcome { case started, announced, none }

    /// `started` and `announced` leave the announcing to this call; `none` leaves it to the caller.
    private func trySwitch(from active: ClaudeAccount, usage: AccountUsage?, now: Date) -> Outcome {
        guard !switching else { return .started }
        guard LimitLogic.maySwitch(lastSwitch: lastSwitch, now: now) else { return .none }
        let candidates = accounts.accounts.map { a -> LimitLogic.Candidate in
            let u = accounts.usage[a.id]
            // An unknown account-wide limit leaves no more room than the 5-hour one it sits beside.
            let short = ([u?.session?.percent] + (u?.other ?? []).map { $0.window.percent }).compactMap { $0 }.max()
            return .init(id: a.id, fetched: u?.fetched ?? .distantPast, session: short,
                         week: u?.week?.percent, needsLogin: a.needsLogin)
        }
        guard let target = LimitLogic.pickTarget(candidates, activeID: active.id, now: now, blockedUntil: blocked),
              let to = accounts.accounts.first(where: { $0.id == target.id }) else {
            // Say it once per half hour; the climb itself may already have been announced.
            guard lastNoRoom.map({ now.timeIntervalSince($0) > LimitLogic.switchGap }) ?? true else { return .announced }
            lastNoRoom = now
            let reset = ([usage?.session, usage?.week].compactMap { $0 } + (usage?.other ?? []).map(\.window))
                .filter { $0.percent >= Double(switchAt) }.compactMap(\.resetsAt).max()
            announce("\(active.email) is near its limit and no other account has room left"
                     + (reset.map { "; resets \(Self.clock($0))" } ?? "") + ".")
            return .announced
        }
        switching = true
        let until = LimitLogic.blockedUntil(session: usage?.session, week: usage?.week,
                                            other: (usage?.other ?? []).map(\.window), at: Double(switchAt), now: now)
        accounts.switchTo(to.id) { [weak self] error in
            guard let self else { return }
            self.switching = false
            if let error {
                self.announce("Couldn't switch accounts: \(error.localizedDescription)")
                return
            }
            self.lastSwitch = Date()
            self.lastNoRoom = nil
            self.blocked[active.id] = until
            let u = self.accounts.usage[to.id]
            let room = max(u?.session?.percent ?? 0, u?.week?.percent ?? 0)
            self.announce("Switched from \(active.email) to \(to.email) (\(Int(room.rounded()))% used) because of the usage limit.")
        }
        return .started
    }

    // MARK: - Agents stopped on the limit

    private func limitHits(_ agents: [Agent], now: Date) -> [(agent: Agent, jobID: String, hit: TranscriptReader.LimitHit)] {
        guard now.timeIntervalSince(lastTranscriptScan) >= Self.transcriptEvery else { return cachedHits }
        lastTranscriptScan = now
        var out: [(Agent, String, TranscriptReader.LimitHit)] = []
        for agent in agents where agent.group != .working {
            guard case .job(let jobID) = agent.target, let hit = TranscriptReader.limitHit(for: agent) else { continue }
            out.append((agent, jobID, hit))
        }
        cachedHits = out
        return out
    }
    private var cachedHits: [(agent: Agent, jobID: String, hit: TranscriptReader.LimitHit)] = []

    private func isNew(_ hit: TranscriptReader.LimitHit, since: Date?, now: Date) -> Bool {
        guard !switchedHits.contains(hit.id), let at = hit.at, now.timeIntervalSince(at) <= Self.freshHit else { return false }
        return since.map { at > $0 } ?? true
    }

    /// After a switch: restart each background job that stopped on the limit, then tell it to go on.
    /// One nudge per error entry, and per job at most once in half an hour.
    private func nudgeStopped(_ hits: [(agent: Agent, jobID: String, hit: TranscriptReader.LimitHit)], now: Date) {
        guard let switched = lastSwitch, now.timeIntervalSince(switched) < 2 * 3600 else { return }
        for (_, jobID, hit) in hits {
            guard !nudged.contains(hit.id), !working.contains(jobID),
                  hit.at.map({ now.timeIntervalSince($0) <= Self.nudgeWindow }) ?? false,
                  nudgedJobAt[jobID].map({ now.timeIntervalSince($0) > LimitLogic.switchGap }) ?? true else { continue }
            nudged.insert(hit.id)
            switchedHits.insert(hit.id)
            nudgedJobAt[jobID] = now
            working.insert(jobID)
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                Self.respawn(jobID)
                try? ReplySender.send(Self.nudge, toJob: jobID)
                DispatchQueue.main.async { self?.working.remove(jobID) }
            }
        }
    }

    /// `claude respawn <id>` restarts the session on the current login; then wait for its state file to
    /// move (it is running again) and let it settle before typing.
    private static func respawn(_ jobID: String) {
        let state = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".claude/jobs/\(jobID)/state.json").path
        func stamp() -> Date? { (try? FileManager.default.attributesOfItem(atPath: state))?[.modificationDate] as? Date }
        let before = stamp()
        let p = Process()
        p.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
        p.arguments = ["respawn", jobID]
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return }
        p.waitUntilExit()
        let deadline = Date().addingTimeInterval(60)
        while Date() < deadline {
            if let now = stamp(), now != before { break }
            Thread.sleep(forTimeInterval: 2)
        }
        Thread.sleep(forTimeInterval: 6)
    }

    // MARK: - Text

    private static func clock(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = Calendar.current.isDateInToday(date) ? "h:mm a" : "EEE h:mm a"
        return f.string(from: date)
    }

    private static func resets(_ window: UsageWindow) -> String {
        window.resetsAt.map { ", resets \(clock($0))" } ?? ""
    }
}

/// A notification: the system one when macOS allows it, otherwise Notchling's own banner.
enum UserNotifier {
    static func post(_ title: String, _ body: String) {
        guard Bundle.main.bundleIdentifier != nil else { return banner(title, body) }
        let center = UNUserNotificationCenter.current()
        center.getNotificationSettings { settings in
            switch settings.authorizationStatus {
            case .authorized, .provisional:
                add(center, title, body)
            case .notDetermined:
                // The answer can take as long as the user does; the message can't wait for it.
                banner(title, body)
                center.requestAuthorization(options: [.alert]) { _, _ in }
            default:
                banner(title, body)
            }
        }
    }

    private static func add(_ center: UNUserNotificationCenter, _ title: String, _ body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        center.add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)) { error in
            if error != nil { banner(title, body) }
        }
    }

    /// macOS can refuse an ad hoc signed app's notifications (UNErrorDomain 1). Never fall back on
    /// osascript: its notifications carry Script Editor's name and icon.
    private static func banner(_ title: String, _ body: String) {
        DispatchQueue.main.async { NoticeBanner.show(title, body) }
    }
}

/// Notchling's own notification: the app icon, a title and a message at the top right of the screen.
/// It works without any permission, stays over full-screen apps, and never takes focus.
enum NoticeBanner {
    private static let width: CGFloat = 360
    private static var panel: NSPanel?
    private static var closing: DispatchWorkItem?

    static func show(_ title: String, _ body: String) {
        dismiss()
        let view = NSHostingView(rootView: NoticeBannerView(title: title, message: body) { dismiss() })
        view.setFrameSize(NSSize(width: width, height: view.fittingSize.height))
        let panel = NSPanel(contentRect: view.frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.contentView = view
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .statusBar
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        if let screen = NSScreen.main?.visibleFrame {
            panel.setFrameOrigin(NSPoint(x: screen.maxX - view.frame.width - 16, y: screen.maxY - view.frame.height - 12))
        }
        panel.orderFrontRegardless()
        self.panel = panel
        let work = DispatchWorkItem { dismiss() }
        closing = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 12, execute: work)
    }

    static func dismiss() {
        closing?.cancel()
        closing = nil
        panel?.orderOut(nil)
        panel = nil
    }
}

struct NoticeBannerView: View {
    let title: String
    let message: String
    let onClose: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(nsImage: NSApp.applicationIconImage ?? NSImage())
                .resizable()
                .frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.headline)
                Text(message).font(.callout).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .frame(width: 360, alignment: .leading)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .contentShape(Rectangle())
        .onTapGesture(perform: onClose)
    }
}
