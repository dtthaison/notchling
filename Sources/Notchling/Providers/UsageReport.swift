import Foundation

/// What every provider reports about its limits, whatever its API calls them: groups of bars, each
/// bar a used percentage with a reset time and a kind. Cards and menu rows read this, never a
/// provider's own type.
struct UsageReport: Equatable {
    enum Kind: String, Equatable {
        /// The short window: Claude's 5 hours, Codex's and Antigravity's "5h".
        case session
        /// A weekly window, account-wide or for one model.
        case week
        /// Anything else (a month, a limit the app doesn't know yet).
        case other
    }

    struct Bar: Equatable {
        var label: String
        var kind: Kind
        var percent: Double
        var resetsAt: Date?

        var window: UsageWindow { UsageWindow(percent: percent, resetsAt: resetsAt) }
    }

    struct Group: Equatable {
        /// Shown over the bars; nil for a provider with one unnamed set (Claude).
        var name: String?
        var bars: [Bar]
    }

    var groups: [Group]
    var fetched = Date()

    var bars: [Bar] { groups.flatMap(\.bars) }
    var isEmpty: Bool { bars.isEmpty }
}

extension UsageReport {
    /// Claude: one unnamed group, the 5-hour and week bars first, then the limits the app doesn't know
    /// (which stop the account like the two above), then the per-model weekly ones.
    init(_ usage: AccountUsage) {
        var bars: [Bar] = []
        if let s = usage.session { bars.append(Bar(label: "5 hours", kind: .session, percent: s.percent, resetsAt: s.resetsAt)) }
        if let w = usage.week { bars.append(Bar(label: "Week", kind: .week, percent: w.percent, resetsAt: w.resetsAt)) }
        for other in usage.other {
            bars.append(Bar(label: other.name, kind: .other, percent: other.window.percent, resetsAt: other.window.resetsAt))
        }
        for model in usage.models {
            bars.append(Bar(label: model.name, kind: .week, percent: model.window.percent, resetsAt: model.window.resetsAt))
        }
        self.init(groups: [Group(name: nil, bars: bars)], fetched: usage.fetched)
    }

    /// Codex and Antigravity: named groups, each bar labelled "5h", "Week" or the window's length.
    init(_ usage: GroupedUsage) {
        self.init(groups: usage.groups.map { group in
            Group(name: group.name, bars: group.bars.map { bar in
                Bar(label: bar.label, kind: Self.kind(ofLabel: bar.label), percent: bar.window.percent,
                    resetsAt: bar.window.resetsAt)
            })
        }, fetched: usage.fetched)
    }

    static func kind(ofLabel label: String) -> Kind {
        switch label {
        case "5h": return .session
        case "Week": return .week
        default: return .other
        }
    }
}
