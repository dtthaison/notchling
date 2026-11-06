import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// The session's account in its header, with a menu to move it to another account or start a new
/// terminal session on one. A lane moves as a whole; so does the main login. The menu is AppKit's: on
/// macOS 13 a SwiftUI borderless Menu drops a custom label.
/// How full the session's context is and how much of its account's limits is used: what /context and
/// /usage would say, without typing them.
/// One limit bar in the session header.
struct SessionLimit {
    let label: String
    let name: String
    let window: UsageWindow

    static func name(_ label: String) -> String {
        switch label {
        case "5h": return "5-hour limit"
        case "Week": return "Weekly limit"
        default: return "\(label) limit"
        }
    }
}

struct SessionMeters: View {
    let context: Int
    let window: Int?
    let limits: [SessionLimit]

    var body: some View {
        // Top-aligned so the bars line up when only the limits have a reset line under them.
        HStack(alignment: .top, spacing: 14) {
            if context > 0 {
                let fraction = window.map { Double(context) / Double($0) }
                meter("Context", width: 120,
                      window.map { "\(ChatFormat.tokens(context)) / \(ChatFormat.tokens($0))" } ?? ChatFormat.tokens(context),
                      fraction,
                      help: "Context: \(context.formatted()) tokens"
                        + (window.map { " of \($0.formatted()) (\(Int((fraction ?? 0) * 100))%). Claude Code compacts the conversation near the limit." } ?? "."))
                    .part("Context Meter", .bottom)
            }
            if !limits.isEmpty {
                // Redrawn each minute so "in 2h 13m" counts down without a new usage fetch.
                TimelineView(.everyMinute) { _ in
                    HStack(alignment: .top, spacing: 10) {
                        ForEach(limits, id: \.label) { l in
                            limit(l.label, l.window, short: l.label.hasSuffix("h"), name: l.name)
                        }
                    }
                }
                .part("Limit Meters", .bottom)
            }
        }
    }

    private func limit(_ label: String, _ window: UsageWindow, short: Bool, name: String) -> some View {
        let reset = window.resetsAt.map { AccountRowView.resetText($0, short: short) }
        return meter(label, width: 64, "\(Int(window.percent.rounded()))%", window.percent / 100, below: reset,
                     help: "\(name): \(Int(window.percent.rounded()))% used" + (reset.map { ", resets \($0)" } ?? ""))
    }

    private func meter(_ label: String, width: CGFloat, _ value: String, _ fraction: Double?, below: String? = nil,
                       help: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Text(label).kitType(.meta).foregroundColor(KitInk.textMuted)
                Text(value).kitType(.label).foregroundColor(KitInk.text).monospacedDigit()
            }
            .lineLimit(1).fixedSize()
            if let fraction {
                let f = min(max(fraction, 0), 1)
                Capsule().fill(KitInk.raised)
                    .overlay(alignment: .leading) {
                        GeometryReader { g in
                            Capsule().fill(f >= 0.8 ? KitInk.needs : KitInk.working).frame(width: max(g.size.width * f, 3))
                        }
                    }
                    .frame(height: 3)
            }
            if let below {
                Text(below).kitType(.meta).foregroundColor(KitInk.textFaint).monospacedDigit()
                    .lineLimit(1).fixedSize()
            }
        }
        .frame(minWidth: width, alignment: .leading)
        .fixedSize()
        .help(help)
        .accessibilityElement(children: .combine)
    }
}
