import AppKit

/// One account in the menu: email and plan, then a bar per limit with when it resets. Drawn by hand
/// so the bars line up; it highlights and clicks like a normal item.
final class AccountRowView: NSView {
    var account: ClaudeAccount { didSet { needsDisplay = true } }
    var usage: AccountUsage? { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var isActive: Bool { didSet { needsDisplay = true } }
    var onClick: (() -> Void)?

    static let width: CGFloat = 330

    init(account: ClaudeAccount, usage: AccountUsage?, isActive: Bool) {
        self.account = account
        self.usage = usage
        self.isActive = isActive
        super.init(frame: NSRect(x: 0, y: 0, width: Self.width, height: 0))
        setFrameSize(NSSize(width: Self.width, height: height))
        autoresizingMask = .width
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    private var lines: [(label: String, window: UsageWindow)] {
        guard let usage else { return [] }
        var out: [(String, UsageWindow)] = []
        if let s = usage.session { out.append(("5h", s)) }
        if let w = usage.week { out.append(("Week", w)) }
        out += usage.models.map { ($0.name, $0.window) }
        return out
    }

    private var height: CGFloat { 26 + CGFloat(max(lines.count, 1)) * 17 + 6 }

    func relayout() { setFrameSize(NSSize(width: frame.width, height: height)) }

    override func draw(_ dirtyRect: NSRect) {
        let lit = enclosingMenuItem?.isHighlighted == true
        if lit {
            let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 5, dy: 1), xRadius: 5, yRadius: 5)
            NSColor.selectedContentBackgroundColor.setFill()
            path.fill()
        }
        let primary: NSColor = lit ? .white : .labelColor
        let secondary: NSColor = lit ? NSColor.white.withAlphaComponent(0.8) : .secondaryLabelColor
        let right = bounds.width - 14

        if isActive {
            text("✓", x: 10, y: 5, font: .systemFont(ofSize: 13, weight: .semibold), color: primary)
        }
        let plan = attributed(account.plan, font: .systemFont(ofSize: 11), color: secondary)
        plan.draw(at: NSPoint(x: right - plan.size().width, y: 7))
        let email = attributed(account.email, font: .systemFont(ofSize: 13, weight: .medium), color: primary)
        email.draw(with: NSRect(x: 26, y: 5, width: right - plan.size().width - 34, height: 18),
                   options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])

        var y: CGFloat = 26
        guard !account.needsLogin else {
            text("Signed out. Use Add Account to sign in again.", x: 26, y: y,
                 font: .systemFont(ofSize: 11), color: lit ? .white : .systemOrange)
            return
        }
        guard !lines.isEmpty else {
            text(usage == nil ? "Checking limits…" : "No limits reported", x: 26, y: y, font: .systemFont(ofSize: 11), color: secondary)
            return
        }
        let small = NSFont.systemFont(ofSize: 11)
        let digits = NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium)
        for (label, window) in lines {
            text(label, x: 26, y: y, font: small, color: secondary)
            let bar = NSRect(x: 72, y: y + 5, width: 110, height: 6)
            (lit ? NSColor.white.withAlphaComponent(0.3) : NSColor.quaternaryLabelColor).setFill()
            NSBezierPath(roundedRect: bar, xRadius: 3, yRadius: 3).fill()
            let fraction = min(max(window.percent / 100, 0), 1)
            if fraction > 0 {
                var fill = bar
                fill.size.width = max(bar.height, bar.width * fraction)
                (lit ? NSColor.white : Self.color(window.percent)).setFill()
                NSBezierPath(roundedRect: fill, xRadius: 3, yRadius: 3).fill()
            }
            text("\(Int(window.percent.rounded()))%", x: 190, y: y, font: digits, color: primary)
            if let reset = window.resetsAt {
                let r = attributed(Self.resetText(reset, short: label == "5h"), font: small, color: secondary)
                r.draw(at: NSPoint(x: right - r.size().width, y: y))
            }
            y += 17
        }
    }

    static func color(_ percent: Double) -> NSColor {
        percent >= 90 ? .systemRed : percent >= 70 ? .systemOrange : .systemGreen
    }

    /// "in 1h 20m" for the 5-hour window, "Tue 13:00" for the week.
    static func resetText(_ date: Date, short: Bool) -> String {
        let left = date.timeIntervalSinceNow
        if left <= 0 { return "resetting" }
        if short || left < 6 * 3600 {
            let minutes = Int(left / 60)
            return minutes >= 60 ? "in \(minutes / 60)h \(minutes % 60)m" : "in \(max(minutes, 1))m"
        }
        let f = DateFormatter()
        f.setLocalizedDateFormatFromTemplate("EEE jmm")
        return f.string(from: date)
    }

    private func attributed(_ s: String, font: NSFont, color: NSColor) -> NSAttributedString {
        NSAttributedString(string: s, attributes: [.font: font, .foregroundColor: color])
    }

    private func text(_ s: String, x: CGFloat, y: CGFloat, font: NSFont, color: NSColor) {
        attributed(s, font: font, color: color).draw(at: NSPoint(x: x, y: y))
    }

    // MARK: Mouse

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                       owner: self))
    }

    override func mouseEntered(with event: NSEvent) { needsDisplay = true }
    override func mouseExited(with event: NSEvent) { needsDisplay = true }

    override func mouseUp(with event: NSEvent) {
        enclosingMenuItem?.menu?.cancelTracking()
        onClick?()
    }
}

/// Antigravity's account in the menu, drawn like the Claude row: email and plan, then each model
/// group's name followed by its weekly and five-hour bars.
final class AntigravityRowView: NSView {
    var account: AntigravityAccount { didSet { needsDisplay = true } }
    var usage: AntigravityUsage? { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var onClick: (() -> Void)?

    static let width: CGFloat = 330

    init(account: AntigravityAccount, usage: AntigravityUsage?) {
        self.account = account
        self.usage = usage
        super.init(frame: NSRect(x: 0, y: 0, width: Self.width, height: 0))
        setFrameSize(NSSize(width: Self.width, height: height))
        autoresizingMask = .width
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    /// Each row is a group header or a bar, so they stack like the Claude limits.
    private enum Line { case header(String); case bar(String, UsageWindow) }

    private var lines: [Line] {
        guard let usage else { return [] }
        return usage.groups.flatMap { group -> [Line] in
            [.header(group.name)] + group.bars.map { .bar($0.label, $0.window) }
        }
    }

    private var height: CGFloat { 26 + CGFloat(max(lines.count, 1)) * 17 + 6 }

    func relayout() { setFrameSize(NSSize(width: frame.width, height: height)) }

    override func draw(_ dirtyRect: NSRect) {
        let lit = enclosingMenuItem?.isHighlighted == true
        if lit {
            let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 5, dy: 1), xRadius: 5, yRadius: 5)
            NSColor.selectedContentBackgroundColor.setFill()
            path.fill()
        }
        let primary: NSColor = lit ? .white : .labelColor
        let secondary: NSColor = lit ? NSColor.white.withAlphaComponent(0.8) : .secondaryLabelColor
        let right = bounds.width - 14

        text("✓", x: 10, y: 5, font: .systemFont(ofSize: 13, weight: .semibold), color: primary)
        let plan = attributed(account.plan ?? "Google", font: .systemFont(ofSize: 11), color: secondary)
        plan.draw(at: NSPoint(x: right - plan.size().width, y: 7))
        let email = attributed(account.email, font: .systemFont(ofSize: 13, weight: .medium), color: primary)
        email.draw(with: NSRect(x: 26, y: 5, width: right - plan.size().width - 34, height: 18),
                   options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])

        var y: CGFloat = 26
        guard !lines.isEmpty else {
            text("Open Antigravity to read its limits.", x: 26, y: y, font: .systemFont(ofSize: 11), color: secondary)
            return
        }
        let small = NSFont.systemFont(ofSize: 11)
        let digits = NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium)
        for line in lines {
            switch line {
            case .header(let name):
                text(name, x: 26, y: y, font: .systemFont(ofSize: 11, weight: .semibold), color: secondary)
            case .bar(let label, let window):
                text(label, x: 38, y: y, font: small, color: secondary)
                let bar = NSRect(x: 84, y: y + 5, width: 98, height: 6)
                (lit ? NSColor.white.withAlphaComponent(0.3) : NSColor.quaternaryLabelColor).setFill()
                NSBezierPath(roundedRect: bar, xRadius: 3, yRadius: 3).fill()
                let fraction = min(max(window.percent / 100, 0), 1)
                if fraction > 0 {
                    var fill = bar
                    fill.size.width = max(bar.height, bar.width * fraction)
                    (lit ? NSColor.white : AccountRowView.color(window.percent)).setFill()
                    NSBezierPath(roundedRect: fill, xRadius: 3, yRadius: 3).fill()
                }
                text("\(Int(window.percent.rounded()))%", x: 190, y: y, font: digits, color: primary)
                if let reset = window.resetsAt {
                    let r = attributed(AccountRowView.resetText(reset, short: label == "5h"), font: small, color: secondary)
                    r.draw(at: NSPoint(x: right - r.size().width, y: y))
                }
            }
            y += 17
        }
    }

    private func attributed(_ s: String, font: NSFont, color: NSColor) -> NSAttributedString {
        NSAttributedString(string: s, attributes: [.font: font, .foregroundColor: color])
    }

    private func text(_ s: String, x: CGFloat, y: CGFloat, font: NSFont, color: NSColor) {
        attributed(s, font: font, color: color).draw(at: NSPoint(x: x, y: y))
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                       owner: self))
    }

    override func mouseEntered(with event: NSEvent) { needsDisplay = true }
    override func mouseExited(with event: NSEvent) { needsDisplay = true }

    override func mouseUp(with event: NSEvent) {
        enclosingMenuItem?.menu?.cancelTracking()
        onClick?()
    }
}

/// Fourteen days of cost as bars, with today, 7-day and 30-day totals above.
final class CostChartView: NSView {
    var summary: CostTracker.Summary { didSet { needsDisplay = true } }

    init(summary: CostTracker.Summary) {
        self.summary = summary
        super.init(frame: NSRect(x: 0, y: 0, width: 300, height: 150))
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        let left: CGFloat = 16, width = bounds.width - 32
        let stats = [("Today", summary.today), ("7 Days", summary.week), ("30 Days", summary.month)]
        for (i, stat) in stats.enumerated() {
            let x = left + CGFloat(i) * width / 3
            draw(stat.0, at: NSPoint(x: x, y: 6), font: .systemFont(ofSize: 11), color: .secondaryLabelColor)
            draw(Self.dollars(stat.1), at: NSPoint(x: x, y: 20), font: .systemFont(ofSize: 16, weight: .semibold), color: .labelColor)
        }

        let days = summary.days
        guard !days.isEmpty else { return }
        let top: CGFloat = 54, chartHeight: CGFloat = 66
        let peak = max(days.map(\.cost).max() ?? 0, 0.01)
        draw(Self.dollars(peak), at: NSPoint(x: left, y: top - 4), font: .systemFont(ofSize: 9), color: .tertiaryLabelColor)
        let slot = width / CGFloat(days.count), barWidth = slot * 0.62
        let letter = DateFormatter()
        letter.setLocalizedDateFormatFromTemplate("EEEEE")
        for (i, day) in days.enumerated() {
            let h = max(CGFloat(day.cost / peak) * (chartHeight - 10), day.cost > 0 ? 2 : 1)
            let x = left + CGFloat(i) * slot + (slot - barWidth) / 2
            let rect = NSRect(x: x, y: top + chartHeight - h, width: barWidth, height: h)
            (i == days.count - 1 ? NSColor.controlAccentColor : NSColor.secondaryLabelColor.withAlphaComponent(0.45)).setFill()
            NSBezierPath(roundedRect: rect, xRadius: 2, yRadius: 2).fill()
            let label = NSAttributedString(string: letter.string(from: day.day),
                                           attributes: [.font: NSFont.systemFont(ofSize: 9), .foregroundColor: NSColor.tertiaryLabelColor])
            label.draw(at: NSPoint(x: x + (barWidth - label.size().width) / 2, y: top + chartHeight + 3))
        }
    }

    static func dollars(_ value: Double) -> String {
        value >= 1000 ? String(format: "$%.1fk", value / 1000) : value >= 100 ? String(format: "$%.0f", value) : String(format: "$%.2f", value)
    }

    private func draw(_ s: String, at point: NSPoint, font: NSFont, color: NSColor) {
        NSAttributedString(string: s, attributes: [.font: font, .foregroundColor: color]).draw(at: point)
    }
}
