import AppKit
import Combine
import SwiftUI

// MARK: - Pure pieces (tested)

/// What the Machines header shows for one machine.
struct MachineChip: Equatable {
    enum Tone { case neutral, good, warn, bad }

    var name: String
    var word: String
    var tone: Tone
    /// "3 jobs tonight", "week 42%", the job in flight; only while the last poll worked.
    var details: [String]
    /// Why the last poll failed, when it did.
    var error: String?
    var tokenWarning: String?

    static func make(machine: Machine, report: LastReport?, previous: LastReport?, now: Date) -> MachineChip {
        guard let report else {
            return MachineChip(name: machine.name, word: "checking…", tone: .neutral, details: [], error: nil, tokenWarning: nil)
        }
        let word = MachineHealth.word(fileVault: machine.fileVault, last: report, previous: previous, now: now)
        let tone: Tone = word == MachineHealth.online ? .good : word == MachineHealth.offline ? .bad : .warn
        var details: [String] = []
        var warning: String?
        if report.succeeded, let status = report.status {
            details = MachineChip.details(for: status)
            warning = MachineHealth.tokenWarning(status)
        }
        return MachineChip(name: machine.name, word: word, tone: tone, details: details,
                           error: report.error, tokenWarning: warning)
    }
}

/// The words of the notch peek (or notification) for a new question.
enum MachineAlert {
    static func text(machine: String, entry: NeedsYouEntry) -> String {
        let question = entry.question ?? ""
        let what = !question.isEmpty ? question : entry.reason.isEmpty ? "needs you" : entry.reason
        return "\(machine) #\(entry.issue): \(what)"
    }

    /// One notification for the questions that were already waiting when polling began.
    static func waiting(count: Int) -> String {
        count == 1 ? "1 question is waiting" : "\(count) questions are waiting"
    }
}

// MARK: - Model

struct DiffSheet: Identifiable {
    let id: String
    let title: String
    var text: String?
}

/// What a Fix confirmation sheet shows. It never holds a password: the sheet keeps that in its own `@State`
/// and hands it to `MachinesModel.confirm` for the one run.
struct FixPrompt: Identifiable, Equatable {
    enum Kind: Equatable { case step(StepID), all, toggle(on: Bool) }

    let id = UUID()
    var machine: String
    var host: String
    var user: String
    var kind: Kind
    var title: String
    /// The exact commands, shown first.
    var display: String
    var needsAdmin: Bool
    /// The machine's admin access is root over SSH (no password).
    var root: Bool

    var warning: String? { needsAdmin && root ? "Admin steps run as root over SSH on \(host)." : nil }
}

enum SetupSheet: Identifiable {
    case fix(FixPrompt)
    case add
    case job(JobDraft)

    var id: String {
        switch self {
        case .fix(let p): return "fix-\(p.id.uuidString)"
        case .add: return "add"
        case .job(let d): return "job-\(d.machine)"
        }
    }
}

/// The last Fix's outcome on a machine. A secret step that opened the vault form says "Set up again": that is
/// guidance, not a failure.
struct FixLine: Equatable {
    var ok: Bool
    var text: String
    var guidance: Bool
}

/// What the Add sheet's Check found about a draft machine.
struct DraftCheck: Equatable {
    struct Line: Equatable {
        var title: String
        var state: CheckState
    }

    var reachable = false
    var os: MachineOS?
    var lines: [Line] = []
    var abilitiesLine = ""
    var problems: [String] = []
    var detected = Detected()
}

struct UpdateOfferRow: Identifiable, Equatable {
    var secret: String
    var count: Int
    var id: String { secret }
    var title: String { UpdateOffer.title(count: count) }
}

struct SecretRow: Identifiable, Equatable {
    var usage: SecretUsage
    var text: String
    var id: String { usage.name + "@" + usage.remotePath }
}

/// Feeds the window: the morning items (rebuilt on each poll, with the issues fetched once per poll, never
/// per redraw), thumbnails, the diff sheet, and which items an action already finished; and the machine setup
/// (cards, Add, jobs, the Fix sheet). It starts no Fix on its own: opening the window, Refresh, Add and a
/// vault change run checks only, and every Fix starts in a function a button calls (`confirm`, `update`,
/// `renameSecrets`).
@MainActor
final class MachinesModel: ObservableObject {
    enum Tab: String, CaseIterable { case machines = "Machines", jobs = "Jobs" }

    let poller: MachinePoller
    let actions: MorningActions
    let engine: SetupEngine

    @Published var tab: Tab = .machines
    @Published private(set) var registry: RegistryState = .missing
    @Published private(set) var machines: [Machine] = []
    @Published private(set) var vaultEntries: [VaultEntry] = []
    @Published private(set) var migrationNote: String?
    @Published private(set) var renameNote: String?
    @Published private(set) var registryProblem: String?
    @Published private(set) var outcomes: [String: FixLine] = [:]
    @Published private(set) var updateNotes: [String: String] = [:]
    @Published var setupSheet: SetupSheet? { didSet { sheetChanged(from: oldValue) } }
    @Published private(set) var items: [MorningItem] = []
    /// A line for an item whose last action worked, by item id. It leaves with the item.
    @Published private(set) var done: [String: String] = [:]
    @Published private(set) var shots: [String: [URL]] = [:]
    @Published var diff: DiffSheet?
    /// Set while the last fetch from GitHub failed for some repo (the titles and notes shown are the last good ones).
    @Published private(set) var githubNote: String?

    let registryPath: String
    private(set) var windowVisible = false
    /// Runs after the registry changed (the app starts polling once it holds a machine).
    var onRegistryChange: () -> Void = {}

    private let loadRegistry: () -> RegistryState
    private let saveRegistry: ([Machine]) -> Bool
    private let loadVault: () -> [VaultEntry]
    private let renameEntry: (VaultEntry, String) throws -> Void
    private let renewSecret: (String) -> Void
    private let runner: CommandRunning
    private var issues: [String: [GHIssue]] = [:]
    private var generation = 0
    private var shotsRequested = Set<String>()
    private var bag = Set<AnyCancellable>()
    /// The admin step `setUpAll` is waiting on: the sheet that asks, and where its answer goes.
    private var waiting: (id: UUID, answer: CheckedContinuation<AdminAnswer, Never>)?

    /// - Parameter migrated: the registry migrated from v1 before the window was made (the app loads it at
    ///   launch), so the note still names the backup.
    init(poller: MachinePoller, actions: MorningActions, engine: SetupEngine,
         registry: @escaping () -> RegistryState = { MachineRegistry.load() },
         save: @escaping ([Machine]) -> Bool = { MachineRegistry.save($0) },
         vaultEntries: @escaping () -> [VaultEntry] = { SecretVault.all() },
         rename: @escaping (VaultEntry, String) throws -> Void = { try VaultStore.shared.rename($0, to: $1) },
         renew: @escaping (String) -> Void = { VaultRenew.start(name: $0) },
         registryPath: String = MachineRegistry.fileURL.path, migrated: Bool = false, runner: CommandRunning) {
        self.poller = poller
        self.actions = actions
        self.engine = engine
        self.loadRegistry = registry
        self.saveRegistry = save
        self.loadVault = vaultEntries
        self.renameEntry = rename
        self.renewSecret = renew
        self.registryPath = registryPath
        self.runner = runner
        if migrated { migrationNote = Self.backupNote() }
        // One poll assigns the reports once per machine; wait for it to settle before asking GitHub, and
        // while the window is open check the machines again (checks only).
        poller.$reports
            .debounce(for: .milliseconds(400), scheduler: RunLoop.main)
            .sink { [weak self] _ in
                guard let self else { return }
                self.reload()
                if self.shouldRefreshOnPollerChange { Task { await self.engine.refresh() } }
            }
            .store(in: &bag)
        NotificationCenter.default.publisher(for: .vaultValueChanged)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.refreshVault() }
            .store(in: &bag)
        refreshMachines()
        refreshVault()
    }

    private static func backupNote() -> String {
        RegistryNotes.migrated(backup: MachineRegistry.backupURL(for: MachineRegistry.fileURL).path)
    }

    // MARK: Registry

    private func refreshMachines() {
        registry = loadRegistry()
        switch registry {
        case .loaded(let list, let migrated):
            machines = list
            if migrated { migrationNote = Self.backupNote() }
        case .missing, .unreadable:
            machines = []
        }
        rebuild()
    }

    private func refreshVault() { vaultEntries = loadVault() }

    var canAdd: Bool {
        if case .unreadable = registry { return false }
        return true
    }

    var unreadableReason: String? {
        if case .unreadable(let reason) = registry { return reason }
        return nil
    }

    var unreadableNote: String? { unreadableReason.map(RegistryNotes.unreadable(reason:)) }

    func dismissMigrationNote() { migrationNote = nil }

    /// Reads the registry again, rebuilds the items from the latest reports and fetches the issues.
    func reload() {
        refreshMachines()
        refreshVault()
        generation += 1
        let mine = generation
        let repos = machines.flatMap { $0.repos.map(\.github) }
        guard !repos.isEmpty else { return }
        let runner = runner
        Task { [weak self] in
            let result = await MorningItems.fetchIssuesChecked(repos: repos, runner: runner)
            guard let self, mine == self.generation else { return }
            self.issues = MorningItems.merged(old: self.issues, fetched: result.issues, failed: result.failed)
            self.githubNote = result.failed.isEmpty ? nil
                : "Couldn't refresh from GitHub (\(result.failed.sorted().joined(separator: ", "))). Titles and notes may be out of date."
            self.rebuild()
        }
    }

    private func rebuild() {
        // A machine whose last poll failed still shows the items of its last good status; acting on them
        // fails with a plain error while it is away.
        var reports = poller.reports
        for (name, report) in poller.reports where report.status == nil {
            if let old = poller.previous[name], old.status != nil { reports[name] = old }
        }
        items = MorningItems.build(reports: reports, issues: issues)
        let ids = Set(items.map(\.id))
        done = done.filter { ids.contains($0.key) }
        actions.pruneOutcomes(keeping: items)
    }

    func chip(for machine: Machine) -> MachineChip {
        MachineChip.make(machine: machine, report: poller.reports[machine.name], previous: poller.previous[machine.name], now: Date())
    }

    var tokenWarnings: [String] { machines.compactMap { chip(for: $0).tokenWarning } }

    /// The status the poller last got from a machine, or the one before a failed poll.
    func status(for machine: Machine) -> WorkerStatus? {
        poller.reports[machine.name]?.status ?? poller.previous[machine.name]?.status
    }

    // MARK: Cadence: checks only

    /// Opening the window: the registry, the issues, and every machine's checks, the slow ones too.
    func open() async {
        windowVisible = true
        reload()
        await engine.refresh(includeSlow: true)
    }

    func windowOpened() { windowVisible = true }
    func windowClosed() { windowVisible = false }

    /// While the window is visible each poller report change checks the machines again.
    var shouldRefreshOnPollerChange: Bool { windowVisible }

    /// The Refresh button.
    func refreshAll() async {
        reload()
        await engine.refresh(includeSlow: true)
    }

    /// A card's Check.
    func check(_ machine: String) async {
        await engine.refresh([machine], includeSlow: true)
    }

    // MARK: Changing the record (never touches a machine)

    private func commit(_ change: (inout [Machine]) -> Bool) -> String? {
        // the latest file, not the last render: the engine saves what the checks learn
        let current = loadRegistry()
        var list: [Machine]
        switch current {
        case .loaded(let machines, _): list = machines
        case .missing: list = []
        case .unreadable: return "machines.json can't be read, so nothing can be changed"
        }
        guard change(&list) else { return "no such machine" }
        guard saveRegistry(list) else {
            registryProblem = "Couldn't save machines.json"
            return registryProblem
        }
        registryProblem = nil
        reload()
        onRegistryChange()
        return nil
    }

    func setAdmin(_ machine: String, _ access: AdminAccess) {
        _ = commit { list in
            guard let i = list.firstIndex(where: { $0.name == machine }) else { return false }
            list[i].admin = access
            return true
        }
    }

    /// Removes the record only. Nothing on the machine changes.
    func remove(_ machine: String) {
        _ = commit { list in
            guard list.contains(where: { $0.name == machine }) else { return false }
            list.removeAll { $0.name == machine }
            return true
        }
        outcomes[machine] = nil
    }

    /// Adds or replaces the machine's sd worker job. Nil when saved, else why not.
    func saveJob(_ draft: JobDraft) -> String? {
        if let first = JobForm.problems(draft).first { return first }
        return commit { list in
            guard let i = list.firstIndex(where: { $0.name == draft.machine }) else { return false }
            list[i].sdWorker = JobForm.settings(draft, base: list[i].sdWorker)
            return true
        }
    }

    // MARK: Add machine

    /// The Add sheet's Check: Reachable, the OS and the abilities of a machine that is not in the registry yet.
    /// An unsafe form is not checked at all.
    func checkDraft(name: String, host: String, user: String) async -> DraftCheck {
        let problems = MachineForm.problems(name: name, host: host, user: user, existing: machines.map(\.name))
        guard problems.isEmpty else { return DraftCheck(problems: problems) }
        let results = await engine.check(draft: Machine(name: name, host: host, user: user))
        var check = DraftCheck()
        if let reachable = results[.reachable] {
            check.lines = [.init(title: "Reachable", state: reachable.state)]
            if case .pass = reachable.state { check.reachable = true }
            check.detected = reachable.detected
        }
        if let filevault = results[.filevault]?.detected.fileVault { check.detected.fileVault = filevault }
        check.os = check.detected.os
        check.abilitiesLine = check.reachable ? AbilitiesLine.text(check.detected.abilities, os: check.detected.os) : ""
        return check
    }

    /// Saves the machine (the record only). Nil when added, else why not. It runs no check and no Fix; the sheet
    /// checks the new machine afterwards.
    func add(name: String, host: String, user: String, check: DraftCheck) -> String? {
        guard canAdd else { return "machines.json can't be read, so nothing can be added" }
        if let first = MachineForm.problems(name: name, host: host, user: user, existing: machines.map(\.name)).first { return first }
        guard check.reachable else { return "Check Reachable first" }
        var machine = Machine(name: name, host: host, user: user)
        machine.os = check.detected.os
        if let fileVault = check.detected.fileVault { machine.fileVault = fileVault }
        if var abilities = check.detected.abilities {
            if let home = check.detected.home, RemoteShell.isSafeMachinePath(home) { abilities.home = home }
            machine.abilities = abilities
        }
        return commit { list in
            guard !list.contains(where: { $0.name == name }) else { return false }
            list.append(machine)
            return true
        }
    }

    // MARK: Fixes: every one starts here, from a button

    private func machine(named name: String) -> Machine? { machines.first { $0.name == name }.map(engine.overlaid) }

    private func prompt(_ m: Machine, kind: FixPrompt.Kind, title: String, display: String, needsAdmin: Bool) -> FixPrompt {
        FixPrompt(machine: m.name, host: m.host, user: m.user, kind: kind, title: title, display: display,
                  needsAdmin: needsAdmin, root: m.admin == .rootSSH)
    }

    /// Set up on one step: shows its commands. Runs nothing.
    func requestSetUp(_ id: StepID, on machine: String) {
        guard let m = self.machine(named: machine),
              let step = engine.steps(for: m).first(where: { $0.id == id }), case .run(let plan) = step.fix else { return }
        setupSheet = .fix(prompt(m, kind: .step(id), title: step.title, display: plan.display, needsAdmin: plan.needsAdmin))
    }

    /// Set up all: shows the commands of every ✗ step. The admin password is asked for per admin step, when
    /// its turn comes.
    func requestSetUpAll(on machine: String) {
        guard let m = self.machine(named: machine) else { return }
        let todo = engine.steps(for: m).compactMap { step -> (SetupStep, FixPlan)? in
            guard case .run(let plan) = step.fix, case .fail? = engine.results[machine]?[step.id]?.state else { return nil }
            return (step, plan)
        }
        guard !todo.isEmpty else {
            outcomes[machine] = FixLine(ok: true, text: "nothing to set up", guidance: false)
            return
        }
        let display = todo.map { "▸ \($0.0.title)\n\($0.1.display)" }.joined(separator: "\n\n")
        setupSheet = .fix(prompt(m, kind: .all, title: "Set up all on \(machine)", display: display, needsAdmin: false))
    }

    /// The Running switch: shows its script. Runs nothing.
    func requestToggle(on machine: String, to on: Bool) {
        guard let m = self.machine(named: machine),
              let step = engine.steps(for: m).first(where: { $0.id == .running }), case .toggle(let needsAdmin) = step.fix else { return }
        setupSheet = .fix(prompt(m, kind: .toggle(on: on), title: on ? "Turn Running on" : "Turn Running off",
                                 display: engine.toggleDisplay(on: machine, to: on) ?? "", needsAdmin: needsAdmin))
    }

    /// The sheet's Run. `password` is for this one run: it goes into the approval and nowhere else, and nothing
    /// here keeps it. The returned task ends when the Fix has.
    @discardableResult
    func confirm(_ prompt: FixPrompt, password: String?) -> Task<Void, Never> {
        if let waiting, waiting.id == prompt.id {
            // Set up all asked for this admin step
            self.waiting = nil
            setupSheet = nil
            waiting.answer.resume(returning: prompt.root ? .rootConfirmed : .password(password ?? ""))
            return Task {}
        }
        if case .fix(let shown)? = setupSheet, shown.id == prompt.id { setupSheet = nil }
        let machine = prompt.machine
        switch prompt.kind {
        case .step(let id):
            let approval = FixApproval(adminPassword: prompt.root ? nil : password, asRootConfirmed: prompt.root)
            return Task { [weak self] in
                guard let self else { return }
                self.record(await self.engine.runFix(id, on: machine, approval: approval), for: machine)
            }
        case .toggle(let on):
            let approval = FixApproval(adminPassword: prompt.root ? nil : password, asRootConfirmed: prompt.root)
            return Task { [weak self] in
                guard let self else { return }
                self.record(await self.engine.toggleRunning(on: machine, to: on, approval: approval), for: machine)
            }
        case .all:
            return Task { [weak self] in
                guard let self else { return }
                let outcome = await self.engine.setUpAll(on: machine) { [weak self] step in
                    guard let self else { return .cancelled }
                    return await self.askAdmin(step, on: machine)
                }
                self.record(outcome, for: machine)
            }
        }
    }

    /// The sheet's Cancel (or closing it): nothing runs. A Set up all waiting on it stops.
    func cancel(_ prompt: FixPrompt) {
        if case .fix(let shown)? = setupSheet, shown.id == prompt.id { setupSheet = nil }   // `didSet` answers a waiting run
        if let waiting, waiting.id == prompt.id {
            self.waiting = nil
            waiting.answer.resume(returning: .cancelled)
        }
    }

    private func record(_ outcome: FixOutcome, for machine: String) {
        outcomes[machine] = FixLine(ok: outcome.ok, text: outcome.message,
                                    guidance: !outcome.ok && outcome.message.contains("Set up again"))
    }

    /// Set up all reached an admin step: the sheet asks, and the answer goes back to the run.
    private func askAdmin(_ step: SetupStep, on machine: String) async -> AdminAnswer {
        guard let m = self.machine(named: machine), case .run(let plan) = step.fix else { return .cancelled }
        let asking = prompt(m, kind: .step(step.id), title: step.title, display: plan.display, needsAdmin: true)
        return await withCheckedContinuation { continuation in
            waiting = (asking.id, continuation)
            setupSheet = .fix(asking)
        }
    }

    /// A sheet closed some other way (Esc, a click outside) while Set up all waited on it.
    private func sheetChanged(from old: SetupSheet?) {
        guard case .fix(let was)? = old, let waiting, waiting.id == was.id else { return }
        if case .fix(let now)? = setupSheet, now.id == was.id { return }
        self.waiting = nil
        waiting.answer.resume(returning: .cancelled)
    }

    // MARK: Renewed secrets, renames

    var updateOffers: [UpdateOfferRow] {
        engine.updateOffers.keys.sorted().map { UpdateOfferRow(secret: $0, count: engine.updateOffers[$0]?.count ?? 0) }
    }

    /// "Update N machines": sends the renewed value to each usage, on this click.
    @discardableResult
    func update(secret: String) -> Task<Void, Never> {
        Task { [weak self] in
            guard let self else { return }
            let outcomes = await self.engine.updateMachines(for: secret, approval: FixApproval(adminPassword: nil, asRootConfirmed: false))
            self.updateNotes[secret] = outcomes.first(where: { !$0.ok })?.message
        }
    }

    var renameBanner: String? { RegistryNotes.rename(SecretNames.pendingRenames(in: vaultEntries)) }

    /// The Rename button: moves each legacy secret to its new name, keeping its value.
    func renameSecrets() {
        var failed = false
        for (entry, name) in SecretNames.pendingRenames(in: vaultEntries) {
            do { try renameEntry(entry, name) } catch { failed = true }
        }
        refreshVault()
        renameNote = failed ? "Rename didn't finish; try again" : nil
    }

    /// Renew…: GitHub's token page or the secret's own, then the vault form.
    func renew(_ secret: String) { renewSecret(secret) }

    /// The secret files of a machine with what the checks found about each.
    func secretRows(for machine: Machine, now: Date = Date()) -> [SecretRow] {
        SecretSteps.usages(in: [machine]).filter { $0.machine == machine.name }.map { usage in
            let stepID: StepID = usage.name == machine.claudeSecret ? .claudeLogin : .issuesToken
            let entry = vaultEntries.first { $0.id == usage.name.lowercased() }
            return SecretRow(usage: usage, text: SecretRowFacts.text(usage: usage, entry: entry,
                                                                    facts: engine.results[machine.name]?[stepID], now: now))
        }
    }

    // MARK: Actions on the morning items (called from buttons only)

    /// Starts `action`, and when it ends polls again and notes the success on the item. An action that leaves its
    /// item where it was (an undone merge) passes no `success`: the item stays open for the next action, and
    /// the action itself leaves its note.
    func perform(_ item: MorningItem, success: String?, _ action: (MorningActions) -> Task<Void, Never>) {
        guard !actions.running.contains(item.id) else { return }
        let task = action(actions)
        Task { [weak self] in
            await task.value
            guard let self else { return }
            if let success, self.actions.lastError[item.id] == nil { self.done[item.id] = success }
            await self.poller.refresh()
            self.reload()
        }
    }

    func showDiff(_ item: MorningItem) {
        diff = DiffSheet(id: item.id, title: "\(item.repo) #\(item.issue)", text: nil)
        Task { [weak self] in
            guard let self else { return }
            let text = await self.actions.diff(item)
            if self.diff?.id == item.id { self.diff?.text = text }
        }
    }

    /// Copies a ready branch's screenshots once per commit (a read from the machine, not an action).
    func loadShots(_ item: MorningItem) async {
        guard case .ready(let entry) = item.entry, !entry.evidenceDir.isEmpty else { return }
        guard shotsRequested.insert("\(item.id)@\(entry.commit)").inserted else { return }
        let files = await actions.screenshots(item)
        if !files.isEmpty { shots[item.id] = files }
    }
}

// MARK: - Window

@MainActor
final class MachinesWindowController: NSObject, NSWindowDelegate {
    private let model: MachinesModel
    private let reload: () -> Void
    private var window: NSWindow?

    /// `reload` runs on show and on "Check again": the app uses it to start polling once machines.json has machines.
    init(poller: MachinePoller, actions: MorningActions, engine: SetupEngine, migrated: Bool = false,
         runner: CommandRunning = ProcessRunner(), reload: @escaping () -> Void = {}) {
        model = MachinesModel(poller: poller, actions: actions, engine: engine, migrated: migrated, runner: runner)
        self.reload = reload
        model.onRegistryChange = reload
    }

    func show() {
        reload()
        // opening the window checks the machines (checks only, the slow ones too)
        Task { [model] in await model.open() }
        if window == nil {
            let view = MachinesView(model: model, onCheckAgain: { [weak self] in
                self?.reload()
                guard let model = self?.model else { return }
                Task { await model.refreshAll() }
            })
            let window = EditingWindow(contentViewController: NSHostingController(rootView: view))
            window.title = "Machines"
            window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.setContentSize(NSSize(width: 900, height: 780))
            window.minSize = NSSize(width: 680, height: 460)
            window.center()
            window.setFrameAutosaveName("Notchling Machines")
            self.window = window
        }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    func windowDidBecomeKey(_ notification: Notification) { model.reload() }

    func windowWillClose(_ notification: Notification) {
        model.windowClosed()
        DispatchQueue.main.async { self.window = nil }
    }
}

/// What the app holds: one poller, one set of actions, one setup engine, one window. Polling starts only when
/// `machines.json` holds a machine.
@MainActor
final class MachinesCenter {
    let poller: MachinePoller
    let actions: MorningActions
    let engine: SetupEngine
    private(set) lazy var window = MachinesWindowController(poller: poller, actions: actions, engine: engine,
                                                            migrated: migrated, reload: { [weak self] in
        self?.startIfNeeded()
    })
    private var started = false
    /// The load at launch turned a v1 file into v2; the window's note names the backup.
    private let migrated: Bool

    init(runner: CommandRunning = ProcessRunner()) {
        // the first load migrates a v1 file; keep that fact for the window
        if case .loaded(_, let migrated) = MachineRegistry.load() { self.migrated = migrated } else { self.migrated = false }
        poller = MachinePoller(runner: runner, machines: { MachineRegistry.all() }, clock: { Date() })
        actions = MorningActions(runner: runner)
        engine = SetupEngine(runner: runner, secrets: VaultSecretSource(), http: URLSessionFetcher(),
                             mac: { MacFacts.load(home: FileManager.default.homeDirectoryForCurrentUser) },
                             machines: { MachineRegistry.all() }, save: { MachineRegistry.save($0) }, clock: Date.init)
    }

    func startIfNeeded() {
        guard !started, !MachineRegistry.all().isEmpty else { return }
        started = true
        poller.start()
    }

    func show() { window.show() }
}

// MARK: - Views

enum Danger {
    static let color = Color(light: Color(hex: 0xB3261E), dark: Color(hex: 0xFF8A80))
}

struct MachinesView: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject private var poller: MachinePoller
    @ObservedObject private var actions: MorningActions
    @ObservedObject private var engine: SetupEngine
    var onCheckAgain: () -> Void = {}

    init(model: MachinesModel, onCheckAgain: @escaping () -> Void = {}) {
        self.model = model
        self.poller = model.poller
        self.actions = model.actions
        self.engine = model.engine
        self.onCheckAgain = onCheckAgain
    }

    var body: some View {
        ZStack {
            KitInk.window.ignoresSafeArea()
            VStack(spacing: 0) {
                topBar
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        switch model.tab {
                        case .machines: MachinesTab(model: model, engine: engine, onCheckAgain: onCheckAgain)
                        case .jobs: jobs
                        }
                    }
                    .padding(24)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .frame(minWidth: 680, minHeight: 460)
        .sheet(item: $model.diff) { MachineDiffView(sheet: $0) { model.diff = nil } }
        .sheet(item: $model.setupSheet) { SetupSheetView(model: model, sheet: $0) }
    }

    private var topBar: some View {
        HStack(spacing: 12) {
            Picker("", selection: $model.tab) {
                ForEach(MachinesModel.Tab.allCases, id: \.self) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented).labelsHidden().frame(width: 200)
            Spacer()
            Button("Refresh") { Task { await model.refreshAll() } }.buttonStyle(.kit(.secondary))
            if model.tab == .machines {
                Button("Add machine…") { model.setupSheet = .add }.buttonStyle(.kit(.primary)).disabled(!model.canAdd)
            }
        }
        .padding(.horizontal, 24).padding(.vertical, 12)
    }

    @ViewBuilder private var jobs: some View {
        VStack(alignment: .leading, spacing: 12) {
            if model.machines.isEmpty {
                Text("Add a machine first.").kitType(.body).foregroundColor(KitInk.textFaint)
            }
            ForEach(model.machines, id: \.name) { machine in
                JobCardView(model: model, engine: engine, poller: poller, machine: engine.overlaid(machine))
            }
            if let note = model.githubNote {
                Label(note, systemImage: "exclamationmark.triangle").kitType(.label).foregroundColor(KitInk.textMuted)
            }
            ForEach(model.tokenWarnings, id: \.self) { warning in
                Label(warning, systemImage: "exclamationmark.triangle.fill")
                    .kitType(.label).foregroundColor(KitInk.accentText)
            }
        }
        section("Ready to merge", model.items.filter { $0.kind == .ready }) { item in
            if case .ready(let entry) = item.entry { ReadyCard(model: model, actions: actions, item: item, entry: entry) }
        }
        section("Needs you", model.items.filter { $0.kind == .needsYou }) { item in
            if case .needsYou(let entry) = item.entry { NeedsYouCard(model: model, actions: actions, item: item, entry: entry) }
        }
        section("Stuck", model.items.filter { $0.kind == .stuck }) { item in
            if case .stuck(let entry) = item.entry { StuckCard(model: model, actions: actions, item: item, entry: entry) }
        }
    }

    private func section<Row: View>(_ title: String, _ items: [MorningItem], @ViewBuilder row: @escaping (MorningItem) -> Row) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text(title).kitType(.section).foregroundColor(KitInk.textMuted)
                if !items.isEmpty {
                    Text("\(items.count)").kitType(.section).foregroundColor(KitInk.onAccent)
                        .padding(.horizontal, 6).background(Capsule().fill(KitInk.accent))
                }
            }
            if items.isEmpty {
                Text("Nothing here.").kitType(.body).foregroundColor(KitInk.textFaint)
            } else {
                ForEach(items) { row($0) }
            }
        }
    }
}

// MARK: Cards

struct Card<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) { content }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .kitCard()
    }
}

private struct ItemTitle: View {
    let item: MorningItem
    var branch: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("\(item.repo) #\(item.issue)").kitType(.meta).foregroundColor(KitInk.textMuted)
            Text(item.title).kitType(.heading).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
            Text([item.machine, branch].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
                .kitType(.meta).foregroundColor(KitInk.textFaint).textSelection(.enabled)
        }
    }
}

/// The spinner while an action runs, its error, and the line for one that worked.
private struct ActionStatus: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var actions: MorningActions
    let item: MorningItem

    var body: some View {
        if actions.running.contains(item.id) {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Working…").kitType(.label).foregroundColor(KitInk.textMuted)
            }
        }
        if let error = actions.lastError[item.id] {
            Text(error).kitType(.meta).foregroundColor(Danger.color)
                .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
        }
        if let note = model.done[item.id] ?? actions.notes[item.id], actions.lastError[item.id] == nil {
            Label(note, systemImage: "checkmark.circle.fill").kitType(.label).foregroundColor(KitInk.done)
        }
    }
}

private struct NoteField: View {
    let placeholder: String
    @Binding var text: String
    var onSubmit: () -> Void

    var body: some View {
        // `.onSubmit`, not `onCommit`: on macOS `onCommit` also fires when the field only loses focus, which
        // would post a half-typed answer to GitHub.
        TextField(placeholder, text: $text)
            .onSubmit(onSubmit)
            .textFieldStyle(.plain)
            .kitType(.body)
            .padding(.horizontal, 10)
            .frame(height: KitMetric.control)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.raised))
    }
}

private struct ReadyCard: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var actions: MorningActions
    let item: MorningItem
    let entry: ReadyEntry
    @State private var sendingBack = false
    @State private var note = ""
    @State private var confirmClose = false

    private var busy: Bool { actions.running.contains(item.id) || model.done[item.id] != nil }
    private var outcome: MergeOutcome? { actions.outcome(for: item) }

    var body: some View {
        Card {
            ItemTitle(item: item, branch: entry.branch)
            if let comment = item.comment { MarkdownView(text: comment) }
            if let files = model.shots[item.id] {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 10) { ForEach(files, id: \.self) { Thumbnail(url: $0) } }
                }
            }
            HStack(spacing: 8) {
                Button("Merge") { model.perform(item, success: "Merged, pushed and closed") { $0.merge(item) } }
                    .buttonStyle(.kit(.primary))
                Button("Diff") { model.showDiff(item) }.buttonStyle(.kit(.secondary))
                Button("Send back") { sendingBack.toggle() }.buttonStyle(.kit(.secondary))
                Button("Close") { confirmClose = true }.buttonStyle(.kit(.ghost))
                // Retry push only for a merge whose check passed. A merge that sits on main without that proof (a failed
                // check that could not be undone, an edit made during the check, a quit during the check) can only be undone.
                if let outcome, outcome.canRetryPush {
                    Button("Retry push") { model.perform(item, success: "Pushed and closed") { $0.retryPush(item) } }
                        .buttonStyle(.kit(.primary))
                }
                if let outcome, outcome.canUndo {
                    Button("Undo merge") { model.perform(item, success: nil) { $0.undoMerge(item) } }
                        .buttonStyle(.kit(.secondary))
                }
            }
            .disabled(busy)
            if sendingBack {
                HStack(spacing: 8) {
                    NoteField(placeholder: "What should the worker change?", text: $note, onSubmit: sendBack)
                    Button("Send", action: sendBack).buttonStyle(.kit(.primary)).disabled(trimmed.isEmpty || busy)
                }
            }
            if let outcome { Checklist(outcome: outcome) }
            ActionStatus(model: model, actions: actions, item: item)
        }
        .task(id: "\(item.id)@\(entry.commit)") { await model.loadShots(item) }
        .confirmationDialog("Close \(item.repo) #\(item.issue) as not planned?", isPresented: $confirmClose) {
            Button("Close the issue", role: .destructive) { model.perform(item, success: "Closed") { $0.close(item) } }
        }
    }

    private var trimmed: String { note.trimmingCharacters(in: .whitespacesAndNewlines) }

    private func sendBack() {
        guard !trimmed.isEmpty, !busy else { return }
        let text = trimmed
        model.perform(item, success: "Sent back to the worker") { $0.sendBack(item, note: text) }
        note = ""
        sendingBack = false
    }
}

private struct Checklist: View {
    let outcome: MergeOutcome

    var body: some View {
        HStack(spacing: 14) {
            step("fetched", outcome.fetched)
            step("merged", outcome.merged)
            step("checked", outcome.checked)
            step("pushed", outcome.pushed)
            step("closed", outcome.closed)
        }
    }

    private func step(_ name: String, _ ok: Bool) -> some View {
        HStack(spacing: 4) {
            Image(systemName: ok ? "checkmark.circle.fill" : "circle").foregroundColor(ok ? KitInk.done : KitInk.textFaint)
            Text(name).kitType(.meta).foregroundColor(ok ? KitInk.text : KitInk.textMuted)
        }
    }
}

private struct Thumbnail: View {
    let url: URL
    @State private var image: NSImage?

    var body: some View {
        Group {
            if let image {
                Image(nsImage: image).resizable().scaledToFit()
            } else {
                Color.clear
            }
        }
        .frame(height: 96)
        .frame(minWidth: 40)
        .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).strokeBorder(KitInk.hairline))
        .onTapGesture { NSWorkspace.shared.open(url) }
        .pointerCursor()
        .help(url.lastPathComponent)
        .task { image = NSImage(contentsOf: url) }
    }
}

private struct NeedsYouCard: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var actions: MorningActions
    let item: MorningItem
    let entry: NeedsYouEntry
    @State private var answer = ""
    @State private var confirmClose = false

    private var busy: Bool { actions.running.contains(item.id) || model.done[item.id] != nil }
    private var trimmed: String { answer.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var recommend: String? { entry.recommend.flatMap { $0.isEmpty ? nil : $0 } }

    var body: some View {
        Card {
            ItemTitle(item: item, branch: entry.branch)
            if let question = entry.question, !question.isEmpty {
                Text(question).kitType(.body).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
                if !entry.reason.isEmpty { Text(entry.reason).kitType(.meta).foregroundColor(KitInk.textMuted) }
            } else if !entry.reason.isEmpty {
                Text(entry.reason).kitType(.body).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
            }
            if let recommend {
                Label("Recommended: \(recommend)", systemImage: "hand.thumbsup.fill").kitType(.label).foregroundColor(KitInk.accentText)
            }
            if !entry.options.isEmpty {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 70), spacing: 8, alignment: .leading)], alignment: .leading, spacing: 8) {
                    ForEach(entry.options, id: \.self) { option in
                        Button(option) { send(option) }.buttonStyle(.kit(.secondary)).disabled(busy)
                    }
                }
            }
            HStack(spacing: 8) {
                NoteField(placeholder: "Your answer", text: $answer, onSubmit: { send(trimmed) })
                Button("Answer") { send(trimmed) }.buttonStyle(.kit(.primary)).disabled(trimmed.isEmpty || busy)
            }
            HStack(spacing: 8) {
                if recommend != nil {
                    Button("Use recommended") { model.perform(item, success: "Answered with the recommendation") { $0.useRecommended(item) } }
                        .buttonStyle(.kit(.secondary)).disabled(busy)
                }
                Button("Close") { confirmClose = true }.buttonStyle(.kit(.ghost)).disabled(busy)
            }
            ActionStatus(model: model, actions: actions, item: item)
        }
        .confirmationDialog("Close \(item.repo) #\(item.issue) as not planned?", isPresented: $confirmClose) {
            Button("Close the issue", role: .destructive) { model.perform(item, success: "Closed") { $0.close(item) } }
        }
    }

    private func send(_ text: String) {
        guard !text.isEmpty, !busy else { return }
        model.perform(item, success: "Answered. The worker resumes it on its next tick") { $0.answer(item, text: text) }
        answer = ""
    }
}

private struct StuckCard: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var actions: MorningActions
    let item: MorningItem
    let entry: StuckEntry
    @State private var confirmDrop = false

    private var busy: Bool { actions.running.contains(item.id) || model.done[item.id] != nil }

    var body: some View {
        Card {
            ItemTitle(item: item, branch: entry.branch)
            if !entry.reason.isEmpty {
                Text(entry.reason).kitType(.body).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
            }
            if !entry.reportTail.isEmpty {
                DisclosureGroup("Report tail") {
                    ScrollView {
                        Text(entry.reportTail).kitType(.mono).foregroundColor(KitInk.text)
                            .frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
                    }
                    .frame(maxHeight: 180)
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.sidebar))
                }
                .kitType(.label).foregroundColor(KitInk.textMuted)
            }
            HStack(spacing: 8) {
                Button("Retry") { model.perform(item, success: "Queued again") { $0.retry(item) } }.buttonStyle(.kit(.primary))
                Button("Drop") { confirmDrop = true }.buttonStyle(.kit(.ghost))
            }
            .disabled(busy)
            ActionStatus(model: model, actions: actions, item: item)
        }
        .confirmationDialog("Drop \(item.repo) #\(item.issue)? This closes the issue as not planned.", isPresented: $confirmDrop) {
            Button("Drop it", role: .destructive) { model.perform(item, success: "Dropped") { $0.drop(item) } }
        }
    }
}

private struct MachineDiffView: View {
    let sheet: DiffSheet
    let close: () -> Void
    private static let limit = 200_000

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Diff · \(sheet.title)").kitType(.heading).foregroundColor(KitInk.text)
                Spacer()
                Button("Done", action: close).buttonStyle(.kit(.secondary)).keyboardShortcut(.cancelAction)
            }
            .padding(14)
            Divider()
            if let text = sheet.text {
                ScrollView([.vertical, .horizontal]) {
                    Text(colored(text)).textSelection(.enabled)
                        .padding(14).frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                VStack { Spacer(); ProgressView("Fetching the branch…"); Spacer() }.frame(maxWidth: .infinity)
            }
        }
        .frame(minWidth: 760, minHeight: 520)
        .background(KitInk.window)
    }

    private func colored(_ text: String) -> AttributedString {
        let shown = text.count > Self.limit ? String(text.prefix(Self.limit)) + "\n… cut at \(Self.limit) characters" : text
        var out = AttributedString()
        for line in shown.split(separator: "\n", omittingEmptySubsequences: false) {
            var piece = AttributedString(String(line) + "\n")
            piece.font = .system(size: 12, design: .monospaced)
            if line.hasPrefix("+++") || line.hasPrefix("---") || line.hasPrefix("diff ") {
                piece.foregroundColor = KitInk.textMuted
            } else if line.hasPrefix("+") {
                piece.foregroundColor = KitInk.done
            } else if line.hasPrefix("-") {
                piece.foregroundColor = Danger.color
            } else if line.hasPrefix("@@") {
                piece.foregroundColor = KitInk.accentText
            } else {
                piece.foregroundColor = KitInk.text
            }
            out += piece
        }
        return out
    }
}
