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

/// A little green candy monster that web-swings along the whole menu bar. It picks a spot far along
/// the bar and heads there: it hangs from a strand of silk stuck to the bottom edge of the bar and
/// swings on it like a real pendulum, pumping its legs. Near the top of a forward swing it lets go,
/// tumbles through the air in a random pose, shoots a new strand up to the edge ahead, and swings on
/// that. Far from the spot it takes long strands and big hops; near it, short ones, and when it
/// arrives it rests, waving or dozing, then picks a new spot (turning round at the top of a swing
/// if that's behind it). The silk is a chain of points with its own inertia: taut it runs straight,
/// slack it sags, and a strand it lets go of hangs loose, sways and fades. Every strand it catches
/// gets its own roll of length, swing height, release angle and poses (kicking, tucked in a ball,
/// running, cheering). It goes faster while sessions need you. When the tabs open it moves into the
/// middle and fades. Positions are kept relative to the camera, so the black shape growing for a
/// peek doesn't move it along the bar.
struct PetView: View {
    @ObservedObject var model: NotchModel
    /// Size of the black shape this frame; the silk sticks to the bottom of the menu bar, which is
    /// as wide as the screen.
    let shape: CGSize

    /// Height of a pose frame, from the hands down to the feet.
    static let bodyHeight: CGFloat = 37.5
    static let gravity = 450.0
    /// The strand length it settles to when nothing else is rolled, and the range it will reel to.
    static let reach = 22.0, shortest = 14.0, longest = 100.0
    /// How fast a shot strand flies, and how long after letting go it shoots.
    static let shotSpeed = 420.0, shotDelay = 0.07
    /// How long one pose takes to fade into the next.
    static let fadeTime = 0.12
    static let silk = Color(white: 0.93)

    static let swingPoses = ["swing-forward", "swing-down", "swing-back"]
    static let flightPoses = ["fly-superhero", "fly-flip", "fly-star"]
    static let poseNames = swingPoses + flightPoses + [
        "fly-shoot", "fly-reach", "hang-wave", "hang-ball", "hang-cheer", "hang-run", "hang-sleepy",
    ]

    /// Every pose frame by name. Each is pre-rotated so its centre of mass sits under the hands,
    /// which are at the top centre, and all face right. The probe build has no bundle, so a missing
    /// frame falls back to ✻.
    static let poses: [String: NSImage] = Dictionary(uniqueKeysWithValues: poseNames.compactMap { name in
        Bundle.main.url(forResource: name, withExtension: "png")
            .flatMap(NSImage.init(contentsOf:))
            .map { (name, $0) }
    })

    /// Which hanging poses a strand uses while swinging.
    fileprivate enum Style {
        /// Legs kick forward, hang, tuck back, by swing angle.
        case kick
        /// Curled in a ball through the bottom of the arc.
        case tuck
        /// Running in the air while it moves forward fast.
        case run
        /// Legs spread near the forward peak.
        case cheer
    }

    /// A strand of silk as a chain of points. `previous` holds last frame's points, which is what
    /// gives every point its inertia. The first point is stuck to the edge; while held, the last
    /// point is pinned to the hands, and once let go it hangs free and the strand fades.
    fileprivate struct Strand {
        static let segments = 10
        var points: [CGPoint]
        var previous: [CGPoint]
        var rest: Double
        var held = true
        var fade = 1.0

        init(from anchor: CGPoint, to hands: CGPoint, length: Double) {
            points = (0...Self.segments).map { index in
                let t = Double(index) / Double(Self.segments)
                return CGPoint(x: anchor.x + (hands.x - anchor.x) * t, y: anchor.y + (hands.y - anchor.y) * t)
            }
            previous = points
            rest = length / Double(Self.segments)
        }

        mutating func simulate(_ dt: Double, hands: CGPoint) {
            let pull = PetView.gravity * dt * dt
            for i in 1..<points.count {
                let p = points[i], q = previous[i]
                previous[i] = p
                points[i] = CGPoint(x: p.x + (p.x - q.x) * 0.97, y: p.y + (p.y - q.y) * 0.97 + pull)
            }
            if held { points[points.count - 1] = hands }
            let last = held ? points.count - 2 : points.count - 1
            for _ in 0..<12 {
                for i in 0..<(points.count - 1) {
                    let a = points[i], b = points[i + 1]
                    let dx = b.x - a.x, dy = b.y - a.y
                    let length = max((dx * dx + dy * dy).squareRoot(), 0.0001)
                    // Silk only pulls: a short gap is slack, never pushed apart.
                    guard length > rest else { continue }
                    let fix = (length - rest) / length
                    let aFree = i > 0, bFree = i + 1 <= last
                    let share = aFree && bFree ? 0.5 : 1
                    if aFree { points[i] = CGPoint(x: a.x + dx * fix * share, y: a.y + dy * fix * share) }
                    if bFree { points[i + 1] = CGPoint(x: b.x - dx * fix * share, y: b.y - dy * fix * share) }
                }
            }
        }
    }

    /// A strand on its way from the hands to the spot ahead on the edge.
    fileprivate struct Shot {
        let target: CGPoint
        var travelled = 0.0
    }

    /// Everything that carries over between frames.
    fileprivate final class Swing: ObservableObject {
        var rng = SystemRandomNumberGenerator()
        var hands: CGPoint?
        var velocity = CGVector.zero
        var heading = 1.0
        var anchor: CGPoint?
        var length = PetView.reach
        var strands: [Strand] = []
        var shot: Shot?
        var airborne = 0.0
        var bodyAngle = 0.0
        var last: Double?

        // Rolled again for every strand it catches.
        var style = Style.kick
        var reach = PetView.reach
        var height = 1.15
        var release = 0.55
        /// Seconds left of a rest on this strand: no pumping, drag up, waving or dozing.
        var restLeft = 0.0
        var restPose = "hang-wave"
        /// Seconds after a catch spent reaching, before the strand counts as taut.
        var reaching = 0.0
        /// Whether the strand was rolled for a long way to go.
        var far = false
        /// Where it's heading along the bar, and whether it has got there and is resting.
        var destination: Double?
        var atDestination = false

        // Rolled when it lets go.
        var flightPose = "fly-superhero"
        var aim = 1.0
        var longShot = false
        var spin = 0.0
        var spinRate = 0.0

        /// How much of each pose is showing, so poses fade into each other.
        var weights: [String: Double] = [:]

        func random(_ range: ClosedRange<Double>) -> Double { Double.random(in: range, using: &rng) }
        func chance(_ probability: Double) -> Bool { random(0...1) < probability }
    }

    /// What one frame draws.
    fileprivate struct Frame {
        let hands: CGPoint
        let angle: Double
        let heading: Double
        let weights: [String: Double]
        let spin: Double
        let strands: [(points: [CGPoint], opacity: Double)]
        let shot: (from: CGPoint, to: CGPoint)?
    }

    @StateObject private var swing = Swing()

    var body: some View {
        let gathered = model.state == .list
        let resting = model.state == .collapsed
        let moving = model.petWalking && resting
        let needs = model.needsCount
        TimelineView(.animation(minimumInterval: 1.0 / 60, paused: !moving)) { context in
            let frame = step(to: context.date.timeIntervalSinceReferenceDate, moving: moving)
            let hands = gathered ? middle : frame.hands
            let width = Self.bodyWidth
            let box = width + 34

            ZStack(alignment: .topLeading) {
                silk(frame)
                    .opacity(gathered ? 0 : 1)
                ZStack(alignment: .top) {
                    creature(weights: gathered ? ["swing-down": 1] : frame.weights, spin: frame.spin)
                        .scaleEffect(x: frame.heading, y: 1)
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
                .rotationEffect(.radians(gathered ? 0 : -frame.angle), anchor: .top)
                .offset(x: hands.x - box / 2, y: hands.y)
            }
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.85), value: gathered)
        // Only out on its own while the notch rests; it fades once it reaches the tabs.
        .opacity(resting ? 1 : 0)
        .animation(resting ? .easeIn(duration: 0.15).delay(0.15) : .easeOut(duration: 0.12).delay(gathered ? 0.24 : 0),
                   value: resting)
    }

    /// Every strand as a smooth line through its points, a dab where it sticks to the edge, and a
    /// straight streak for a strand still flying.
    private func silk(_ frame: Frame) -> some View {
        ZStack(alignment: .topLeading) {
            ForEach(frame.strands.indices, id: \.self) { index in
                let strand = frame.strands[index]
                Path { path in
                    guard let first = strand.points.first else { return }
                    path.move(to: first)
                    for i in 1..<strand.points.count {
                        let mid = CGPoint(x: (strand.points[i - 1].x + strand.points[i].x) / 2,
                                          y: (strand.points[i - 1].y + strand.points[i].y) / 2)
                        path.addQuadCurve(to: mid, control: strand.points[i - 1])
                    }
                    path.addLine(to: strand.points[strand.points.count - 1])
                }
                .stroke(Self.silk.opacity(0.9 * strand.opacity), style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
                if let first = strand.points.first {
                    Circle().fill(Self.silk.opacity(strand.opacity))
                        .frame(width: 3, height: 3)
                        .offset(x: first.x - 1.5, y: first.y - 1.5)
                }
            }
            if let shot = frame.shot {
                Path { path in path.move(to: shot.from); path.addLine(to: shot.to) }
                    .stroke(Self.silk.opacity(0.9), style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
            }
        }
        // Wide enough to reach every spot along the bar; the origin stays at the shape's corner.
        .frame(width: shape.width + model.screenSpan.left + model.screenSpan.right,
               height: shape.height + 160, alignment: .topLeading)
        .allowsHitTesting(false)
    }

    static var bodyWidth: CGFloat {
        guard let first = poses["swing-down"], first.size.height > 0 else { return bodyHeight * 0.6 }
        return bodyHeight * first.size.width / first.size.height
    }

    /// All poses stacked, each as visible as its weight. Only the flip pose turns, once round.
    @ViewBuilder private func creature(weights: [String: Double], spin: Double) -> some View {
        if Self.poses.count == Self.poseNames.count {
            ZStack {
                ForEach(Self.poseNames.filter { (weights[$0] ?? 0) > 0.001 }, id: \.self) { name in
                    if let image = Self.poses[name] {
                        Image(nsImage: image)
                            .resizable()
                            .interpolation(.high)
                            .rotationEffect(.radians(name == "fly-flip" ? spin : 0))
                            .opacity(weights[name] ?? 0)
                    }
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

    /// Fades between the swing poses by how far out the swing is, measured in the direction it's
    /// heading: legs out at the front of the arc, tucked at the back, straight through the bottom.
    private func swingBlend(_ angle: Double) -> [String: Double] {
        let forward = smoothstep(0.12, 0.45, angle), back = smoothstep(0.12, 0.45, -angle)
        return ["swing-forward": forward, "swing-down": 1 - forward - back, "swing-back": back]
    }

    /// The spot the tabs gather it to, under the camera.
    private var middle: CGPoint {
        overlay(CGPoint(x: 0, y: model.notchSize.height + TabBar.height / 2 - Self.bodyHeight / 2))
    }

    /// Where silk can stick: the bottom of the menu bar across the whole screen, in the
    /// camera-centred frame (x = 0 under the camera, y = 0 at the top of the screen).
    private var edge: (lo: Double, hi: Double, y: Double) {
        let lo = 14 - Double(model.screenSpan.left)
        let hi = Double(model.screenSpan.right) - 14
        return (lo, max(hi, lo + 1), Double(model.notchSize.height))
    }

    /// Which poses it wants right now: the airborne ones by phase of the flight, then a rest, then
    /// the strand's style while swinging.
    private func wantedPoses(_ s: Swing, moving: Bool) -> [String: Double] {
        let angle = s.bodyAngle * s.heading
        guard moving else { return swingBlend(angle) }
        guard s.anchor != nil else { return [s.shot != nil ? "fly-shoot" : s.flightPose: 1] }
        if s.reaching > 0 { return ["fly-reach": 1] }
        if s.restLeft > 0 { return [s.restPose: 1] }
        switch s.style {
        case .kick: return swingBlend(angle)
        case .tuck: return abs(angle) < 0.3 ? ["hang-ball": 1] : swingBlend(angle)
        case .run: return s.velocity.dx * s.heading > 60 ? ["hang-run": 1] : swingBlend(angle)
        case .cheer: return angle > 0.35 ? ["hang-cheer": 1] : swingBlend(angle)
        }
    }

    /// Moves each pose's weight towards what's wanted, all at the same rate so they sum to one.
    private func fade(_ s: Swing, toward wanted: [String: Double], by dt: Double) {
        let amount = dt / Self.fadeTime
        for name in Set(s.weights.keys).union(wanted.keys) {
            let current = s.weights[name] ?? 0, goal = wanted[name] ?? 0
            let next = current + min(max(goal - current, -amount), amount)
            s.weights[name] = next > 0.001 ? next : nil
        }
    }

    /// Rolls the look of the next strand. Far from where it's heading it takes long, high swings
    /// to cover ground; near it, short ones to land precisely, and sometimes a rest.
    private func rollStrand(_ s: Swing) {
        let urgent = model.needsCount > 0 ? 0.15 : 0
        let remaining = s.destination.map { abs($0 - Double(s.hands?.x ?? 0)) } ?? 0
        s.far = remaining > 200
        if s.far {
            s.reach = s.random(40...90)
            s.height = min(s.random(1.2...1.5) + urgent, 1.6)
        } else {
            s.reach = s.random(16...30)
            s.height = s.random(0.9...1.2) + urgent
        }
        s.release = s.random(0.45...0.8)
        let pick = s.random(0...1)
        s.style = pick < 0.5 ? .kick : pick < 0.68 ? .tuck : pick < 0.84 ? .run : .cheer
        s.restLeft = 0
        if !s.far && s.chance(0.2) { startRest(s) }
    }

    private func startRest(_ s: Swing) {
        s.restLeft = s.random(1.5...4)
        s.restPose = model.needsCount == 0 && s.restLeft >= 3 ? "hang-sleepy" : "hang-wave"
    }

    /// Somewhere else along the bar, at least 250 pt from where it is (or the farther end if the
    /// bar is too short for that), so it always seems to be going somewhere.
    private func pickDestination(_ s: Swing, _ edge: (lo: Double, hi: Double, y: Double)) {
        let x = Double(s.hands?.x ?? 0)
        let left = (x - 250) - edge.lo, right = edge.hi - (x + 250)
        if left > 0 || right > 0 {
            let goLeft = s.random(0...(max(left, 0) + max(right, 0))) < max(left, 0)
            s.destination = goLeft ? s.random(edge.lo...(x - 250)) : s.random((x + 250)...edge.hi)
        } else {
            s.destination = x - edge.lo > edge.hi - x ? edge.lo : edge.hi
        }
        s.atDestination = false
    }

    /// All positions are kept in a frame centred on the camera, so they stay put on screen when the
    /// black shape grows or shrinks; this shifts one into the overlay's frame, whose origin is the
    /// shape's top-left corner.
    private func overlay(_ point: CGPoint) -> CGPoint {
        CGPoint(x: point.x + shape.width / 2, y: point.y)
    }

    private func step(to t: Double, moving: Bool) -> Frame {
        let s = swing
        let edge = self.edge
        if s.hands == nil || s.anchor.map({ $0.x < edge.lo - 1 || $0.x > edge.hi + 1 }) == true {
            let start = CGPoint(x: s.random(edge.lo...edge.hi), y: edge.y)
            hang(s, at: start, angle: 0)
            pickDestination(s, edge)
            s.heading = (s.destination ?? 0) >= Double(start.x) ? 1 : -1
            rollStrand(s)
            hang(s, at: start, angle: 0)
        }
        if s.destination == nil { pickDestination(s, edge) }

        var fadeDt = 1.0
        if moving, let last = s.last {
            // A long gap (a stalled frame) moves it one frame's worth, never a leap.
            let dt = min(max(t - last, 0), 0.05)
            fadeDt = dt
            let substeps = 8
            for _ in 0..<substeps { advance(s, by: dt / Double(substeps), edge) }
            let hands = s.hands ?? .zero
            for i in s.strands.indices {
                if s.strands[i].held { s.strands[i].rest = s.length / Double(Strand.segments) }
                s.strands[i].simulate(dt, hands: hands)
                if !s.strands[i].held { s.strands[i].fade -= dt / 1.4 }
            }
            s.strands.removeAll { $0.fade <= 0 }
        } else if !moving, let anchor = s.anchor ?? s.shot?.target ?? s.strands.last?.points.first {
            // Still with Animate Pet off; otherwise ready on its strand, held up behind itself.
            hang(s, at: anchor, angle: model.petWalking ? -0.6 * s.heading : 0)
        }
        s.last = moving ? t : nil
        fade(s, toward: wantedPoses(s, moving: moving), by: fadeDt)

        let hands = s.hands ?? .zero
        let shot = s.shot.map { shot -> (CGPoint, CGPoint) in
            let dx = shot.target.x - hands.x, dy = shot.target.y - hands.y
            let distance = max((dx * dx + dy * dy).squareRoot(), 0.0001)
            let reach = min(shot.travelled / distance, 1)
            return (overlay(hands), overlay(CGPoint(x: hands.x + dx * reach, y: hands.y + dy * reach)))
        }
        return Frame(hands: overlay(hands), angle: s.bodyAngle, heading: s.heading, weights: s.weights, spin: s.spin,
                     strands: s.strands.map { ($0.points.map(overlay), max(0, min(1, $0.fade))) }, shot: shot)
    }

    /// Puts it on a fresh, taut strand from `anchor`, `angle` out from straight down.
    private func hang(_ s: Swing, at anchor: CGPoint, angle: Double) {
        s.anchor = anchor
        s.length = s.reach
        s.hands = CGPoint(x: anchor.x + s.length * sin(angle), y: anchor.y + s.length * cos(angle))
        s.velocity = .zero
        s.shot = nil
        s.reaching = 0
        s.bodyAngle = angle
        s.strands = [Strand(from: anchor, to: s.hands!, length: s.length)]
    }

    /// One small step of the body as a weight on inextensible silk: gravity and air always, a pump
    /// near the bottom of the arc like a kid on a swing (harder on a long strand, and not while
    /// resting, when the air holds it back until it hangs nearly still), and when the strand is
    /// taut it can't get any further from the anchor, so only the swinging part of its speed survives.
    private func advance(_ s: Swing, by h: Double, _ edge: (lo: Double, hi: Double, y: Double)) {
        guard var hands = s.hands else { return }
        let pump = (model.needsCount > 0 ? 150.0 : 100.0) * max(1, s.reach / Self.reach)
        let resting = s.restLeft > 0
        let drag = resting ? 2.5 : 0.25
        var v = s.velocity
        v.dy += Self.gravity * h
        v.dx -= drag * v.dx * h
        v.dy -= drag * v.dy * h
        if s.reaching > 0 { s.reaching -= h }
        else if resting, s.anchor != nil {
            s.restLeft -= h
            // Rested at the destination: time to head somewhere else.
            if s.restLeft <= 0 && s.atDestination { s.destination = nil }
        }

        var ropeAngle: Double?
        if let anchor = s.anchor {
            let dx = Double(hands.x - anchor.x), dy = Double(hands.y - anchor.y)
            let angle = atan2(dx, dy)
            ropeAngle = angle
            let tangent = CGVector(dx: cos(angle), dy: -sin(angle))
            let along = v.dx * tangent.dx + v.dy * tangent.dy
            let energy = 0.5 * (v.dx * v.dx + v.dy * v.dy) + Self.gravity * s.length * (1 - cos(angle))
            if !resting && abs(angle) < 0.35 && energy < Self.gravity * s.length * (1 - cos(s.height)) {
                // From a standstill the first push goes the way it's facing.
                let direction = abs(along) > 4 ? (along > 0 ? 1.0 : -1.0) : s.heading
                let push = direction * pump * h
                v.dx += tangent.dx * push
                v.dy += tangent.dy * push
            }
            // Reel towards the length this strand likes, the way it settles after a long catch.
            s.length += (s.reach - s.length) * min(1, 1.5 * h)
        }

        hands = CGPoint(x: hands.x + v.dx * h, y: hands.y + v.dy * h)

        if let anchor = s.anchor {
            let dx = Double(hands.x - anchor.x), dy = Double(hands.y - anchor.y)
            let distance = max((dx * dx + dy * dy).squareRoot(), 0.0001)
            if distance > s.length {
                let nx = dx / distance, ny = dy / distance
                hands = CGPoint(x: anchor.x + nx * s.length, y: anchor.y + ny * s.length)
                let outward = v.dx * nx + v.dy * ny
                if outward > 0 { v.dx -= nx * outward; v.dy -= ny * outward }
            }
        }
        // Never up into the black.
        if Double(hands.y) < edge.y + 4 { hands.y = edge.y + 4; v.dy = max(v.dy, 0) }
        s.hands = hands
        s.velocity = v

        // The body hangs along the silk; in the air it straightens up.
        let lean = ropeAngle ?? 0.15 * s.heading
        s.bodyAngle += (lean - s.bodyAngle) * min(1, 14 * h)

        if let anchor = s.anchor, let angle = ropeAngle {
            let forward = angle * s.heading
            let goingForward = v.dx * s.heading > 0
            if let destination = s.destination {
                if !s.atDestination && s.reaching <= 0 && abs(Double(anchor.x) - destination) < 25 {
                    s.atDestination = true
                    startRest(s)
                }
                let behind = (destination - Double(anchor.x)) * s.heading < 0
                if behind {
                    // The way is behind it: turn round at the top of the forward swing, never mid-arc.
                    if forward > 0.3 && !goingForward { s.heading = -s.heading }
                    return
                }
            }
            if !resting && forward >= s.release && goingForward {
                // Let go: the strand stays stuck to the edge and drops loose.
                if let held = s.strands.lastIndex(where: { $0.held }) { s.strands[held].held = false }
                s.anchor = nil
                s.airborne = 0
                s.flightPose = Self.flightPoses.randomElement(using: &s.rng) ?? "fly-superhero"
                s.aim = s.random(0.8...1.6)
                s.longShot = s.chance(0.25)
                s.spin = 0
                s.spinRate = 2 * Double.pi / 0.5
            }
            return
        }

        s.airborne += h
        if s.flightPose == "fly-flip" { s.spin = min(2 * Double.pi, s.spin + s.spinRate * h) }
        if s.shot == nil, s.airborne >= Self.shotDelay {
            // Aim up to the edge ahead, as far as the edge allows and never well past the destination.
            let drop = max(0, Double(hands.y) - edge.y)
            var ahead: Double
            if s.far {
                // A hop that covers ground, kept short enough for the strand to reach.
                ahead = min(s.random(60...140), (max(0, 95 * 95 - drop * drop)).squareRoot())
            } else if s.longShot {
                ahead = s.random(32...50)
            } else {
                ahead = ((max(0, s.reach * s.reach - drop * drop)).squareRoot() + 6) * s.aim
            }
            if let destination = s.destination {
                ahead = min(ahead, max((destination - Double(hands.x)) * s.heading + 20, 6))
            }
            let x = min(max(Double(hands.x) + s.heading * ahead, edge.lo), edge.hi)
            let target = CGPoint(x: x, y: edge.y)
            s.shot = Shot(target: target)
            // Spend the rest of the flight finishing the turn.
            let dx = Double(target.x - hands.x), dy = Double(target.y - hands.y)
            let flight = max((dx * dx + dy * dy).squareRoot() / Self.shotSpeed, 0.1)
            s.spinRate = (2 * Double.pi - s.spin) / flight
        }
        if var shot = s.shot {
            shot.travelled += Self.shotSpeed * h
            let dx = Double(shot.target.x - hands.x), dy = Double(shot.target.y - hands.y)
            let distance = (dx * dx + dy * dy).squareRoot()
            if shot.travelled >= distance || s.airborne > 1.2 {
                // Stuck: swing from here on a strand as long as the gap, then reel it in.
                s.anchor = shot.target
                s.length = min(max(distance, Self.shortest), Self.longest)
                s.strands.append(Strand(from: shot.target, to: hands, length: s.length))
                s.shot = nil
                rollStrand(s)
                s.reaching = 0.22
            } else {
                s.shot = shot
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
            // Cards keep their full height and the conversation above gives way; squeezed, the
            // wrapped option text spills out of its buttons and overlaps.
            if let permission {
                PermissionCard(model: model, agent: agent, request: permission)
                    .fixedSize(horizontal: false, vertical: true)
                    .layoutPriority(1)
            } else if let question {
                QuestionCard(model: model, agent: agent, question: question)
                    .fixedSize(horizontal: false, vertical: true)
                    .layoutPriority(1)
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
