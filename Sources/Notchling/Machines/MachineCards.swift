import SwiftUI

// MARK: - Pure pieces (tested)

/// The Add machine form: what is refused before anything reaches ssh. Name, host and user later become a vault
/// name, an ssh argument and a login, so each is checked against a short alphabet instead of being quoted.
enum MachineForm {
    static let nameProblem = "Name: lowercase letters, digits and hyphens"
    static let hostProblem = "Host: a Tailscale name or an address"
    static let userProblem = "User: lowercase letters, digits, hyphens and underscores"

    /// The problems in the order name, host, user; empty when the form may be used. `existing` are the names of
    /// the machines already in the registry.
    static func problems(name: String, host: String, user: String, existing: [String]) -> [String] {
        var out: [String] = []
        if !isName(name) {
            out.append(nameProblem)
        } else if existing.contains(name) {
            out.append("\(name) is already a machine")
        } else if RepoSteps.endsInReplacedKeyDate(name) {
            out.append("A machine name can't end in -<8 digits>; that's how replaced keys are titled")
        }
        if !isHost(host) { out.append(hostProblem) }
        if !isUser(user) { out.append(userProblem) }
        return out
    }

    private static func isName(_ s: String) -> Bool {
        guard let first = s.unicodeScalars.first, (1...40).contains(s.utf8.count) else { return false }
        return alnum(first) && s.unicodeScalars.allSatisfy { alnum($0) || $0 == "-" }
    }

    /// `^[A-Za-z0-9][A-Za-z0-9.-]*$`, or an IP address (IPv6 has colons).
    private static func isHost(_ s: String) -> Bool {
        guard let first = s.unicodeScalars.first, s.utf8.count <= 253 else { return false }
        if hostAlnum(first) && s.unicodeScalars.allSatisfy({ hostAlnum($0) || $0 == "." || $0 == "-" }) { return true }
        var v6 = in6_addr()
        return s.contains(":") && s.unicodeScalars.allSatisfy({ hex($0) || $0 == ":" || $0 == "." }) && inet_pton(AF_INET6, s, &v6) == 1
    }

    /// `^[a-z_][a-z0-9_-]*$`
    private static func isUser(_ s: String) -> Bool {
        guard let first = s.unicodeScalars.first, s.utf8.count <= 32 else { return false }
        return (lower(first) || first == "_") && s.unicodeScalars.allSatisfy { lower($0) || digit($0) || $0 == "_" || $0 == "-" }
    }

    private static func lower(_ c: Unicode.Scalar) -> Bool { c >= "a" && c <= "z" }
    private static func digit(_ c: Unicode.Scalar) -> Bool { c >= "0" && c <= "9" }
    private static func hex(_ c: Unicode.Scalar) -> Bool { digit(c) || (c >= "a" && c <= "f") || (c >= "A" && c <= "F") }
    private static func alnum(_ c: Unicode.Scalar) -> Bool { lower(c) || digit(c) }
    /// Host names may use capitals; machine names may not.
    private static func hostAlnum(_ c: Unicode.Scalar) -> Bool { alnum(c) || (c >= "A" && c <= "Z") }
}

/// What a card says a machine can do: `macOS 26.1 · arm64 · Xcode 26.1 · 2 simulators · Docker rootless · 120 GB free`.
enum AbilitiesLine {
    static func text(_ a: MachineAbilities?, os: MachineOS?) -> String {
        var parts: [String] = []
        let version = a?.osVersion.flatMap { $0.isEmpty ? nil : $0 }
        switch os {
        case .macOS?: parts.append(version.map { "macOS \($0)" } ?? "macOS")
        case .linux?: parts.append(version ?? "Linux")
        case nil: if let version { parts.append(version) }
        }
        if let arch = a?.arch, !arch.isEmpty { parts.append(arch) }
        if let xcode = a?.xcode, !xcode.isEmpty { parts.append("Xcode \(xcode)") }
        if let count = a?.simulators.count, count > 0 { parts.append("\(count) simulator\(count == 1 ? "" : "s")") }
        if let docker = a?.docker {
            switch docker {
            case .none: parts.append("no Docker")
            case .stopped: parts.append("Docker stopped")
            case .rootful: parts.append("Docker rootful")
            case .rootless: parts.append("Docker rootless")
            }
        }
        if let gb = a?.freeDiskGB { parts.append("\(gb) GB free") }
        return parts.isEmpty ? "not checked yet" : parts.joined(separator: " · ")
    }
}

extension MachineChip {
    /// The lines under a chip's name: the night's jobs, the week's use, the job in flight.
    static func details(for status: WorkerStatus) -> [String] {
        var details = ["\(status.jobsTonight) \(status.jobsTonight == 1 ? "job" : "jobs") tonight"]
        if let pct = status.weeklyPct { details.append("week \(Int(pct.rounded()))%") }
        if let job = status.current {
            details.append("now \(job.repo) #\(job.issue)" + (job.stage.isEmpty ? "" : " · \(job.stage)"))
        }
        return details
    }
}

/// What a job card shows from the worker's status: the chip's details, and what plan C never showed.
enum JobFacts {
    static func lines(_ status: WorkerStatus) -> [String] {
        var lines = MachineChip.details(for: status)
        if let paused = status.paused, !paused.isEmpty { lines.append("paused: \(paused)") }
        for key in status.health.keys.sorted() {
            if let value = status.health[key], value != "ok" { lines.append("health \(key): \(value)") }
        }
        if !status.unrouted.isEmpty { lines.append("unrouted: " + status.unrouted.joined(separator: ", ")) }
        if status.outboxFailed > 0 { lines.append("outbox: \(status.outboxFailed) failed") }
        if status.testflight?.quiet == true { lines.append("TestFlight: no new feedback in over 7 days") }
        return lines
    }
}

enum UpdateOffer {
    static func title(count: Int) -> String { "Update \(count) machine\(count == 1 ? "" : "s")" }
}

/// One secret file of a machine: `claude-worker → ~/.config/agents/claude.env · mode 600, 41 bytes · expires in 12 days`.
enum SecretRowFacts {
    /// `facts` is the check of the step that delivers it (mode and size only; a value is never read back).
    static func text(usage: SecretUsage, entry: VaultEntry?, facts: CheckResult?, now: Date,
                     calendar: Calendar = .current) -> String {
        var parts = ["\(usage.name) → \(usage.remotePath)"]
        if let facts {
            switch facts.state {
            case .pass(let s), .fail(let s), .cantTell(let s): parts.append(s)
            }
        }
        if let entry {
            if let day = entry.expires, !day.isEmpty {
                switch VaultExpiry.status(of: day, now: now, calendar: calendar) {
                case .expired?: parts.append("expired \(day)")
                case .today?: parts.append("expires today")
                case .inDays?:
                    parts.append("expires " + (VaultExpiry.phrase(of: day, now: now, calendar: calendar) ?? day))
                case nil: parts.append("expires \(day)")
                }
            }
        } else {
            parts.append("not in the vault")
        }
        return parts.joined(separator: " · ")
    }
}

/// The sentences of the window's banners.
enum RegistryNotes {
    static func unreadable(reason: String) -> String {
        "machines.json can't be read: \(reason). Notchling won't change it. Fix or move it, then Check again."
    }

    static func migrated(backup: String) -> String {
        "machines.json was upgraded to the new format. The old file is kept at \(backup)."
    }

    /// Nil when no secret is still under an old name with a free new one.
    static func rename(_ pending: [(VaultEntry, String)]) -> String? {
        let names = pending.map { $0.0.name }
        guard !names.isEmpty else { return nil }
        let list = names.count == 1 ? names[0] : names.dropLast().joined(separator: ", ") + " and " + names[names.count - 1]
        return "Rename \(list) to the new name\(names.count == 1 ? "" : "s")"
    }
}

// MARK: Jobs

struct JobRepoDraft: Identifiable, Equatable {
    var id = UUID()
    var github = ""
    var localPath = ""
    var checkCommand = ""
    /// Empty means the default, `~/src/<name>`.
    var machinePath = ""
    var verifySkill = ""
}

/// The job sheet's fields as text, before they are checked.
struct JobDraft: Identifiable, Equatable {
    var machine: String
    var repos: [JobRepoDraft] = []
    var kinds: [String] = []
    var healthURL = ""
    /// Shown, never edited: Token scope sets it.
    var tokenExpires: String?
    /// Whether the machine has no sd worker job yet.
    var isNew = true
    /// What the sheet edits; kept so Save keeps python, sdPath and the vault name.
    var base: SDWorkerSettings?

    var id: String { machine }

    static let kindChoices = ["ios", "backend"]

    init(machine: String) { self.machine = machine }

    init(machine: Machine) {
        self.machine = machine.name
        guard let job = machine.sdWorker else { return }
        isNew = false
        base = job
        repos = job.repos.map {
            JobRepoDraft(github: $0.github, localPath: $0.localPath, checkCommand: $0.checkCommand,
                         machinePath: $0.machinePath ?? "", verifySkill: $0.verifySkill ?? "")
        }
        kinds = job.kinds
        healthURL = job.healthURL ?? ""
        tokenExpires = job.tokenExpires
    }
}

/// Checks the job sheet. A repo name, a path on the machine or a skill path reaches scripts, so each is held to
/// plain characters.
enum JobForm {
    static func problems(_ draft: JobDraft) -> [String] {
        var out: [String] = []
        func add(_ s: String) { if !out.contains(s) { out.append(s) } }
        if draft.repos.isEmpty { add("Add at least one repository") }
        var seen = Set<String>()
        for repo in draft.repos {
            let github = repo.github.trimmingCharacters(in: .whitespaces)
            if !isRepo(github) {
                add("Repository: owner/name, like me/app")
            } else if !seen.insert(github.lowercased()).inserted {
                add("\(github) is listed twice")
            }
            if !repo.localPath.trimmingCharacters(in: .whitespaces).hasPrefix("/") { add("Checkout on this Mac: an absolute path") }
            if repo.checkCommand.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { add("Check command: required") }
            let machinePath = repo.machinePath.trimmingCharacters(in: .whitespaces)
            if !machinePath.isEmpty && !isMachinePath(machinePath) {
                add("Machine path: ~/ or an absolute path with plain characters")
            }
            let skill = repo.verifySkill.trimmingCharacters(in: .whitespaces)
            if !skill.isEmpty && (skill.hasPrefix("/") || !RemoteShell.isSafeMachinePath("/" + skill)) {
                add("Verify skill: a path inside the checkout")
            }
        }
        let health = draft.healthURL.trimmingCharacters(in: .whitespaces)
        if !health.isEmpty && !isHealthURL(health) { add("Health URL: http or https") }
        return out
    }

    /// The settings the draft makes. A job being edited keeps its python, sdPath, token expiry and vault name.
    static func settings(_ draft: JobDraft, base: SDWorkerSettings?) -> SDWorkerSettings {
        let repos = draft.repos.map { r -> MachineRepo in
            let machinePath = r.machinePath.trimmingCharacters(in: .whitespaces)
            let skill = r.verifySkill.trimmingCharacters(in: .whitespaces)
            return MachineRepo(github: r.github.trimmingCharacters(in: .whitespaces),
                               localPath: r.localPath.trimmingCharacters(in: .whitespaces),
                               checkCommand: r.checkCommand.trimmingCharacters(in: .whitespacesAndNewlines),
                               machinePath: machinePath.isEmpty ? nil : machinePath,
                               verifySkill: skill.isEmpty ? nil : skill)
        }
        let health = draft.healthURL.trimmingCharacters(in: .whitespaces)
        let secret = (base?.issuesSecret).flatMap { $0.isEmpty ? nil : $0 } ?? "\(draft.machine)-github-issues"
        return SDWorkerSettings(python: base?.python ?? "python3", sdPath: base?.sdPath ?? "", repos: repos,
                                kinds: draft.kinds, healthURL: health.isEmpty ? nil : health,
                                tokenExpires: base?.tokenExpires, issuesSecret: secret)
    }

    /// `owner/name`, GitHub's alphabet.
    private static func isRepo(_ s: String) -> Bool {
        let parts = s.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 2, s.utf8.count <= 140 else { return false }
        return parts.allSatisfy { part in
            guard let first = part.unicodeScalars.first, plain(first) && first != "." && first != "-" && first != "_" else { return false }
            return part.unicodeScalars.allSatisfy { plain($0) || $0 == "." || $0 == "-" || $0 == "_" }
        }
    }

    private static func plain(_ c: Unicode.Scalar) -> Bool {
        (c >= "A" && c <= "Z") || (c >= "a" && c <= "z") || (c >= "0" && c <= "9")
    }

    /// `~/rest` or an absolute path, plain characters only (it goes into scripts on the machine).
    private static func isMachinePath(_ p: String) -> Bool {
        if p.hasPrefix("~/") { return RemoteShell.isSafeMachinePath(String(p.dropFirst(1))) }
        return RemoteShell.isSafeMachinePath(p)
    }

    private static func isHealthURL(_ s: String) -> Bool {
        guard let url = URL(string: s), let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
              let host = url.host, !host.isEmpty, s.utf8.count <= 300 else { return false }
        let banned = Set(" \t\n\r\"'`$\\;<>|&(){}")
        return !s.contains(where: { banned.contains($0) })
    }
}

// MARK: - Views

private func ink(_ state: CheckState) -> Color {
    switch state {
    case .pass: return KitInk.done
    case .fail: return Danger.color
    case .cantTell: return KitInk.textFaint
    }
}

private func glyph(_ state: CheckState) -> String {
    switch state {
    case .pass: return "checkmark.circle.fill"
    case .fail: return "xmark.circle.fill"
    case .cantTell: return "questionmark.circle"
    }
}

private func text(_ state: CheckState) -> String {
    switch state {
    case .pass(let s), .fail(let s), .cantTell(let s): return s
    }
}

private struct Banner<Trailing: View>: View {
    let icon: String
    let message: String
    var tint: Color = KitInk.accentText
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).foregroundColor(tint)
            // plain text: a banner may carry a reason that came from a file or a machine
            Text(verbatim: message).kitType(.body).foregroundColor(KitInk.text)
                .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            Spacer(minLength: 8)
            trailing
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .kitCard(radius: KitMetric.radiusMd, fill: KitInk.accentWash)
    }
}

/// The Machines half of the window: banners, a card per machine, Add machine….
struct MachinesTab: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var engine: SetupEngine
    var onCheckAgain: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            banners
            if model.machines.isEmpty {
                empty
            } else {
                ForEach(model.machines, id: \.name) { MachineCardView(model: model, engine: engine, machine: $0) }
            }
        }
    }

    @ViewBuilder private var banners: some View {
        if let note = model.unreadableNote {
            VStack(alignment: .leading, spacing: 6) {
                Banner(icon: "exclamationmark.triangle.fill", message: note, tint: Danger.color) {
                    Button("Check again", action: onCheckAgain).buttonStyle(.kit(.secondary))
                }
                Text(verbatim: model.registryPath).kitType(.mono).foregroundColor(KitInk.textMuted).textSelection(.enabled)
            }
        }
        if let note = model.migrationNote {
            Banner(icon: "arrow.triangle.2.circlepath", message: note) {
                Button("Dismiss") { model.dismissMigrationNote() }.buttonStyle(.kit(.ghost))
            }
        }
        if let rename = model.renameBanner {
            VStack(alignment: .leading, spacing: 6) {
                Banner(icon: "key.fill", message: rename) {
                    Button("Rename") { model.renameSecrets() }.buttonStyle(.kit(.primary))
                }
                if let note = model.renameNote {
                    Text(verbatim: note).kitType(.meta).foregroundColor(Danger.color)
                }
            }
        }
        ForEach(model.updateOffers) { offer in
            VStack(alignment: .leading, spacing: 6) {
                Banner(icon: "arrow.up.circle.fill", message: "\(offer.secret) has a new value.") {
                    Button(offer.title) { model.update(secret: offer.secret) }.buttonStyle(.kit(.primary))
                        .disabled(!engine.running.isEmpty)
                }
                if let note = model.updateNotes[offer.secret] {
                    Text(verbatim: note).kitType(.meta).foregroundColor(Danger.color)
                }
            }
        }
        if let problem = model.registryProblem {
            Label(problem, systemImage: "exclamationmark.triangle.fill").kitType(.label).foregroundColor(Danger.color)
        }
        if engine.saveRefused {
            Label("What the checks found was not saved: machines.json can't be written.", systemImage: "exclamationmark.triangle")
                .kitType(.label).foregroundColor(KitInk.textMuted)
        }
    }

    private var empty: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("No machines yet").kitType(.title).foregroundColor(KitInk.text)
            Text("Machines are the Macs and servers that do Notchling's overnight jobs. Add one by its Tailscale name; Notchling checks it and lists what it still needs.")
                .kitType(.body).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
            Text(verbatim: model.registryPath).kitType(.mono).foregroundColor(KitInk.textFaint).textSelection(.enabled)
            Button("Add machine…") { model.setupSheet = .add }.buttonStyle(.kit(.primary)).disabled(!model.canAdd)
        }
        .padding(.vertical, 12)
        .frame(maxWidth: 560, alignment: .leading)
    }
}

private struct MachineCardView: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var engine: SetupEngine
    let machine: Machine
    @State private var removing = false
    @State private var logOpen = false
    @State private var howTo: StepID?

    private var shown: Machine { engine.overlaid(machine) }
    private var busy: Bool { engine.running[machine.name] != nil }

    var body: some View {
        Card {
            header
            if shown.sdWorker != nil { chipLine }
            Text(verbatim: AbilitiesLine.text(shown.abilities, os: shown.os)).kitType(.meta).foregroundColor(KitInk.textMuted)
            if machine.admin == .rootSSH {
                Label("Admin steps run as root over SSH on \(machine.host).", systemImage: "exclamationmark.triangle.fill")
                    .kitType(.label).foregroundColor(KitInk.accentText)
            }
            VStack(alignment: .leading, spacing: 8) {
                ForEach(engine.steps(for: machine, among: model.machines), id: \.id.raw) { step in stepRow(step) }
            }
            let rows = model.secretRows(for: shown)
            if !rows.isEmpty { secrets(rows) }
            progress
            outcome
            log
        }
        .confirmationDialog("Remove \(machine.name) from Notchling? The machine itself is not changed.", isPresented: $removing) {
            Button("Remove", role: .destructive) { model.remove(machine.name) }
        }
        .onChange(of: busy) { if $0 { logOpen = true } }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(verbatim: machine.name).kitType(.title).foregroundColor(KitInk.text)
            Text(verbatim: "\(machine.user)@\(machine.host)").kitType(.meta).foregroundColor(KitInk.textMuted).textSelection(.enabled)
            Spacer()
            Menu {
                Button("sudo as \(machine.user)") { model.setAdmin(machine.name, .sudo) }
                Button("root over SSH") { model.setAdmin(machine.name, .rootSSH) }
            } label: {
                Text(machine.admin == .sudo ? "Admin: sudo as \(machine.user)" : "Admin: root over SSH")
            }
            .menuStyle(.borderlessButton).fixedSize()
            Button("Check") { Task { await model.check(machine.name) } }.buttonStyle(.kit(.secondary)).disabled(busy)
            Button("Set up all") { model.requestSetUpAll(on: machine.name) }.buttonStyle(.kit(.primary)).disabled(busy)
            Button("Remove…") { removing = true }.buttonStyle(.kit(.ghost))
        }
    }

    private var chipLine: some View {
        let chip = model.chip(for: machine)
        return HStack(spacing: 8) {
            Circle().fill(tint(chip.tone)).frame(width: 8, height: 8)
            Text(verbatim: chip.word).kitType(.label).foregroundColor(chip.tone == .bad ? Danger.color : KitInk.textMuted)
            ForEach(chip.details, id: \.self) { Text(verbatim: "· " + $0).kitType(.meta).foregroundColor(KitInk.textMuted) }
            if let error = chip.error { Text(verbatim: error).kitType(.meta).foregroundColor(Danger.color).lineLimit(1).help(error) }
        }
    }

    private func tint(_ tone: MachineChip.Tone) -> Color {
        switch tone {
        case .good: return KitInk.done
        case .warn: return KitInk.needs
        case .bad: return Danger.color
        case .neutral: return KitInk.textFaint
        }
    }

    @ViewBuilder private func stepRow(_ step: SetupStep) -> some View {
        let result = engine.results[machine.name]?[step.id]
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if let result {
                    Image(systemName: glyph(result.state)).foregroundColor(ink(result.state))
                } else {
                    Image(systemName: "circle.dotted").foregroundColor(KitInk.textFaint)
                }
                Text(verbatim: step.title).kitType(.body).foregroundColor(KitInk.text)
                Text(verbatim: result.map { text($0.state) } ?? "checking…").kitType(.meta)
                    .foregroundColor(KitInk.textMuted).lineLimit(2).textSelection(.enabled)
                Spacer(minLength: 8)
                action(for: step, result: result)
            }
            if let result {
                ForEach(Array(result.rows.enumerated()), id: \.offset) { _, row in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Image(systemName: row.ok ? "checkmark" : "xmark").font(.system(size: 9, weight: .bold))
                            .foregroundColor(row.ok ? KitInk.done : Danger.color)
                        Text(verbatim: row.name).kitType(.meta).foregroundColor(KitInk.text)
                        Text(verbatim: row.detail).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(2)
                    }
                    .padding(.leading, 26)
                }
            }
            if howTo == step.id, case .howTo(let how) = step.fix {
                Text(verbatim: how).kitType(.meta).foregroundColor(KitInk.text).textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(10).frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.sidebar))
                    .padding(.leading, 26)
            }
        }
    }

    @ViewBuilder private func action(for step: SetupStep, result: CheckResult?) -> some View {
        // Replace key: only on a click, and only while the key is ✓ or GitHub has no record of it
        if step.replace != nil, RepoSteps.offersReplace(result) {
            Button("Replace key…") { model.requestReplaceKey(step.id, on: machine.name) }
                .buttonStyle(.kit(.ghost)).disabled(busy)
                .help("Makes a new read-only deploy key, proves it works, then removes the old one")
        }
        switch step.fix {
        case .run:
            if case .fail? = result?.state {
                Button("Set up") { model.requestSetUp(step.id, on: machine.name) }.buttonStyle(.kit(.secondary)).disabled(busy)
            }
        case .howTo:
            if case .fail? = result?.state {
                Button(howTo == step.id ? "Hide" : "How to…") { howTo = howTo == step.id ? nil : step.id }.buttonStyle(.kit(.ghost))
            }
        case .toggle:
            EmptyView()
        }
    }

    private func secrets(_ rows: [SecretRow]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Secrets").kitType(.section).foregroundColor(KitInk.textMuted)
            ForEach(rows) { row in
                HStack(spacing: 8) {
                    Text(verbatim: row.text).kitType(.meta).foregroundColor(KitInk.text).textSelection(.enabled)
                    Spacer(minLength: 8)
                    // on every delivered secret, an OK one too: the file can hold an older value than the vault
                    Button("Send again") { model.requestSendAgain(row.usage) }.buttonStyle(.kit(.ghost))
                        .disabled(busy || !row.canSend)
                    Button("Renew…") { model.renew(row.usage.name) }.buttonStyle(.kit(.ghost))
                }
            }
        }
    }

    @ViewBuilder private var progress: some View {
        if let id = engine.running[machine.name] {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(verbatim: "Running " + (engine.steps(for: machine, among: model.machines).first { $0.id == id }?.title ?? id.raw) + "…")
                    .kitType(.label).foregroundColor(KitInk.textMuted)
            }
        }
    }

    @ViewBuilder private var outcome: some View {
        if let line = model.outcomes[machine.name], !busy {
            // guidance (the vault form was opened) is not a red failure
            Label {
                Text(verbatim: line.text).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            } icon: {
                Image(systemName: line.ok ? "checkmark.circle.fill" : line.guidance ? "info.circle.fill" : "xmark.circle.fill")
            }
            .kitType(.label)
            .foregroundColor(line.ok ? KitInk.done : line.guidance ? KitInk.accentText : Danger.color)
        }
    }

    @ViewBuilder private var log: some View {
        let lines = engine.logs[machine.name] ?? []
        if !lines.isEmpty {
            let tail = LogTail.make(lines)
            DisclosureGroup("Log (\(lines.count) lines)", isExpanded: $logOpen) {
                if let note = tail.note { Text(verbatim: note).kitType(.meta).foregroundColor(KitInk.textMuted) }
                ScrollView {
                    Text(verbatim: tail.shown.joined(separator: "\n")).kitType(.mono).foregroundColor(KitInk.text)
                        .frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
                }
                .frame(maxHeight: 220)
                .padding(10)
                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.sidebar))
            }
            .kitType(.label).foregroundColor(KitInk.textMuted)
        }
    }
}

// MARK: Jobs

/// A machine's sd worker job: its settings, the Running switch and what the worker says about itself.
struct JobCardView: View {
    @ObservedObject var model: MachinesModel
    @ObservedObject var engine: SetupEngine
    @ObservedObject var poller: MachinePoller
    let machine: Machine

    private var busy: Bool { engine.running[machine.name] != nil }
    private var running: CheckResult? { engine.results[machine.name]?[.running] }

    var body: some View {
        Card {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(verbatim: machine.name).kitType(.title).foregroundColor(KitInk.text)
                Text("sd worker").kitType(.meta).foregroundColor(KitInk.textMuted)
                Spacer()
                if let job = machine.sdWorker {
                    Button("Edit…") { model.setupSheet = .job(JobDraft(machine: machine)) }.buttonStyle(.kit(.secondary))
                    runningSwitch(job)
                } else {
                    Button("Add sd worker job…") { model.setupSheet = .job(JobDraft(machine: machine.name)) }
                        .buttonStyle(.kit(.primary))
                }
            }
            if let job = machine.sdWorker { settings(job) }
            if let status = model.status(for: machine) {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(JobFacts.lines(status), id: \.self) { Text(verbatim: $0).kitType(.meta).foregroundColor(KitInk.textMuted) }
                }
            }
            if let line = model.outcomes[machine.name], !busy {
                Text(verbatim: line.text).kitType(.meta)
                    .foregroundColor(line.ok ? KitInk.done : line.guidance ? KitInk.accentText : Danger.color)
                    .textSelection(.enabled)
            }
        }
    }

    private func runningSwitch(_ job: SDWorkerSettings) -> some View {
        // on, off or can't tell from its own probe; only turning it ON waits for the steps before it
        let sw = model.runningSwitch(for: machine)
        return HStack(spacing: 8) {
            if let note = sw.note { Text(verbatim: note).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1) }
            Toggle("Running", isOn: Binding(get: { sw.on }, set: { model.requestToggle(on: machine.name, to: $0) }))
                .toggleStyle(.switch).disabled(!sw.enabled)
        }
    }

    private func settings(_ job: SDWorkerSettings) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            ForEach(job.repos, id: \.github) { repo in
                Text(verbatim: "\(repo.github) → \(repo.machinePath ?? "~/src/" + (repo.github.split(separator: "/").last.map(String.init) ?? ""))")
                    .kitType(.meta).foregroundColor(KitInk.text)
            }
            Text(verbatim: "kinds: " + (job.kinds.isEmpty ? "none" : job.kinds.joined(separator: ", "))
                 + (job.healthURL.map { " · health \($0)" } ?? ""))
                .kitType(.meta).foregroundColor(KitInk.textMuted)
            if let day = job.tokenExpires {
                Text(verbatim: "token expires \(day)").kitType(.meta).foregroundColor(KitInk.textMuted)
            }
        }
    }
}

// MARK: Sheets

struct SetupSheetView: View {
    @ObservedObject var model: MachinesModel
    let sheet: SetupSheet

    var body: some View {
        switch sheet {
        case .fix(let prompt):
            FixConfirmSheet(prompt: prompt, run: { model.confirm(prompt, password: $0) }, cancel: { model.cancel(prompt) })
        case .add:
            AddMachineSheet(model: model)
        case .job(let draft):
            JobSheet(model: model, draft: draft)
        }
    }
}

/// The commands, then the admin password (or the root warning), then Cancel and Run. The password lives in this
/// sheet's `@State` only and is cleared when it closes.
private struct FixConfirmSheet: View {
    let prompt: FixPrompt
    let run: (String?) -> Void
    let cancel: () -> Void
    @State private var password = ""

    private var needsPassword: Bool { prompt.needsAdmin && !prompt.root && prompt.blocked == nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(verbatim: prompt.title).kitType(.title).foregroundColor(KitInk.text)
            Text(prompt.kind == .all ? "These steps run in order and stop at the first one that fails:" : "These commands run on \(prompt.user)@\(prompt.host):")
                .kitType(.body).foregroundColor(KitInk.textMuted)
            ScrollView {
                Text(verbatim: prompt.display).kitType(.mono).foregroundColor(KitInk.text).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 80, maxHeight: 300)
            .padding(10)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.sidebar))
            if let blocked = prompt.blocked {
                Label(blocked, systemImage: "xmark.octagon.fill").kitType(.label).foregroundColor(Danger.color)
                    .fixedSize(horizontal: false, vertical: true)
            } else if let warning = prompt.warning {
                Label(warning, systemImage: "exclamationmark.triangle.fill").kitType(.label).foregroundColor(KitInk.accentText)
            } else if needsPassword {
                SecureField("Admin password for \(prompt.user)@\(prompt.host)", text: $password)
                    .textFieldStyle(.roundedBorder)
            }
            HStack {
                Spacer()
                Button("Cancel") { password = ""; cancel() }.buttonStyle(.kit(.secondary)).keyboardShortcut(.cancelAction)
                Button("Run") {
                    let given = password
                    password = ""
                    run(needsPassword ? given : nil)
                }
                .buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
                .disabled(prompt.blocked != nil || (needsPassword && password.isEmpty))
            }
        }
        .padding(20)
        .frame(minWidth: 520, idealWidth: 600)
        .background(KitInk.window)
        .onDisappear { password = "" }
    }
}

private struct AddMachineSheet: View {
    @ObservedObject var model: MachinesModel
    @State private var name = ""
    @State private var host = ""
    @State private var user = ""
    @State private var check: DraftCheck?
    @State private var checking = false
    @State private var error: String?

    private var problems: [String] {
        guard !name.isEmpty || !host.isEmpty || !user.isEmpty else { return [] }
        return MachineForm.problems(name: name, host: host, user: user, existing: model.machines.map(\.name))
    }

    private var formOK: Bool {
        !name.isEmpty && !host.isEmpty && !user.isEmpty
            && MachineForm.problems(name: name, host: host, user: user, existing: model.machines.map(\.name)).isEmpty
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add Machine").kitType(.title).foregroundColor(KitInk.text)
            Text("Name it (m1), give its Tailscale name or address, and the user the worker runs as. Check only reads from the machine.")
                .kitType(.body).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
            field("Name", text: $name, prompt: "m1")
            field("Host", text: $host, prompt: "m1 or m1.tail1234.ts.net")
            field("User", text: $user, prompt: "sondoan")
            ForEach(problems, id: \.self) { Text(verbatim: $0).kitType(.meta).foregroundColor(Danger.color) }
            if let check { result(check) }
            if let error { Text(verbatim: error).kitType(.meta).foregroundColor(Danger.color) }
            HStack {
                if checking { ProgressView().controlSize(.small) }
                Spacer()
                Button("Cancel") { model.setupSheet = nil }.buttonStyle(.kit(.secondary)).keyboardShortcut(.cancelAction)
                Button("Check") { runCheck() }.buttonStyle(.kit(.secondary)).disabled(!formOK || checking || !model.canAdd)
                Button("Add") { add() }.buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
                    .disabled(!(check?.reachable ?? false) || !formOK || !model.canAdd || checking)
            }
        }
        .padding(20)
        .frame(minWidth: 480, idealWidth: 520)
        .background(KitInk.window)
        .onChange(of: name) { _ in check = nil; error = nil }
        .onChange(of: host) { _ in check = nil; error = nil }
        .onChange(of: user) { _ in check = nil; error = nil }
    }

    private func field(_ label: String, text: Binding<String>, prompt: String) -> some View {
        HStack {
            Text(label).kitType(.label).foregroundColor(KitInk.textMuted).frame(width: 48, alignment: .leading)
            TextField(prompt, text: text).textFieldStyle(.roundedBorder).disableAutocorrection(true)
        }
    }

    @ViewBuilder private func result(_ check: DraftCheck) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(check.lines, id: \.title) { line in
                HStack(spacing: 8) {
                    Image(systemName: glyph(line.state)).foregroundColor(ink(line.state))
                    Text(verbatim: line.title).kitType(.body).foregroundColor(KitInk.text)
                    Text(verbatim: text(line.state)).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(3).textSelection(.enabled)
                }
            }
            if !check.abilitiesLine.isEmpty {
                Text(verbatim: check.abilitiesLine).kitType(.meta).foregroundColor(KitInk.textMuted)
            }
        }
    }

    private func runCheck() {
        checking = true
        let (n, h, u) = (name, host, user)
        Task {
            let found = await model.checkDraft(name: n, host: h, user: u)
            checking = false
            if (n, h, u) == (name, host, user) { check = found }
        }
    }

    private func add() {
        guard let check else { return }
        if let why = model.add(name: name, host: host, user: user, check: check) {
            error = why
            return
        }
        let added = name
        model.setupSheet = nil
        Task { await model.check(added) }
    }
}

private struct JobSheet: View {
    @ObservedObject var model: MachinesModel
    @State private var draft: JobDraft
    @State private var error: String?

    init(model: MachinesModel, draft: JobDraft) {
        self.model = model
        _draft = State(initialValue: draft)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(draft.isNew ? "Add sd Worker Job" : "Edit sd Worker Job").kitType(.title).foregroundColor(KitInk.text)
            Text(verbatim: "on \(draft.machine)").kitType(.meta).foregroundColor(KitInk.textMuted)
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach($draft.repos) { $repo in repoFields($repo) }
                    Button("Add repository") { draft.repos.append(JobRepoDraft()) }.buttonStyle(.kit(.secondary))
                    HStack(spacing: 14) {
                        Text("Kinds").kitType(.label).foregroundColor(KitInk.textMuted)
                        ForEach(JobDraft.kindChoices, id: \.self) { kind in
                            Toggle(kind, isOn: Binding(get: { draft.kinds.contains(kind) }, set: { on in
                                draft.kinds.removeAll { $0 == kind }
                                if on { draft.kinds.append(kind) }
                            }))
                        }
                    }
                    labeled("Health URL", text: $draft.healthURL, prompt: "https://example.com/health (optional)")
                    if let day = draft.tokenExpires {
                        Text(verbatim: "Token expires \(day) (set by the token check)").kitType(.meta).foregroundColor(KitInk.textMuted)
                    }
                }
            }
            .frame(minHeight: 200, maxHeight: 420)
            ForEach(JobForm.problems(draft).prefix(3), id: \.self) { Text(verbatim: $0).kitType(.meta).foregroundColor(Danger.color) }
            if let error { Text(verbatim: error).kitType(.meta).foregroundColor(Danger.color) }
            HStack {
                Spacer()
                Button("Cancel") { model.setupSheet = nil }.buttonStyle(.kit(.secondary)).keyboardShortcut(.cancelAction)
                Button("Save") {
                    if let why = model.saveJob(draft) { error = why } else { model.setupSheet = nil }
                }
                .buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction).disabled(!JobForm.problems(draft).isEmpty)
            }
        }
        .padding(20)
        .frame(minWidth: 560, idealWidth: 620)
        .background(KitInk.window)
    }

    private func labeled(_ label: String, text: Binding<String>, prompt: String) -> some View {
        HStack {
            Text(label).kitType(.label).foregroundColor(KitInk.textMuted).frame(width: 110, alignment: .leading)
            TextField(prompt, text: text).textFieldStyle(.roundedBorder).disableAutocorrection(true)
        }
    }

    private func repoFields(_ repo: Binding<JobRepoDraft>) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            labeled("Repository", text: repo.github, prompt: "owner/name")
            labeled("Checkout here", text: repo.localPath, prompt: "/Users/you/code/name")
            labeled("Check command", text: repo.checkCommand, prompt: "just check-affected")
            labeled("Machine path", text: repo.machinePath, prompt: "default ~/src/<name>")
            labeled("Verify skill", text: repo.verifySkill, prompt: ".claude/skills/verify/SKILL.md (optional)")
            Button("Remove repository") { draft.repos.removeAll { $0.id == repo.wrappedValue.id } }.buttonStyle(.kit(.ghost))
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .kitCard(radius: KitMetric.radiusMd)
    }
}
