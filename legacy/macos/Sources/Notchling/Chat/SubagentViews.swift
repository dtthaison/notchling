import SwiftUI

enum SubagentFormat {
    /// "Running 3m · sonnet high · Explore" or "Done 2h ago · opus": the status first, since a narrow
    /// panel cuts the end; the usual general-purpose type is left out.
    static func line(_ sub: Subagent, now: Date = Date()) -> String {
        let model = [sub.model, sub.effort].compactMap { $0 }.joined(separator: " ")
        let when: String
        switch sub.status {
        case .running: when = "Running " + span(now.timeIntervalSince(sub.started))
        case .completed: when = "Done " + ago(sub.updated, now: now)
        case .failed: when = "Failed " + ago(sub.updated, now: now)
        case .stopped: when = "Stopped " + ago(sub.updated, now: now)
        case .quiet: when = "Last active " + ago(sub.updated, now: now)
        }
        let type = sub.type == "general-purpose" ? nil : sub.type
        return [when, model.isEmpty ? nil : model, type].compactMap { $0 }.joined(separator: " · ")
    }

    static func ago(_ date: Date, now: Date) -> String {
        let s = now.timeIntervalSince(date)
        return s < 60 ? "just now" : span(s) + " ago"
    }

    static func span(_ seconds: TimeInterval) -> String {
        let s = max(0, Int(seconds))
        if s < 60 { return "\(s)s" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86400 { return "\(s / 3600)h \(s % 3600 / 60)m" }
        return "\(s / 86400)d"
    }

    static func statusWord(_ status: Subagent.Status) -> String {
        switch status {
        case .running: return "Running"
        case .completed: return "Done"
        case .failed: return "Failed"
        case .stopped: return "Stopped"
        case .quiet: return "Not running"
        }
    }
}

struct SubagentStatusIcon: View {
    let status: Subagent.Status

    var body: some View {
        switch status {
        case .running: ProgressView().controlSize(.mini)
        case .completed: Image(systemName: "checkmark.circle.fill").font(.system(size: 11)).foregroundColor(KitInk.done)
        case .failed: Image(systemName: "xmark.octagon.fill").font(.system(size: 11)).foregroundColor(.red)
        case .stopped: Image(systemName: "stop.circle").font(.system(size: 11)).foregroundColor(KitInk.textFaint)
        case .quiet: Image(systemName: "moon.zzz").font(.system(size: 10)).foregroundColor(KitInk.textFaint)
        }
    }
}

/// Above a subagent's conversation: which one it is, and the way back to its session. Read-only:
/// what it was told is its first message, what it reported back is its last.
struct SubagentBanner: View {
    let subagent: Subagent
    let back: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: back) {
                HStack(spacing: 4) {
                    Image(systemName: "chevron.left").font(.system(size: 10, weight: .semibold))
                    Text("Session")
                }
            }
            .buttonStyle(.kit(.ghost))
            .keyboardShortcut("[", modifiers: .command)
            .help("Back to the session's conversation (⌘[ or Esc)")
            .part("Back to Session Button", .bottom)
            SubagentStatusIcon(status: subagent.status).frame(width: 14)
            VStack(alignment: .leading, spacing: 1) {
                Text(subagent.description).kitType(.label).foregroundColor(KitInk.text).lineLimit(1)
                TimelineView(.periodic(from: .now, by: 30)) { context in
                    Text("Subagent · " + SubagentFormat.line(subagent, now: context.date))
                        .kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(KitInk.raised.opacity(0.5))
        .overlay(Rectangle().fill(KitInk.hairline).frame(height: 1), alignment: .bottom)
    }
}
