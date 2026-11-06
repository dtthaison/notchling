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
            details.append("\(status.jobsTonight) \(status.jobsTonight == 1 ? "job" : "jobs") tonight")
            if let pct = status.weeklyPct { details.append("week \(Int(pct.rounded()))%") }
            if let job = status.current {
                details.append("now \(job.repo) #\(job.issue)" + (job.stage.isEmpty ? "" : " · \(job.stage)"))
            }
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
}

// MARK: - Model

struct DiffSheet: Identifiable {
    let id: String
    let title: String
    var text: String?
}

/// Feeds the window: the morning items (rebuilt on each poll, with the issues fetched once per poll, never
/// per redraw), thumbnails, the diff sheet, and which items an action already finished. It starts no action on
/// its own: only the buttons do.
@MainActor
final class MachinesModel: ObservableObject {
    let poller: MachinePoller
    let actions: MorningActions

    @Published private(set) var machines: [Machine] = []
    @Published private(set) var items: [MorningItem] = []
    /// A line for an item whose last action worked, by item id. It leaves with the item.
    @Published private(set) var done: [String: String] = [:]
    @Published private(set) var shots: [String: [URL]] = [:]
    @Published var diff: DiffSheet?

    private let machineList: () -> [Machine]
    private let runner: CommandRunning
    private var issues: [String: [GHIssue]] = [:]
    /// Set while the last fetch from GitHub failed for some repo (the titles and notes shown are the last good ones).
    @Published private(set) var githubNote: String?
    private var generation = 0
    private var shotsRequested = Set<String>()
    private var bag = Set<AnyCancellable>()

    init(poller: MachinePoller, actions: MorningActions, machines: @escaping () -> [Machine], runner: CommandRunning) {
        self.poller = poller
        self.actions = actions
        self.machineList = machines
        self.runner = runner
        // One poll assigns the reports once per machine; wait for it to settle before asking GitHub.
        poller.$reports
            .debounce(for: .milliseconds(400), scheduler: RunLoop.main)
            .sink { [weak self] _ in self?.reload() }
            .store(in: &bag)
        refreshMachines()
    }

    private func refreshMachines() { machines = machineList(); rebuild() }

    /// Reads the registry again, rebuilds the items from the latest reports and fetches the issues.
    func reload() {
        refreshMachines()
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

    // MARK: Actions (called from buttons only)

    /// Starts `action`, and when it ends polls again and notes the success on the item.
    func perform(_ item: MorningItem, success: String, _ action: (MorningActions) -> Task<Void, Never>) {
        guard !actions.running.contains(item.id) else { return }
        let task = action(actions)
        Task { [weak self] in
            await task.value
            guard let self else { return }
            if self.actions.lastError[item.id] == nil { self.done[item.id] = success }
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
    init(poller: MachinePoller, actions: MorningActions,
         machines: @escaping () -> [Machine] = { MachineRegistry.all() },
         runner: CommandRunning = ProcessRunner(), reload: @escaping () -> Void = {}) {
        model = MachinesModel(poller: poller, actions: actions, machines: machines, runner: runner)
        self.reload = reload
    }

    func show() {
        reload()
        model.reload()
        if window == nil {
            let view = MachinesView(model: model, onCheckAgain: { [weak self] in self?.reload(); self?.model.reload() })
            let window = EditingWindow(contentViewController: NSHostingController(rootView: view))
            window.title = "Machines"
            window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.setContentSize(NSSize(width: 860, height: 760))
            window.minSize = NSSize(width: 640, height: 420)
            window.center()
            window.setFrameAutosaveName("Notchling Machines")
            self.window = window
        }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    func windowDidBecomeKey(_ notification: Notification) { model.reload() }

    func windowWillClose(_ notification: Notification) {
        DispatchQueue.main.async { self.window = nil }
    }
}

/// What the app holds: one poller, one set of actions, one window. Polling starts only when `machines.json`
/// holds a machine.
@MainActor
final class MachinesCenter {
    let poller: MachinePoller
    let actions: MorningActions
    private(set) lazy var window = MachinesWindowController(poller: poller, actions: actions, reload: { [weak self] in
        self?.startIfNeeded()
    })
    private var started = false

    init(runner: CommandRunning = ProcessRunner()) {
        poller = MachinePoller(runner: runner, machines: { MachineRegistry.all() }, clock: { Date() })
        actions = MorningActions(runner: runner)
    }

    func startIfNeeded() {
        guard !started, !MachineRegistry.all().isEmpty else { return }
        started = true
        poller.start()
    }

    func show() { window.show() }
}

// MARK: - Views

private enum Danger {
    static let color = Color(light: Color(hex: 0xB3261E), dark: Color(hex: 0xFF8A80))
}

struct MachinesView: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject private var poller: MachinePoller
    @ObservedObject private var actions: MorningActions
    var onCheckAgain: () -> Void = {}

    init(model: MachinesModel, onCheckAgain: @escaping () -> Void = {}) {
        self.model = model
        self.poller = model.poller
        self.actions = model.actions
        self.onCheckAgain = onCheckAgain
    }

    var body: some View {
        ZStack {
            KitInk.window.ignoresSafeArea()
            if model.machines.isEmpty {
                MachinesEmptyState(onCheckAgain: onCheckAgain)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        header
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
                    .padding(24)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .frame(minWidth: 640, minHeight: 420)
        .sheet(item: $model.diff) { MachineDiffView(sheet: $0) { model.diff = nil } }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 250), spacing: 12, alignment: .top)], alignment: .leading, spacing: 12) {
                ForEach(model.machines, id: \.name) { ChipView(chip: model.chip(for: $0)) }
            }
            if let note = model.githubNote {
                Label(note, systemImage: "exclamationmark.triangle").kitType(.label).foregroundColor(KitInk.textMuted)
            }
            ForEach(model.tokenWarnings, id: \.self) { warning in
                Label(warning, systemImage: "exclamationmark.triangle.fill")
                    .kitType(.label).foregroundColor(KitInk.accentText)
            }
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

private struct ChipView: View {
    let chip: MachineChip

    private var tint: Color {
        switch chip.tone {
        case .good: return KitInk.done
        case .warn: return KitInk.needs
        case .bad: return Danger.color
        case .neutral: return KitInk.textFaint
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Circle().fill(tint).frame(width: 8, height: 8)
                Text(chip.name).kitType(.heading).foregroundColor(KitInk.text)
                Text(chip.word).kitType(.label).foregroundColor(chip.tone == .bad ? Danger.color : KitInk.textMuted)
                    .lineLimit(1)
            }
            ForEach(chip.details, id: \.self) { Text($0).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(2) }
            if let error = chip.error {
                Text(error).kitType(.meta).foregroundColor(Danger.color).lineLimit(2).help(error)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .kitCard(radius: KitMetric.radiusMd)
    }
}

private struct MachinesEmptyState: View {
    let onCheckAgain: () -> Void

    private let example = """
    [
      {"name": "m1", "host": "m1", "user": "sondoan", "python": "/usr/bin/python3",
       "sdPath": "/Users/sondoan/sd/sd.py", "fileVault": true,
       "repos": [{"github": "owner/repo", "localPath": "/Users/sondoan/code/repo",
                  "checkCommand": "just check-affected"}]}
    ]
    """

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("No machines yet").kitType(.title).foregroundColor(KitInk.text)
            Text("Machines are the Macs and servers that run sd's overnight worker. Notchling reads them from this file, one entry per machine:")
                .kitType(.body).foregroundColor(KitInk.textMuted)
            Text(MachineRegistry.fileURL.path).kitType(.mono).foregroundColor(KitInk.text).textSelection(.enabled)
            Text(example).kitType(.mono).foregroundColor(KitInk.text).textSelection(.enabled)
                .padding(12).frame(maxWidth: .infinity, alignment: .leading).kitCard(radius: KitMetric.radiusMd)
            Text("Use absolute paths for sdPath and python. The command runs over ssh with the path quoted, so ~ is not expanded.")
                .kitType(.body).foregroundColor(KitInk.text)
            Button("Check again", action: onCheckAgain).buttonStyle(.kit(.primary))
        }
        .padding(32)
        .frame(maxWidth: 640, maxHeight: .infinity, alignment: .topLeading)
    }
}

// MARK: Cards

private struct Card<Content: View>: View {
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
        if let note = model.done[item.id], actions.lastError[item.id] == nil {
            Label(note, systemImage: "checkmark.circle.fill").kitType(.label).foregroundColor(KitInk.done)
        }
    }
}

private struct NoteField: View {
    let placeholder: String
    @Binding var text: String
    var onSubmit: () -> Void

    var body: some View {
        TextField(placeholder, text: $text, onCommit: onSubmit)
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
                if let outcome, outcome.merged, !outcome.pushed {
                    Button("Retry push") { model.perform(item, success: "Pushed and closed") { $0.retryPush(item) } }
                        .buttonStyle(.kit(.primary))
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
