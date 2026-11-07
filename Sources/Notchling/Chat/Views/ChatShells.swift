import SwiftUI

/// The Details panel's Background Shells: what Claude Code's Background dialog shows. Running ones
/// first with Stop; Output opens the live output file in the file panel (it reloads as the file grows).
struct BackgroundShellsSection: View {
    let shells: [BackgroundShell]
    let finished: [String: String]
    var openOutput: (URL) -> Void
    /// Each shell's state, re-read every 3 s off the main thread (one `lsof` per shell still running).
    @State private var states: [String: BackgroundShells.State] = [:]
    @State private var stopping: Set<String> = []
    @State private var showEnded = false

    private static let endedShown = 8

    var body: some View {
        let running = shells.filter { states[$0.id] == .running }
        let ended = shells.filter { if case .ended = states[$0.id] { return true } else { return false } }.reversed()
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(running.isEmpty ? "Background Shells · \(shells.count)" : "Background Shells · \(running.count) running")
                    .kitType(.section).foregroundColor(KitInk.textFaint)
                Spacer(minLength: 0)
                if !ended.isEmpty {
                    Button(showEnded ? "Hide Ended" : "Ended · \(ended.count)") { showEnded.toggle() }
                        .buttonStyle(.kit(.ghost))
                        .part("Show Ended Shells")
                }
            }
            if running.isEmpty && !showEnded {
                Text("None running").kitType(.meta).foregroundColor(KitInk.textFaint)
            }
            ForEach(running) { row($0, state: .running) }
            if showEnded {
                ForEach(Array(ended.prefix(Self.endedShown))) { row($0, state: states[$0.id] ?? .ended("ended")) }
            }
        }
        .task(id: shells.map(\.id) + finished.keys.sorted()) {
            while !Task.isCancelled {
                let list = shells, done = finished, known = states
                let fresh = await Task.detached(priority: .utility) { () -> [String: BackgroundShells.State] in
                    var out: [String: BackgroundShells.State] = [:]
                    for shell in list {
                        // An ended shell never runs again; only the running ones need lsof.
                        if let status = done[shell.id] { out[shell.id] = .ended(status); continue }
                        if case .ended(let s)? = known[shell.id] { out[shell.id] = .ended(s); continue }
                        out[shell.id] = BackgroundShells.state(shell, finished: done)
                    }
                    return out
                }.value
                if fresh != states { states = fresh }
                stopping.formIntersection(fresh.filter { $0.value == .running }.keys)
                try? await Task.sleep(nanoseconds: 3_000_000_000)
            }
        }
    }

    private func row(_ shell: BackgroundShell, state: BackgroundShells.State) -> some View {
        let isRunning = state == .running
        return VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 7) {
                Circle().fill(dotColor(state)).frame(width: 7, height: 7)
                VStack(alignment: .leading, spacing: 2) {
                    if shell.title != "Ran " + String(shell.command.prefix(60)) {
                        Text(shell.title).kitType(.meta).foregroundColor(KitInk.text).lineLimit(2)
                    }
                    Text(shell.command).kitType(.mono).foregroundColor(KitInk.textMuted).lineLimit(3)
                        .textSelection(.enabled)
                    Text(statusLine(shell, state)).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            HStack(spacing: 6) {
                Spacer().frame(width: 7)
                Button("Output") { openOutput(URL(fileURLWithPath: shell.outputPath)) }
                    .buttonStyle(.kit(.secondary))
                    .disabled(!FileManager.default.fileExists(atPath: shell.outputPath))
                    .help(shell.outputPath)
                    .part("Shell Output Button")
                if isRunning {
                    Button(stopping.contains(shell.id) ? "Stopping…" : "Stop") {
                        stopping.insert(shell.id)
                        BackgroundShells.stop(shell)
                    }
                    .buttonStyle(.kit(.secondary))
                    .disabled(stopping.contains(shell.id))
                    .help("End this shell and everything it started, like x in Claude Code's Background list")
                    .part("Stop Shell Button")
                }
            }
        }
        .padding(.vertical, 3)
        .part("Background Shell", .leading)
    }

    private func dotColor(_ state: BackgroundShells.State) -> Color {
        switch state {
        case .running: return KitInk.working
        case .ended(let status): return status == "completed" ? KitInk.done : (status == "ended" ? KitInk.textFaint : KitInk.needs)
        }
    }

    private func statusLine(_ shell: BackgroundShell, _ state: BackgroundShells.State) -> String {
        let since = shell.started.map { " · started " + RelativeDateTimeFormatter().localizedString(for: $0, relativeTo: Date()) } ?? ""
        switch state {
        case .running: return "running" + since
        case .ended(let status): return status + since
        }
    }
}

/// "2 shells" in the session header while background shells run, like Claude Code's footer; a click
/// shows the Details panel, where they're listed with Output and Stop.
struct ShellsChip: View {
    let shells: [BackgroundShell]
    let finished: [String: String]
    let open: () -> Void
    @State private var running = 0

    var body: some View {
        Group {
            if running > 0 {
                Button(action: open) {
                    HStack(spacing: 4) {
                        Image(systemName: "terminal").font(.system(size: 10, weight: .semibold))
                        Text(running == 1 ? "1 shell" : "\(running) shells").monospacedDigit()
                    }
                    .kitType(.meta).foregroundColor(KitInk.working)
                    .padding(.horizontal, 8).frame(height: 22)
                    .overlay(Capsule().strokeBorder(KitInk.hairline, lineWidth: 1))
                    .contentShape(Capsule())
                }
                .buttonStyle(.kitPlain)
                .help("Background shells still running. Click to see them, read their output or stop them.")
                .part("Shells Chip", .bottom)
            }
        }
        .task(id: shells.map(\.id) + finished.keys.sorted()) {
            // Only shells with no end notification can be running; most sessions have none.
            let open = shells.filter { finished[$0.id] == nil }
            guard !open.isEmpty else { running = 0; return }
            while !Task.isCancelled {
                let count = await Task.detached(priority: .utility) {
                    open.filter { !BackgroundShells.writers(of: $0.outputPath).isEmpty }.count
                }.value
                if count != running { running = count }
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
    }
}
