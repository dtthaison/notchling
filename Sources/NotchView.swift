import SwiftUI

/// Black shape that continues the camera housing: small outward flares where it meets the
/// top of the screen, rounded corners at the bottom.
struct NotchShape: Shape {
    var radius: CGFloat
    var flare: CGFloat = NotchModel.flare

    var animatableData: CGFloat {
        get { radius }
        set { radius = newValue }
    }

    func path(in r: CGRect) -> Path {
        let f = flare
        let radius = min(self.radius, (r.height - f) / 2, (r.width - 2 * f) / 2)
        var p = Path()
        p.move(to: CGPoint(x: r.minX, y: r.minY))
        p.addQuadCurve(to: CGPoint(x: r.minX + f, y: r.minY + f), control: CGPoint(x: r.minX + f, y: r.minY))
        p.addLine(to: CGPoint(x: r.minX + f, y: r.maxY - radius))
        p.addQuadCurve(to: CGPoint(x: r.minX + f + radius, y: r.maxY), control: CGPoint(x: r.minX + f, y: r.maxY))
        p.addLine(to: CGPoint(x: r.maxX - f - radius, y: r.maxY))
        p.addQuadCurve(to: CGPoint(x: r.maxX - f, y: r.maxY - radius), control: CGPoint(x: r.maxX - f, y: r.maxY))
        p.addLine(to: CGPoint(x: r.maxX - f, y: r.minY + f))
        p.addQuadCurve(to: CGPoint(x: r.maxX, y: r.minY), control: CGPoint(x: r.maxX - f, y: r.minY))
        p.closeSubpath()
        return p
    }
}

extension Color {
    static let claude = Color(red: 0.85, green: 0.47, blue: 0.34)
    static let asks = Color(red: 0.36, green: 0.74, blue: 0.86)
    static let optionFill = Color(red: 0.06, green: 0.12, blue: 0.14)
    static let optionBadge = Color(red: 0.13, green: 0.27, blue: 0.30)
}

extension AgentGroup {
    /// Short enough for three tabs to fit under the camera.
    var shortTitle: String {
        switch self {
        case .needsInput: return "Input"
        case .working: return "Working"
        case .completed: return "Done"
        }
    }

    var color: Color {
        switch self {
        case .needsInput: return .orange
        case .working: return .blue
        case .completed: return .green
        }
    }
}

extension Agent {
    var color: Color {
        switch (group, label) {
        case (.needsInput, _): return .orange
        case (.working, _): return .blue
        case (_, "Done"): return .green
        case (_, "Failed"), (_, "Failure"), (_, "Error"): return .red
        default: return .gray
        }
    }

    /// Fits a narrow row: "bg", "VS Code", "-p".
    var shortHost: String {
        switch host {
        case "Background": return "bg"
        case "claude -p": return "-p"
        default: return host
        }
    }

    var ago: String {
        let s = max(0, Int(Date().timeIntervalSince(since)))
        if s < 60 { return "now" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86400 { return "\(s / 3600)h" }
        return "\(s / 86400)d"
    }
}

private func oneLine(_ s: String) -> String {
    s.replacingOccurrences(of: "\n", with: " ").trimmingCharacters(in: .whitespaces)
}

struct NotchView: View {
    @ObservedObject var model: NotchModel

    var body: some View {
        let size = model.size(for: model.state)
        VStack(spacing: 0) {
            topBar.frame(height: model.notchSize.height)
            Group {
                switch model.state {
                case .collapsed: EmptyView()
                case .peek(let id): if let agent = model.agent(id) { PeekView(model: model, agent: agent) }
                case .list: AgentListView(model: model)
                case .detail(let id): if let agent = model.agent(id) { DetailView(model: model, agent: agent) }
                }
            }
            // Content grows out of the notch as the shape opens, and fades quickly as it closes.
            .transition(.asymmetric(
                insertion: .scale(scale: 0.6, anchor: .top).combined(with: .opacity)
                    .animation(.spring(response: 0.36, dampingFraction: 0.9).delay(0.05)),
                removal: .opacity.animation(.easeIn(duration: 0.12))))
        }
        .padding(.horizontal, NotchModel.flare)
        .frame(width: size.width, height: size.height, alignment: .top)
        .background(Color.black)
        .clipShape(NotchShape(radius: model.state == .collapsed ? 10 : 24))
        // The pet lives outside the black, so it sits over the shape rather than inside it.
        .overlay(alignment: .topLeading) {
            if model.needsCount + model.workingCount > 0 {
                PetView(model: model, shape: size)
            }
        }
        // The resting strip grows a lane for the pet's walks, and folds it away again.
        .animation(.spring(response: 0.4, dampingFraction: 0.9), value: size)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .preferredColorScheme(.dark)
    }

    /// The number of active sessions to the right of the camera. When the list opens it slides in
    /// under the camera with the pet, and the tabs grow out of that spot.
    private var topBar: some View {
        let active = model.needsCount + model.workingCount
        let gathered = model.state == .list
        // From each wing to the middle of the tab row just under the camera.
        let across = model.notchSize.width / 2 + NotchModel.wingWidth / 2 - 9
        let down = model.notchSize.height / 2 + TabBar.height / 2

        return HStack(spacing: 0) {
            Color.clear.frame(width: NotchModel.wingWidth)
            Spacer(minLength: model.notchSize.width)
            Text(active > 0 ? "\(active)" : "")
                .font(.system(size: 12.5, weight: .semibold, design: .rounded))
                .foregroundColor(model.needsCount > 0 ? .orange : .white)
                .frame(width: NotchModel.wingWidth)
                .offset(x: gathered ? -across : 0, y: gathered ? down : 0)
                .animation(.spring(response: 0.3, dampingFraction: 0.85), value: gathered)
        }
        .frame(width: model.notchSize.width + 2 * NotchModel.wingWidth)
        // Fade only once they've met in the middle; reappear straight away on the way back.
        .opacity(gathered ? 0 : 1)
        .animation(gathered ? .easeOut(duration: 0.12).delay(0.24) : .easeIn(duration: 0.08), value: gathered)
    }
}

/// Claude's ✻ as a little pet that rolls around the outside of the camera: down its left edge,
/// along underneath, up its right edge and back, spinning like a wheel. While sessions need you it
/// tows an orange badge with their number; at each end the badge swings round the outside of the
/// pet to trail it the other way. When the tabs open it rolls into the middle and fades.
struct PetView: View {
    @ObservedObject var model: NotchModel
    /// Size of the black shape this frame; the pet's path follows its outline.
    let shape: CGSize

    static let radius: CGFloat = 8
    /// Highest point of the path, just below the top of the screen.
    static let top: CGFloat = 10
    /// How far behind the pet the badge trails, measured along the path.
    static let rope: Double = 21

    /// Where the pet is along the path, which way it's heading, and how far through turning round.
    fileprivate struct Motion {
        var d = rope
        var heading = 1.0
        var turn: (progress: Double, from: Double)? = nil
    }

    /// Keeps the pet's place between frames, so it carries on from where it stopped rather than
    /// jumping when it starts moving again or changes speed.
    fileprivate final class Walk: ObservableObject {
        var motion = Motion()
        var last: Double?
    }

    @StateObject private var walk = Walk()

    var body: some View {
        let gathered = model.state == .list
        let resting = model.state == .collapsed
        let rolling = model.petWalking && resting
        let needs = model.needsCount
        TimelineView(.animation(minimumInterval: 1.0 / 30, paused: !rolling)) { context in
            let path = Track(shape: shape)
            let motion = step(to: context.date.timeIntervalSinceReferenceDate, rolling: rolling, path)
            let d = motion.d
            let pet = gathered ? middle : path.point(d)
            let badge = badgePoint(motion, path)

            ZStack(alignment: .topLeading) {
                if needs > 0 && !gathered {
                    Text("\(needs)")
                        .font(.system(size: 10, weight: .bold, design: .rounded))
                        .foregroundColor(.white)
                        .padding(.horizontal, 4)
                        .frame(minWidth: 15, minHeight: 15)
                        .background(Capsule().fill(Color.orange))
                        .shadow(color: .black.opacity(0.35), radius: 1.5)
                        .fixedSize()
                        .frame(width: 30, height: 16)
                        .offset(x: badge.x - 15, y: badge.y - 8)
                }
                Text("✻")
                    .font(.system(size: 15, weight: .bold))
                    .foregroundColor(.claude)
                    .shadow(color: .black.opacity(0.45), radius: 1.5)
                    .frame(width: Self.radius * 2, height: Self.radius * 2)
                    .rotationEffect(.radians(gathered ? 0 : d / Double(Self.radius)))
                    .offset(x: pet.x - Self.radius, y: pet.y - Self.radius)
            }
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.85), value: gathered)
        // Only out on its own while the notch rests; it fades once it reaches the tabs.
        .opacity(resting ? 1 : 0)
        .animation(resting ? .easeIn(duration: 0.15).delay(0.15) : .easeOut(duration: 0.12).delay(gathered ? 0.24 : 0),
                   value: resting)
    }

    private var middle: CGPoint {
        CGPoint(x: shape.width / 2, y: model.notchSize.height + TabBar.height / 2)
    }

    /// The pet ping-pongs between a rope's length from each top end, resting at each end while it
    /// turns round, so the badge always has room behind it. When it isn't rolling it stays put.
    private func step(to t: Double, rolling: Bool, _ path: Track) -> Motion {
        let lo = Self.rope, hi = max(lo, path.length - Self.rope)
        var m = walk.motion
        m.d = min(max(m.d, lo), hi)
        defer { walk.motion = m }
        guard rolling, let last = walk.last else {
            walk.last = rolling ? t : nil
            return m
        }
        walk.last = t
        // A long gap (a stalled frame) moves it one frame's worth, never a leap.
        let dt = min(max(t - last, 0), 0.1)
        let speed = model.needsCount > 0 ? 60.0 : 36.0, rest = 0.7

        if let turn = m.turn {
            let progress = turn.progress + dt / rest
            m.turn = progress >= 1 ? nil : (progress, turn.from)
            return m
        }
        m.d += m.heading * speed * dt
        if m.d >= hi {
            m.d = hi; m.heading = -1; m.turn = (0, 1)
        } else if m.d <= lo {
            m.d = lo; m.heading = 1; m.turn = (0, -1)
        }
        return m
    }

    /// A rope's length behind the pet along the path. While turning, it swings round the outside of
    /// the pet (away from the camera) from the old trailing side to the new one, never across it.
    private func badgePoint(_ m: Motion, _ path: Track) -> CGPoint {
        guard let turn = m.turn else { return path.point(m.d - m.heading * Self.rope) }
        let pet = path.point(m.d)
        let from = path.point(m.d - turn.from * Self.rope)
        let to = path.point(m.d - m.heading * Self.rope)
        let n = path.outward(at: m.d)
        let eased = (1 - cos(turn.progress * .pi)) / 2

        // Sweep round the pet at a steady distance, the way that passes outside (away from the camera).
        let start = atan2(from.y - pet.y, from.x - pet.x), end = atan2(to.y - pet.y, to.x - pet.x)
        var sweep = (end - start).truncatingRemainder(dividingBy: 2 * .pi)
        if sweep < 0 { sweep += 2 * .pi }
        let outwardAngle = atan2(n.y, n.x)
        func offAngle(_ a: Double) -> Double { abs(atan2(sin(a - outwardAngle), cos(a - outwardAngle))) }
        if offAngle(start + sweep / 2) > offAngle(start + (sweep - 2 * .pi) / 2) { sweep -= 2 * .pi }

        let angle = start + eased * sweep
        let radiusFrom = hypot(from.x - pet.x, from.y - pet.y), radiusTo = hypot(to.x - pet.x, to.y - pet.y)
        let radius = radiusFrom + eased * (radiusTo - radiusFrom)
        return CGPoint(x: pet.x + cos(angle) * radius, y: pet.y + sin(angle) * radius)
    }

    /// The outline of the camera pushed out by the pet's radius: down the left edge, round the
    /// bottom-left corner, along underneath, round the bottom-right corner and up the right edge.
    private struct Track {
        let left, right, bottom, top, out: Double
        let corner = 10.0
        let side, arc, floor: Double
        var length: Double { 2 * side + 2 * arc + floor }

        init(shape: CGSize) {
            left = Double(NotchModel.flare)
            right = Double(shape.width - NotchModel.flare)
            bottom = Double(shape.height)
            top = Double(PetView.top)
            out = Double(PetView.radius) + 2
            side = max(0, bottom - corner - top)
            arc = Double.pi / 2 * (corner + out)
            floor = max(0, right - left - 2 * corner)
        }

        /// Unit vector pointing away from the camera at distance `d`.
        func outward(at d: Double) -> CGPoint {
            let a = point(d - 1), b = point(d + 1)
            let tx = b.x - a.x, ty = b.y - a.y
            let len = max(0.001, (tx * tx + ty * ty).squareRoot())
            return CGPoint(x: -ty / len, y: tx / len)
        }

        func point(_ d: Double) -> CGPoint {
            let r = corner + out
            if d < side {
                return CGPoint(x: left - out, y: top + d)
            } else if d < side + arc {
                let a = Double.pi - (d - side) / r                  // 180° → 90°; screen y points down
                return CGPoint(x: left + corner + cos(a) * r, y: bottom - corner + sin(a) * r)
            } else if d < side + arc + floor {
                return CGPoint(x: left + corner + (d - side - arc), y: bottom + out)
            } else if d < side + 2 * arc + floor {
                let a = Double.pi / 2 - (d - side - arc - floor) / r  // 90° → 0°
                return CGPoint(x: right - corner + cos(a) * r, y: bottom - corner + sin(a) * r)
            } else {
                return CGPoint(x: right + out, y: bottom - corner - (d - side - 2 * arc - floor))
            }
        }
    }
}

struct Tag: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .medium))
            .foregroundColor(.white.opacity(0.6))
            .lineLimit(1)
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.09)))
    }
}

// MARK: - Tabs and rows

/// The three tabs that sit right under the camera.
struct TabBar: View {
    @ObservedObject var model: NotchModel
    @State private var shown = false

    static let height: CGFloat = 34

    var body: some View {
        HStack(spacing: 4) {
            ForEach(AgentGroup.allCases, id: \.self) { group in
                let count = model.agents.filter { $0.group == group }.count
                let selected = model.tab == group
                Button { model.tab = group } label: {
                    HStack(spacing: 4) {
                        Circle().fill(group.color).frame(width: 5, height: 5)
                        Text(group.shortTitle).font(.system(size: 10.5, weight: .semibold))
                        Text("\(count)").font(.system(size: 10.5, weight: .bold, design: .rounded))
                            .foregroundColor(count > 0 ? group.color : .secondary)
                    }
                    .foregroundColor(selected ? .white : .secondary)
                    .padding(.horizontal, 4)
                    .frame(maxWidth: .infinity, minHeight: 26)
                    .background(Capsule().fill(Color.white.opacity(selected ? 0.14 : 0)))
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 4)
        .frame(height: Self.height)
        // Grow out of the middle once the mark and count have slid in under the camera.
        .scaleEffect(x: shown ? 1 : 0.15, y: shown ? 1 : 0.6, anchor: .center)
        .opacity(shown ? 1 : 0)
        .onAppear {
            withAnimation(.spring(response: 0.34, dampingFraction: 0.86).delay(0.22)) { shown = true }
        }
    }
}

struct AgentRow: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    var trailing: String?
    @State private var hovered = false

    var body: some View {
        let snapshot = model.snapshot(agent.id)
        HStack(alignment: .top, spacing: 9) {
            Circle().fill(agent.color).frame(width: 8, height: 8)
                .frame(width: 10, height: 18)

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(agent.name).font(.system(size: 12.5, weight: .medium)).lineLimit(1)
                    Spacer(minLength: 4)
                    Text("\(agent.shortHost) · \(trailing ?? agent.ago)")
                        .font(.system(size: 10.5)).foregroundColor(.secondary).lineLimit(1)
                        .fixedSize()
                }
                if agent.group == .completed {
                    if let text = snapshot.lastReply ?? agent.detail {
                        Text(oneLine(text)).font(.system(size: 12)).foregroundColor(.secondary).lineLimit(1)
                    }
                } else {
                    if let prompt = snapshot.lastPrompt {
                        Text("You: " + oneLine(prompt))
                            .font(.system(size: 12)).foregroundColor(.secondary).lineLimit(1)
                    }
                    Text(oneLine(statusLine(snapshot)))
                        .font(.system(size: 12)).foregroundColor(agent.color).lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .frame(height: model.rowHeight(agent), alignment: .top)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hovered ? 0.08 : 0)))
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
    }

    private func statusLine(_ snapshot: Snapshot) -> String {
        if let r = model.permission(agent.id) { return "Wants to use \(r.toolName): \(r.summary)" }
        if let q = model.question(agent.id), let first = q.items.first { return "Asks: \(first.question)" }
        if agent.group == .needsInput { return agent.detail ?? snapshot.lastReply ?? agent.label }
        return snapshot.activity ?? agent.detail ?? "Thinking…"
    }
}

struct AgentListView: View {
    @ObservedObject var model: NotchModel

    var body: some View {
        let members = model.agents.filter { $0.group == model.tab }
        VStack(spacing: 0) {
            TabBar(model: model)
            if members.isEmpty {
                Text(model.agents.isEmpty ? "No Claude agents running" : "Nothing \(model.tab.title.lowercased())")
                    .font(.system(size: 12)).foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 40)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(members, id: \.id) { agent in
                            AgentRow(model: model, agent: agent)
                                .onTapGesture { model.open(agent.id) }
                                .contextMenu {
                                    Button("Hide") { model.onHide?(agent) }
                                    if case .job = agent.target {
                                        Divider()
                                        Button("Remove Agent…") { model.onRemove?(agent) }
                                    }
                                }
                        }
                        if model.tab == .completed {
                            Button("Clear all") { model.onClearCompleted?() }
                                .buttonStyle(.plain)
                                .font(.system(size: 11)).foregroundColor(.secondary)
                                .frame(maxWidth: .infinity)
                                .frame(height: 24)
                        }
                    }
                    .padding(.leading, 6)
                    .padding(.trailing, 12)
                    .padding(.top, 2)
                    .padding(.bottom, 6)
                }
            }
        }
    }
}

// MARK: - Peek

/// Carries a card's laid-out height up to the model, so the panel fits it exactly.
struct CardHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

extension View {
    func reportsCardHeight(to model: NotchModel) -> some View {
        fixedSize(horizontal: false, vertical: true)
            .background(GeometryReader { Color.clear.preference(key: CardHeightKey.self, value: $0.size.height) })
            .onPreferenceChange(CardHeightKey.self) { h in
                if h > 0, abs((model.cardHeight ?? 0) - h) > 0.5 { model.cardHeight = h }
            }
    }
}

struct PeekView: View {
    @ObservedObject var model: NotchModel
    let agent: Agent

    var body: some View {
        Group {
            if let request = model.permission(agent.id) {
                PermissionCard(model: model, agent: agent, request: request, showsAgent: true)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else if let q = model.question(agent.id) {
                QuestionCard(model: model, agent: agent, question: q, showsAgent: true)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Circle().fill(agent.color).frame(width: 8, height: 8)
                        Text(agent.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                        Spacer()
                        Tag(text: agent.host)
                        Text(agent.group == .needsInput ? "Reply ›" : "Open ›")
                            .font(.system(size: 11)).foregroundColor(.secondary)
                    }
                    if let text = model.snapshot(agent.id).lastReply.map(Self.lastParagraph) ?? agent.detail {
                        Text(Bubble.markdown(text))
                            .font(.system(size: 12.5))
                            .foregroundColor(.white.opacity(0.85))
                            .lineLimit(3)
                            .padding(.leading, 16)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 6)
                .contentShape(Rectangle())
                .onTapGesture { model.open(agent.id) }
            }
        }
    }

    /// The last paragraph is usually the question or the summary.
    static func lastParagraph(_ text: String) -> String {
        let paragraph = text.components(separatedBy: "\n\n")
            .last { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? text
        return String(paragraph.prefix(400))
    }
}

// MARK: - Question

/// "Claude asks": the question with its options as buttons, ⌘1…⌘9 to pick, and a free-text answer.
struct QuestionCard: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    let question: PendingQuestion
    var showsAgent = false
    @State private var typed = ""
    @FocusState private var typing: Bool

    var body: some View {
        let step = min(model.choices.count, question.items.count - 1)
        let item = question.items[step]
        let canAnswer = model.canAnswer(agent, question) && !model.sending

        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "bubble.left.fill").foregroundColor(.asks)
                Text("Claude asks").font(.system(size: 13, weight: .semibold)).foregroundColor(.asks)
                if question.items.count > 1 {
                    Text("\(step + 1) of \(question.items.count)").font(.system(size: 11)).foregroundColor(.secondary)
                }
                Spacer()
                if showsAgent { Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1) }
                if step > 0 {
                    Button("Back") { model.undoChoice() }.buttonStyle(.plain)
                        .font(.system(size: 11)).foregroundColor(.secondary)
                }
            }
            .frame(height: 22)

            Text(item.question)
                .font(.system(size: 14, weight: .medium))
                .fixedSize(horizontal: false, vertical: true)

            ForEach(Array(item.options.enumerated()), id: \.offset) { index, label in
                Button { model.choose(.option(index + 1), for: agent.id) } label: {
                    HStack(alignment: .top, spacing: 10) {
                        Text("⌘\(index + 1)")
                            .font(.system(size: 12, weight: .semibold, design: .rounded))
                            .foregroundColor(.white.opacity(0.55))
                            .frame(width: 30, height: 22)
                            .background(RoundedRectangle(cornerRadius: 6).fill(Color.optionBadge))
                        // The whole label and what it means, wrapped, so the choice can be read in full.
                        VStack(alignment: .leading, spacing: 2) {
                            Text(label).font(.system(size: 13, weight: .medium))
                                .fixedSize(horizontal: false, vertical: true)
                            if index < item.details.count, !item.details[index].isEmpty {
                                Text(item.details[index]).font(.system(size: 11.5)).foregroundColor(.secondary)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                        .padding(.top, 2)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity, minHeight: 34, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color.optionFill))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .keyboardShortcut(KeyEquivalent(Character("\(min(index + 1, 9))")), modifiers: .command)
                .disabled(!canAnswer)
            }

            if model.canAnswer(agent, question) {
                TextField("Type something else…", text: $typed)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .focused($typing)
                    .onSubmit {
                        let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !text.isEmpty else { return }
                        typed = ""
                        model.choose(.text(text), for: agent.id)
                    }
                    .padding(.horizontal, 12)
                    .frame(height: 34)
                    .background(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.12)))
                    .disabled(model.sending)
            } else {
                Button("Answer in \(agent.host) ›") { model.answerInTerminal(agent.id) }
                    .buttonStyle(.plain).font(.system(size: 12)).foregroundColor(.asks)
            }

            if let error = model.sendError {
                Text(error).font(.system(size: 11)).foregroundColor(.red)
            } else if model.sending {
                Text("Sending…").font(.system(size: 11)).foregroundColor(.secondary)
            }
        }
    }
}

// MARK: - Permission

/// "Claude wants to run …": allow once, always allow, deny, or hand it back to the terminal.
struct PermissionCard: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    let request: HookRequest
    var showsAgent = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "lock.shield.fill").foregroundColor(.orange)
                Text("Permission needed").font(.system(size: 13, weight: .semibold)).foregroundColor(.orange)
                Spacer()
                if showsAgent { Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1) }
            }
            .frame(height: 22)

            Text("Claude wants to use **\(request.toolName)**").font(.system(size: 13.5)).frame(height: 18)
            Text(request.summary)
                .font(.system(size: 12, design: .monospaced))
                .foregroundColor(.white.opacity(0.85))
                .lineLimit(10)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.07)))

            option(1, "Allow", .green) { model.decide(.allow, for: agent.id) }
            if request.canAlwaysAllow {
                option(2, "Always allow this", .green) { model.decide(.allowAlways, for: agent.id) }
            }
            option(request.canAlwaysAllow ? 3 : 2, "Deny", .red) { model.decide(.deny, for: agent.id) }

            Button("Answer in \(agent.host) ›") { model.decide(.terminal, for: agent.id) }
                .buttonStyle(.plain).font(.system(size: 12)).foregroundColor(.asks)
                .frame(height: 22)
        }
    }

    private func option(_ n: Int, _ label: String, _ tint: Color, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Text("⌘\(n)")
                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                    .foregroundColor(.white.opacity(0.55))
                    .frame(width: 30, height: 24)
                    .background(RoundedRectangle(cornerRadius: 6).fill(Color.optionBadge))
                Text(label).font(.system(size: 13.5)).foregroundColor(tint)
                Spacer()
            }
            .padding(.horizontal, 8)
            .frame(height: 32)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.optionFill))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .keyboardShortcut(KeyEquivalent(Character("\(n)")), modifiers: .command)
    }
}

// MARK: - Conversation

struct DetailView: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    @FocusState private var focused: Bool

    var body: some View {
        let question = model.question(agent.id)
        let permission = model.permission(agent.id)
        VStack(alignment: .leading, spacing: 10) {
            header
            conversation(showNeeds: question == nil && permission == nil)
            if let permission {
                PermissionCard(model: model, agent: agent, request: permission)
            } else if let question {
                QuestionCard(model: model, agent: agent, question: question)
            } else if agent.canReply {
                replyBox
            } else {
                HStack {
                    Text(agent.label == "Exited" ? "This session has exited. Open it to resume."
                                                 : "This session runs in \(agent.host). Open it to reply there.")
                        .font(.system(size: 11)).foregroundColor(.secondary)
                    Spacer()
                    Button("Open") { AgentActions.open(agent.target, cwd: agent.cwd) }
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.top, 4)
        .padding(.bottom, 12)
        .onExitCommand { model.go(.list) }
    }

    private var header: some View {
        HStack(spacing: 10) {
            Button { model.go(.list) } label: { Image(systemName: "chevron.left") }
                .buttonStyle(.plain).foregroundColor(.secondary)
            Circle().fill(agent.color).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 1) {
                Text(agent.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Text("\(agent.label) · \(agent.host) · \(agent.ago)")
                    .font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
            }
            Spacer()
            Button { AgentActions.open(agent.target, cwd: agent.cwd) } label: {
                Image(systemName: "arrow.up.forward.app")
            }
            .buttonStyle(.plain).foregroundColor(.secondary).help("Open in terminal")
            Button { model.go(.collapsed) } label: { Image(systemName: "xmark") }
                .buttonStyle(.plain).foregroundColor(.secondary)
        }
    }

    private func conversation(showNeeds: Bool) -> some View {
        let lines = model.snapshot(agent.id).lines
        return ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if lines.isEmpty {
                        Text("No messages yet").font(.system(size: 12)).foregroundColor(.secondary)
                    }
                    ForEach(lines) { line in Bubble(line: line) }
                    if showNeeds, agent.group == .needsInput, let needs = agent.detail, !needs.isEmpty {
                        HStack(alignment: .top, spacing: 8) {
                            Image(systemName: "hand.raised.fill").foregroundColor(.orange)
                            Text(needs).font(.system(size: 12)).textSelection(.enabled)
                        }
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 10).fill(Color.orange.opacity(0.15)))
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.vertical, 4)
                // Room for the overlay scroll bar, so it never sits on top of the text.
                .padding(.trailing, 12)
            }
            .onAppear { proxy.scrollTo("bottom", anchor: .bottom) }
            .onChange(of: lines) { _ in
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    private var replyBox: some View {
        let completions = model.completions(for: agent)
        let suggestion = model.suggestion(for: agent)
        return VStack(alignment: .leading, spacing: 6) {
            if !completions.isEmpty {
                CommandList(model: model, commands: completions)
            }
            if completions.isEmpty, let suggested = agent.suggestedReply, !suggested.isEmpty, agent.group == .needsInput {
                Button { model.send(suggested) } label: {
                    Text(suggested).font(.system(size: 11, weight: .medium))
                        .padding(.horizontal, 10).padding(.vertical, 4)
                        .background(Capsule().fill(Color.orange.opacity(0.25)))
                }
                .buttonStyle(.plain)
                .disabled(model.sending)
            }
            HStack(alignment: .bottom, spacing: 8) {
                TextField(suggestion.map { "\($0)   ⇥ Tab" } ?? "Reply, or / for commands…", text: $model.draft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .lineLimit(1...5)
                    .focused($focused)
                    .onSubmit { model.send() }
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.09)))
                Button { model.send() } label: {
                    if model.sending {
                        ProgressView().controlSize(.small).frame(width: 24, height: 24)
                    } else {
                        Image(systemName: "arrow.up.circle.fill").font(.system(size: 24))
                    }
                }
                .buttonStyle(.plain)
                .foregroundColor(model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .secondary : .orange)
                .disabled(model.sending)
            }
            if let error = model.sendError {
                Text(error).font(.system(size: 11)).foregroundColor(.red)
            } else if model.sending {
                Text("Sending… it shows up here once the agent has it.").font(.system(size: 11)).foregroundColor(.secondary)
            }
        }
        .onAppear { DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { focused = true } }
    }
}

/// Slash commands matching what's typed; ↑↓ to move, Tab or Return to pick.
struct CommandList: View {
    @ObservedObject var model: NotchModel
    let commands: [SlashCommand]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(commands.enumerated()), id: \.offset) { index, command in
                let selected = index == min(model.completionIndex, commands.count - 1)
                Button { model.accept(command) } label: {
                    VStack(alignment: .leading, spacing: 1) {
                        HStack(spacing: 6) {
                            Text("/" + command.name).font(.system(size: 12, weight: .semibold, design: .monospaced))
                            if let hint = command.argumentHint {
                                Text(hint).font(.system(size: 11, design: .monospaced)).foregroundColor(.secondary).lineLimit(1)
                            }
                        }
                        if !command.description.isEmpty {
                            Text(command.description).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(selected ? 0.12 : 0)))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(4)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.05)))
    }
}

/// One conversation entry, laid out like Claude Code's terminal: "❯" prompts on a shaded bar,
/// Claude's text after a "⏺", and dim tool summaries indented beneath it.
struct Bubble: View {
    let line: ChatLine

    var body: some View {
        switch line.kind {
        case .user:
            row("❯", .secondary, Text(line.text).foregroundColor(.white.opacity(0.9)))
                .padding(.vertical, 5)
                .padding(.horizontal, 6)
                .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(0.1)))
        case .assistant:
            row("⏺", .white.opacity(0.85), Text(Self.markdown(line.text)))
                .padding(.horizontal, 6)
        case .tools:
            row("⎿", .secondary, Text(line.text).foregroundColor(.secondary))
                .padding(.leading, 18)
                .padding(.trailing, 6)
        }
    }

    private func row(_ mark: String, _ markColor: Color, _ text: Text) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(mark).foregroundColor(markColor)
            text.textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .font(.system(size: 11.5, design: .monospaced))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    static func markdown(_ s: String) -> AttributedString {
        (try? AttributedString(markdown: s, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(s)
    }
}
