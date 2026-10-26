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

/// A little green candy monster that swings its way along a row of ropes hanging under the camera,
/// like a level of Cut the Rope. On each rope it swings like a real pendulum, slow at the top of
/// the arc and fastest at the bottom, pumping its legs. Near the top of a forward swing it lets
/// go, flies on the speed it had, and catches the next rope, carrying that speed into the next
/// swing. At the last rope it turns round. It goes faster while sessions need you. When the tabs
/// open it moves into the middle and fades.
struct PetView: View {
    @ObservedObject var model: NotchModel
    /// Size of the black shape this frame; the ropes hang along its bottom edge.
    let shape: CGSize

    /// Height of a pose frame, from the hands on the rope down to the feet.
    static let bodyHeight: CGFloat = 30
    static let rope: CGFloat = 18
    /// From the anchor to the body's centre of mass, which sets how fast it swings.
    static let pendulum = Double(rope + bodyHeight * 0.6)
    static let gravity = 500.0
    /// How far forward of straight down it lets go, and where it catches the next rope (behind it).
    static let release = 0.6, catchAngle = -0.5

    /// Legs kicked forward, hanging straight, legs tucked back, all facing right. Each frame is
    /// pre-rotated so its centre of mass sits straight under the hands, as it would on a taut rope.
    /// The probe build has no bundle, so it falls back to ✻.
    static let poses: [NSImage] = (0..<3).compactMap { index in
        Bundle.main.url(forResource: "swing\(index)", withExtension: "png").flatMap(NSImage.init(contentsOf:))
    }

    /// A jump between ropes: a parabola that leaves with the swing's own velocity and lands on the
    /// next rope's catching point, so its gravity comes out close to the real one.
    fileprivate struct Flight {
        let from: CGPoint
        let velocity: CGVector
        let gravity: Double
        let duration: Double
        let target: Int
        let startAngle: Double
        var elapsed = 0.0

        func point(_ t: Double) -> CGPoint {
            CGPoint(x: from.x + velocity.dx * t, y: from.y + velocity.dy * t + gravity * t * t / 2)
        }
    }

    /// Everything that carries over between frames: which rope it holds, which way it's heading,
    /// the swing, any jump in progress, and the loose ropes still swaying after it let go.
    fileprivate final class Swing: ObservableObject {
        var index = 0
        var heading = 1.0
        var angle = 0.0
        var spin = 0.0
        var flight: Flight?
        var ropeAngles: [Double] = []
        var ropeSpins: [Double] = []
        var last: Double?
    }

    /// What one frame draws: where the hands are, how the body is turned, and every rope's angle.
    fileprivate struct Pose {
        let hands: CGPoint
        let angle: Double
        let heading: Double
        let ropes: [Double]
        let held: Int?
    }

    @StateObject private var swing = Swing()

    var body: some View {
        let gathered = model.state == .list
        let resting = model.state == .collapsed
        let moving = model.petWalking && resting
        let needs = model.needsCount
        let anchors = Self.anchors(for: shape)
        TimelineView(.animation(minimumInterval: 1.0 / 60, paused: !moving)) { context in
            let pose = step(to: context.date.timeIntervalSinceReferenceDate, moving: moving, anchors)
            let hands = gathered ? middle : pose.hands
            let width = Self.bodyWidth
            let box = width + 34

            ZStack(alignment: .topLeading) {
                ForEach(anchors.indices, id: \.self) { index in
                    // The held rope runs to the hands; loose ones hang their full length.
                    let length = pose.held == index && !gathered ? Self.rope + 2 : Self.rope
                    Rectangle()
                        .fill(Color(red: 0.86, green: 0.78, blue: 0.62))
                        .frame(width: 1.4, height: length)
                        .rotationEffect(.radians(-(index < pose.ropes.count ? pose.ropes[index] : 0)), anchor: .top)
                        .offset(x: anchors[index].x - 0.7, y: anchors[index].y)
                }
                ZStack(alignment: .top) {
                    creature(angle: gathered ? 0 : pose.angle * pose.heading)
                        .scaleEffect(x: pose.heading, y: 1)
                        .frame(width: width, height: Self.bodyHeight)
                    if needs > 0 && !gathered {
                        Text("\(needs)")
                            .font(.system(size: 9.5, weight: .bold, design: .rounded))
                            .foregroundColor(.white)
                            .padding(.horizontal, 3.5)
                            .frame(minWidth: 14, minHeight: 14)
                            .background(Capsule().fill(Color.orange))
                            .shadow(color: .black.opacity(0.35), radius: 1.5)
                            .fixedSize()
                            .offset(x: width / 2 + 3, y: Self.bodyHeight * 0.5)
                    }
                }
                .frame(width: box, height: Self.bodyHeight, alignment: .top)
                .rotationEffect(.radians(gathered ? 0 : -pose.angle), anchor: .top)
                .offset(x: hands.x - box / 2, y: hands.y)
            }
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.85), value: gathered)
        // Only out on its own while the notch rests; it fades once it reaches the tabs.
        .opacity(resting ? 1 : 0)
        .animation(resting ? .easeIn(duration: 0.15).delay(0.15) : .easeOut(duration: 0.12).delay(gathered ? 0.24 : 0),
                   value: resting)
    }

    static var bodyWidth: CGFloat {
        guard let first = poses.first, first.size.height > 0 else { return bodyHeight * 0.6 }
        return bodyHeight * first.size.width / first.size.height
    }

    /// Evenly spaced along the flat part of the bottom edge, about a jump's length apart.
    static func anchors(for shape: CGSize) -> [CGPoint] {
        let lo = Double(NotchModel.flare) + 14, hi = Double(shape.width - NotchModel.flare) - 14
        let span = max(0, hi - lo)
        let count = max(2, Int(span / 48) + 1)
        let gap = span / Double(count - 1)
        return (0..<count).map { CGPoint(x: lo + gap * Double($0), y: Double(shape.height)) }
    }

    /// Fades between poses by how far out the swing is, measured in the direction it's heading:
    /// legs out at the front of the arc, tucked at the back, straight through the bottom.
    @ViewBuilder private func creature(angle: Double) -> some View {
        if Self.poses.count == 3 {
            let forward = smoothstep(0.12, 0.45, angle), back = smoothstep(0.12, 0.45, -angle)
            let weights = [forward, 1 - forward - back, back]
            ZStack {
                ForEach(0..<3, id: \.self) { index in
                    Image(nsImage: Self.poses[index])
                        .resizable()
                        .interpolation(.high)
                        .opacity(weights[index])
                }
            }
            .shadow(color: .black.opacity(0.45), radius: 1.5)
        } else {
            Text("✻").font(.system(size: 15, weight: .bold)).foregroundColor(.claude)
        }
    }

    private func smoothstep(_ low: Double, _ high: Double, _ value: Double) -> Double {
        let t = min(max((value - low) / (high - low), 0), 1)
        return t * t * (3 - 2 * t)
    }

    private var middle: CGPoint {
        CGPoint(x: shape.width / 2, y: model.notchSize.height + TabBar.height / 2 - Self.bodyHeight / 2)
    }

    /// Hands on a rope at `angle` (positive swings right; screen y points down).
    private static func hands(on anchor: CGPoint, at angle: Double) -> CGPoint {
        CGPoint(x: anchor.x + Double(rope) * sin(angle), y: anchor.y + Double(rope) * cos(angle))
    }

    /// Advances the swing, a jump, and the loose ropes. On a rope it's a damped pendulum that only
    /// gains energy from a push near the bottom in the direction it's going, the way a kid pumps a
    /// swing; so every arc follows gravity.
    private func step(to t: Double, moving: Bool, _ anchors: [CGPoint]) -> Pose {
        let s = swing
        if s.ropeAngles.count != anchors.count {
            s.ropeAngles = Array(repeating: 0, count: anchors.count)
            s.ropeSpins = Array(repeating: 0, count: anchors.count)
            s.index = anchors.count / 2
            s.flight = nil
        }
        s.index = min(max(s.index, 0), anchors.count - 1)

        if moving, let last = s.last {
            // A long gap (a stalled frame) moves it one frame's worth, never a leap.
            let dt = min(max(t - last, 0), 0.05)
            let substeps = 8
            for _ in 0..<substeps { advance(s, by: dt / Double(substeps), anchors) }
        } else if !moving {
            // Hangs still with Animate Pet off; otherwise waits held up behind itself, ready to let go.
            s.flight = nil
            s.angle = model.petWalking ? -0.6 * s.heading : 0
            s.spin = 0
            for i in s.ropeAngles.indices { s.ropeAngles[i] = 0; s.ropeSpins[i] = 0 }
        }
        s.last = moving ? t : nil

        var ropes = s.ropeAngles
        if let flight = s.flight {
            // Turn the body from its letting-go angle to its catching angle over the jump.
            let progress = min(flight.elapsed / flight.duration, 1)
            let angle = flight.startAngle + (Self.catchAngle * s.heading - flight.startAngle) * progress
            return Pose(hands: flight.point(flight.elapsed), angle: angle, heading: s.heading, ropes: ropes, held: nil)
        }
        ropes[s.index] = s.angle
        return Pose(hands: Self.hands(on: anchors[s.index], at: s.angle), angle: s.angle,
                    heading: s.heading, ropes: ropes, held: s.index)
    }

    private func advance(_ s: Swing, by h: Double, _ anchors: [CGPoint]) {
        let length = Self.pendulum, g = Self.gravity, k = g / length
        let urgent = model.needsCount > 0
        let height = urgent ? 1.25 : 1.1, pump = urgent ? 6.0 : 4.0, air = 0.22

        // Loose ropes sway on their own and settle.
        let ropeK = g / Double(Self.rope)
        for i in s.ropeAngles.indices where s.flight != nil || i != s.index {
            s.ropeSpins[i] += (-ropeK * sin(s.ropeAngles[i]) - 4 * s.ropeSpins[i]) * h
            s.ropeAngles[i] += s.ropeSpins[i] * h
        }

        if var flight = s.flight {
            flight.elapsed += h
            if flight.elapsed >= flight.duration {
                // Caught: what's left of its speed along the rope's swing becomes the new spin.
                let a = Self.catchAngle * s.heading
                let vx = flight.velocity.dx, vy = flight.velocity.dy + flight.gravity * flight.duration
                s.index = flight.target
                s.angle = a
                s.spin = (vx * cos(a) - vy * sin(a)) / length
                s.ropeSpins[s.index] = 0
                s.flight = nil
            } else {
                s.flight = flight
            }
            return
        }

        var accel = -k * sin(s.angle) - air * s.spin
        let energy = 0.5 * s.spin * s.spin + k * (1 - cos(s.angle))
        if abs(s.angle) < 0.35 && energy < k * (1 - cos(height)) {
            accel += (s.spin >= 0 ? 1 : -1) * pump
        }
        s.spin += accel * h
        s.angle += s.spin * h

        let forward = s.angle * s.heading, goingForward = s.spin * s.heading > 0
        let next = s.index + Int(s.heading)
        guard anchors.indices.contains(next) else {
            // Last rope: turn round at the top of the forward swing.
            if forward > 0.3 && !goingForward { s.heading = -s.heading }
            return
        }
        guard forward >= Self.release && goingForward else { return }

        // Let go: leave with the swing's velocity and aim for the next rope's catching point,
        // bending gravity just enough to land on it.
        let from = Self.hands(on: anchors[s.index], at: s.angle)
        let to = Self.hands(on: anchors[next], at: Self.catchAngle * s.heading)
        let speed = s.spin * length
        let vx = speed * cos(s.angle), vy = -speed * sin(s.angle)
        let duration = min(max(Double(to.x - from.x) / vx, 0.15), 0.6)
        let fall = 2 * (Double(to.y - from.y) - vy * duration) / (duration * duration)
        let velocity = CGVector(dx: Double(to.x - from.x) / duration, dy: vy)
        s.ropeAngles[s.index] = s.angle
        s.ropeSpins[s.index] = s.spin * length / Double(Self.rope) * 0.3
        s.flight = Flight(from: from, velocity: velocity, gravity: fall, duration: duration, target: next, startAngle: s.angle)
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
