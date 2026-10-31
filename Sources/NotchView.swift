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
            if model.showPet {
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
/// peek doesn't move it along the bar. It is drawn from frame sequences: the swing frame follows
/// the swing angle, running, waving and dozing loop on a clock, and the web shot is paced over the
/// time the strand takes to stick. Neighbouring drawings blend into each other by how far along the
/// motion is, and a different sequence fades in over the last one.
/// It is drawn by Core Animation layers (see `PetLayer.swift`) on a 60 Hz display link that runs only
/// while it moves, so a tick moves layers instead of laying out and redrawing views.
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

    static let flightPoses = ["fly-superhero", "fly-flip", "fly-star"]
    /// Frames per drawn sequence: the swing from legs tucked back to kicked forward, a wave and a
    /// doze to rest in, running in the air, and the web shot ending with both hands up to catch.
    static let sequences = ["swing": 12, "wave": 5, "sleepy": 4, "run": 8, "shoot": 11]
    /// Where each swing frame sits along the arc, in key-frame units (0 = legs tucked back, 7 = kicked
    /// forward). Keys are whole numbers, inbetweens halves; a missing inbetween just leaves a gap.
    static let swingPositions: [Double] = [0, 0.5, 1, 2, 3, 4, 4.5, 5, 5.5, 6, 6.5, 7]
    /// Where the fist is in drawings (240 px, from the top left) whose hand isn't at the top centre,
    /// so the silk comes out of it instead of the air above the head.
    static let grips: [String: CGPoint] = [
        "shoot-1": CGPoint(x: 180, y: 105), "shoot-2": CGPoint(x: 178, y: 105), "shoot-3": CGPoint(x: 195, y: 100),
        "shoot-4": CGPoint(x: 200, y: 60), "shoot-5": CGPoint(x: 205, y: 60), "shoot-6": CGPoint(x: 205, y: 58),
        "shoot-7": CGPoint(x: 210, y: 55), "shoot-8": CGPoint(x: 210, y: 55), "shoot-9": CGPoint(x: 205, y: 60),
        "shoot-10": CGPoint(x: 132, y: 24), "shoot-11": CGPoint(x: 134, y: 24),
    ]
    static let poseNames = flightPoses + ["hang-ball", "hang-cheer"]
        + sequences.flatMap { name, count in (1...count).map { "\(name)-\($0)" } }
    /// The sitting-on-a-tab drawings: a sit loop, a look and a yawn to break it, a doze, and the hop
    /// between seats. Kept out of `poseNames` so the app still draws while some are missing.
    static let idleSequences = ["sit": 6, "look": 4, "yawn": 5, "doze": 4, "hop": 5]
    static let idleNames = idleSequences.flatMap { name, count in (1...count).map { "\(name)-\($0)" } }

    /// Every pose frame by name. Each is pre-rotated so its centre of mass sits under the hands,
    /// which are at the top centre, and all face right. The probe build has no bundle, so a missing
    /// frame falls back to ✻.
    static let poses: [String: NSImage] = Dictionary(uniqueKeysWithValues: (poseNames + idleNames).compactMap { name in
        Bundle.main.url(forResource: name, withExtension: "png")
            .flatMap(NSImage.init(contentsOf:))
            .map { (name, $0) }
    })
    /// Whether every swing drawing loaded; the idle ones may be missing without hiding the pet.
    static let complete = poseNames.allSatisfy { poses[$0] != nil }

    /// An idle drawing, or while it's missing the swing one that stands in: sitting for the sit
    /// loop and its breaks, the star flight for hops.
    static func idlePose(_ name: String) -> String {
        guard poses[name] == nil else { return name }
        return name.hasPrefix("hop") ? "fly-star" : "swing-6"
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
                // A little stiffness: points two apart never come closer than most of two segments,
                // so slack silk sags in a soft curve instead of folding into a hook.
                for i in 0..<(points.count - 2) {
                    let a = points[i], c = points[i + 2]
                    let dx = c.x - a.x, dy = c.y - a.y
                    let length = max((dx * dx + dy * dy).squareRoot(), 0.0001)
                    let least = rest * 1.6
                    guard length < least else { continue }
                    let fix = (least - length) / length
                    let aFree = i > 0, cFree = i + 2 <= last
                    let share = aFree && cFree ? 0.25 : 0.5
                    if aFree { points[i] = CGPoint(x: a.x - dx * fix * share, y: a.y - dy * fix * share) }
                    if cFree { points[i + 2] = CGPoint(x: c.x + dx * fix * share, y: c.y + dy * fix * share) }
                }
            }
        }
    }

    /// A strand on its way from the hands to the spot ahead on the edge.
    fileprivate struct Shot {
        let target: CGPoint
        var travelled = 0.0
    }

    /// A ballistic trip between two seats (feet positions), in legs of crouch, flight and landing.
    /// Time is accumulated, not read off the clock, so a stalled frame never makes it leap.
    fileprivate struct Hop {
        let from: CGPoint
        let to: CGPoint
        let legs: Int
        /// Springing up to the bar: stops once airborne, with no landing.
        let rising: Bool
        var elapsed = 0.0
        let legTime: Double

        /// Where in a leg the flight starts and ends; before and after it the body only crouches or settles.
        static let takeoff = 0.2, touchdown = 0.85

        init(from: CGPoint, to: CGPoint, legs: Int, rising: Bool, airborne: Bool) {
            self.from = from
            self.to = to
            self.legs = legs
            self.rising = rising
            let leg = Double(to.x - from.x) / Double(legs)
            legTime = 0.55 + 0.15 * min(abs(leg), 400) / 400
            // Coming off a swing it is already in the air: skip the crouch.
            if airborne { elapsed = Self.takeoff * legTime }
        }

        var total: Double { (Double(legs - 1) + (rising ? Self.touchdown : 1)) * legTime }
        var done: Bool { elapsed >= total }
        /// How far through the current leg, 0...1.
        var phase: Double { min(elapsed / legTime - Double(min(Int(elapsed / legTime), legs - 1)), 1) }

        var position: CGPoint {
            let leg = min(Int(elapsed / legTime), legs - 1)
            let flight = min(max((phase - Self.takeoff) / (Self.touchdown - Self.takeoff), 0), 1)
            let a = Double(leg) / Double(legs), b = Double(leg + 1) / Double(legs)
            let x0 = Double(from.x + (to.x - from.x) * a), x1 = Double(from.x + (to.x - from.x) * b)
            let y0 = Double(from.y + (to.y - from.y) * a), y1 = Double(from.y + (to.y - from.y) * b)
            // Never so high that its head leaves the top of the screen.
            let apex = min(12 + 0.08 * min(abs(x1 - x0), 400), max(0, min(y0, y1) - Double(PetView.bodyHeight) - 2))
            return CGPoint(x: x0 + (x1 - x0) * flight, y: y0 + (y1 - y0) * flight - 4 * apex * flight * (1 - flight))
        }
    }

    /// A one-off break in the sit loop.
    fileprivate struct Act {
        let name: String
        let length: Double
        var elapsed = 0.0
    }

    /// Where it is in its day: on silk, sitting on a seat, dropping onto one off a swing, or
    /// springing up to the bar (to swing to a new seat, or when a session starts).
    fileprivate enum Phase { case swing, seated, hopping, rising }

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
        var reach = PetView.reach
        var height = 1.15
        var release = 0.55
        /// Seconds left of a rest on this strand: no pumping, drag up, waving or dozing.
        var restLeft = 0.0
        var restPose = "wave"
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

        /// The sequence playing, when it started, and the frame showing, so loops start at frame 1.
        var sequence = ""
        var sequenceStart = 0.0
        /// The layers of the sequence playing now, and of the one fading out under it.
        var layers: [Layer] = []
        var outgoing: [Layer] = []
        var fadeIn = 1.0
        /// How long the shot in flight is expected to take, which paces the shoot frames.
        var shotTime = 0.3

        // Idle: sitting on a tab. `feet` is the seat it stands on; `hands` stays 37.5 pt above it.
        var phase = Phase.swing
        var feet = CGPoint.zero
        var seat: CGPoint?
        var hop: Hop?
        var act: Act?
        /// Seconds of idle time (frozen while the tabs are open), since the last seat change, and
        /// until the next break in the sit loop.
        var idleTime = 0.0
        var sinceSeat = 0.0
        var nextAct = 10.0
        /// How long it has waited on a strand to let go near its seat.
        var leaveWait = 0.0
        /// Let go of the silk to drop onto the seat.
        var landing = false

        func random(_ range: ClosedRange<Double>) -> Double { Double.random(in: range, using: &rng) }
        func chance(_ probability: Double) -> Bool { random(0...1) < probability }
    }

    /// One pose drawn at some opacity, in drawing order.
    struct Layer {
        let name: String
        let opacity: Double
    }

    /// What one frame draws.
    struct Frame {
        let hands: CGPoint
        let angle: Double
        let heading: Double
        let layers: [Layer]
        let spin: Double
        let strands: [(points: [CGPoint], opacity: Double)]
        let shot: (from: CGPoint, to: CGPoint)?
        /// Sitting still, so the layer can tick slower.
        let settled: Bool
    }

    @StateObject private var swing = Swing()

    var body: some View {
        let gathered = model.state == .list
        let resting = model.state == .collapsed
        let moving = (model.petWalking || (model.petIdle && !Self.reduceMotion)) && resting
        // The layers cover the whole bar from its left edge, and are drawn by Core Animation, so a
        // tick moves layers instead of laying out and redrawing views.
        let left = shape.width / 2 - model.screenSpan.left
        // When the tabs open it slides to the middle; only that change animates, not every tick.
        let slide = gathered ? CGSize(width: middle.x - gatheredHands.x, height: middle.y - gatheredHands.y) : .zero
        PetLayerHost(moving: moving, gathered: gathered, needs: model.needsCount, origin: left,
                     frameAt: { time, moving in step(to: time, moving: moving) })
            .frame(width: model.screenSpan.left + model.screenSpan.right, height: shape.height + 200)
            .offset(x: left + slide.width, y: slide.height)
            .allowsHitTesting(false)
            .animation(.spring(response: 0.3, dampingFraction: 0.85), value: gathered)
            // Only out on its own while the notch rests; it fades once it reaches the tabs.
            .opacity(resting ? 1 : 0)
            .animation(resting ? .easeIn(duration: 0.15).delay(0.15) : .easeOut(duration: 0.12).delay(gathered ? 0.24 : 0),
                       value: resting)
    }

    /// Where its hands will be once the tabs open and it hangs straight from its strand.
    private var gatheredHands: CGPoint {
        let s = swing
        guard let anchor = s.anchor ?? s.shot?.target ?? s.strands.last?.points.first else { return middle }
        return overlay(CGPoint(x: anchor.x, y: anchor.y + s.reach))
    }

    static var bodyWidth: CGFloat {
        guard let first = poses["swing-4"], first.size.height > 0 else { return bodyHeight * 0.6 }
        return bodyHeight * first.size.width / first.size.height
    }

    private func smoothstep(_ low: Double, _ high: Double, _ value: Double) -> Double {
        let t = min(max((value - low) / (high - low), 0), 1)
        return t * t * (3 - 2 * t)
    }

    /// The swing drawings for how far out it is, measured in the direction it's heading: -0.8 (legs
    /// tucked back) to +0.6 (kicked forward) maps onto the arc, and the two drawings either side of
    /// that spot in `swingPositions` blend by how close it is to each. It lets go at 0.45–0.8
    /// forward, so the front end is short of that, or the full kick would never show. The pendulum
    /// already lingers at the ends and rushes through the bottom, so the drawings bunch where they
    /// should.
    private func swingFrame(_ angle: Double) -> (sequence: String, layers: [Layer]) {
        let low = -0.8, high = 0.6
        let position = (min(max(angle, low), high) - low) / (high - low) * 7
        let spots = Self.swingPositions
        let upper = spots.firstIndex { $0 >= position } ?? spots.count - 1
        let lower = max(upper - 1, 0)
        let span = spots[upper] - spots[lower]
        let mix = span > 0 ? (position - spots[lower]) / span : 0
        return ("swing", Self.between("swing-\(lower + 1)", "swing-\(upper + 1)", mix))
    }

    /// One drawing turning into the next, `mix` of the way there. The new one comes up over the
    /// first half and the old one goes over the second, so one of them is always fully opaque (the
    /// body never turns see-through) and the parts of the old drawing that stick out fade away
    /// instead of vanishing at the next pair.
    static func between(_ from: String, _ to: String, _ mix: Double) -> [Layer] {
        let mix = min(max(mix, 0), 1)
        if from == to || mix < 0.01 { return [Layer(name: from, opacity: 1)] }
        if mix > 0.99 { return [Layer(name: to, opacity: 1)] }
        return [Layer(name: from, opacity: min(1, 2 * (1 - mix))), Layer(name: to, opacity: min(1, 2 * mix))]
    }

    /// A looping sequence at `rate` drawings a second, blending each into the next.
    static func loop(_ frames: [String], rate: Double, time: Double) -> [Layer] {
        let place = time * rate
        let index = Int(place) % frames.count
        return between(frames[index], frames[(index + 1) % frames.count], place - place.rounded(.down))
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

    /// The frame it wants right now, and the sequence it belongs to: the airborne poses by phase of
    /// the flight, then a rest, then the strand's style while swinging. `time` is how long the
    /// sequence has been playing, so loops start from their first frame.
    private func wantedFrame(_ s: Swing, moving: Bool, time: Double) -> (sequence: String, layers: [Layer]) {
        let angle = s.bodyAngle * s.heading
        guard moving else { return swingFrame(angle) }
        guard s.anchor != nil else {
            if s.shot != nil {
                let place = min(time / s.shotTime, 1) * 9
                let index = min(Int(place), 8)
                return ("shoot", Self.between("shoot-\(index + 1)", "shoot-\(index + 2)", place - Double(index)))
            }
            return ("flight", [Layer(name: s.flightPose, opacity: 1)])
        }
        if s.reaching > 0 { return ("reach", [Layer(name: "shoot-11", opacity: 1)]) }
        if s.restLeft > 0 {
            switch s.restPose {
            case "sleepy": return ("sleepy", Self.loop((1...4).map { "sleepy-\($0)" }, rate: 3, time: time))
            // A loop that plays forwards then back.
            case "wave": return ("wave", Self.loop([1, 2, 3, 4, 5, 4, 3, 2].map { "wave-\($0)" }, rate: 8, time: time))
            default: return (s.restPose, [Layer(name: s.restPose, opacity: 1)])
            }
        }
        // Only the swing drawings while it swings: poses that flicked in on an angle or speed
        // threshold showed for a fraction of a second and read as a different character.
        return swingFrame(angle)
    }

    /// Within a sequence the drawings blend straight into each other; when a different sequence
    /// starts, it fades in over the last look of the old one.
    private func show(_ s: Swing, _ wanted: (sequence: String, layers: [Layer]), at t: Double, by dt: Double) {
        if wanted.sequence != s.sequence {
            // What is on screen now, so a change mid-fade doesn't drop the half-faded drawing.
            s.outgoing = shownLayers(s)
            s.fadeIn = s.layers.isEmpty ? 1 : 0
            s.sequence = wanted.sequence
            s.sequenceStart = t
        }
        s.fadeIn = min(1, s.fadeIn + dt / Self.fadeTime)
        s.layers = wanted.layers
    }

    /// What to draw: the incoming sequence over the outgoing one while it fades in.
    private func shownLayers(_ s: Swing) -> [Layer] {
        guard s.fadeIn < 1 else { return s.layers }
        return s.outgoing + s.layers.map { Layer(name: $0.name, opacity: $0.opacity * s.fadeIn) }
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
        s.restLeft = 0
        if !s.far && s.chance(0.2) { startRest(s) }
    }

    private func startRest(_ s: Swing) {
        s.restLeft = s.random(1.5...4)
        // Held for the whole rest, so the ball and cheer poses show long enough to read.
        let calm = model.needsCount == 0 && s.restLeft >= 3
        s.restPose = calm ? "sleepy" : ["wave", "wave", "hang-cheer", "hang-ball"].randomElement(using: &s.rng) ?? "wave"
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
        // One flying pose for the whole trip, not a new one every time it lets go.
        s.flightPose = Self.flightPoses.randomElement(using: &s.rng) ?? "fly-superhero"
    }

    /// All positions are kept in a frame centred on the camera, so they stay put on screen when the
    /// black shape grows or shrinks; this shifts one into the overlay's frame, whose origin is the
    /// shape's top-left corner.
    private func overlay(_ point: CGPoint) -> CGPoint {
        CGPoint(x: point.x + shape.width / 2, y: point.y)
    }

    /// Moves every strand on: the held one follows the hands, loose ones spring up and fade.
    private func updateStrands(_ s: Swing, _ dt: Double) {
        let hands = s.hands ?? .zero
        for i in s.strands.indices {
            if s.strands[i].held { s.strands[i].rest = s.length / Double(Strand.segments) }
            s.strands[i].simulate(dt, hands: hands)
            if !s.strands[i].held {
                // Let go, the silk springs back up to where it's stuck instead of dangling by the body.
                s.strands[i].rest = max(s.strands[i].rest - dt * 60, 0.5)
                s.strands[i].fade -= dt / 0.5
            }
        }
        s.strands.removeAll { $0.fade <= 0 }
    }

    private func step(to t: Double, moving: Bool) -> Frame {
        let s = swing
        let edge = self.edge
        if s.phase == .swing && model.petIdle {
            if s.hands == nil {
                // First thing it ever does: appear on its seat.
                let seat = seatPoint(edge)
                s.feet = seat
                s.seat = seat
                s.hands = CGPoint(x: seat.x, y: seat.y - Self.bodyHeight)
                s.phase = .seated
                s.nextAct = s.random(6...14)
            } else if s.anchor == nil && (s.landing || nearSeat(s, edge)) || Self.reduceMotion {
                // Close to the seat, or nothing may move: let go of the silk and drop onto it.
                let hands = s.hands ?? .zero
                s.feet = CGPoint(x: hands.x, y: hands.y + Self.bodyHeight)
                startHop(s, to: seatPoint(edge), airborne: s.anchor == nil)
            } else {
                // Swing over to the seat like to any destination.
                let seatX = Double(seatPoint(edge).x)
                if s.destination != seatX { s.atDestination = false }
                s.destination = seatX
            }
        }
        if s.phase != .swing { return stepPerched(s, t, moving: moving, edge: edge) }
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
            updateStrands(s, dt)
        } else if !moving, let anchor = s.anchor ?? s.shot?.target ?? s.strands.last?.points.first {
            // Still with Animate Pet off; otherwise ready on its strand, held up behind itself.
            hang(s, at: anchor, angle: model.petWalking ? -0.6 * s.heading : 0)
        }
        s.last = moving ? t : nil
        var wanted = wantedFrame(s, moving: moving, time: t - s.sequenceStart)
        if wanted.sequence != s.sequence { wanted = wantedFrame(s, moving: moving, time: 0) }
        show(s, wanted, at: t, by: fadeDt)

        let hands = s.hands ?? .zero
        let shot = s.shot.map { shot -> (CGPoint, CGPoint) in
            let dx = shot.target.x - hands.x, dy = shot.target.y - hands.y
            let distance = max((dx * dx + dy * dy).squareRoot(), 0.0001)
            let reach = min(shot.travelled / distance, 1)
            return (overlay(hands), overlay(CGPoint(x: hands.x + dx * reach, y: hands.y + dy * reach)))
        }
        return Frame(hands: overlay(hands), angle: s.bodyAngle, heading: s.heading, layers: shownLayers(s), spin: s.spin,
                     strands: s.strands.map { ($0.points.map(overlay), max(0, min(1, $0.fade))) }, shot: shot,
                     settled: false)
    }

    static var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    /// How long it sits on one seat before dozing off.
    static let dozeAfter = 120.0

    /// Where it wants to sit (feet position): on top of the perch, kept inside the screen; with no
    /// perch, on the bottom edge of the menu bar just right of the notch.
    private func seatPoint(_ edge: (lo: Double, hi: Double, y: Double)) -> CGPoint {
        guard let perch = model.perch else {
            return CGPoint(x: Double(model.notchSize.width) / 2 + 30, y: edge.y + Self.legHang)
        }
        return CGPoint(x: min(max(Double(perch.midX), edge.lo), edge.hi), y: Double(perch.minY) + Self.legHang)
    }

    /// How far its feet hang below what it sits on: in the sit drawings the seat is 40 px above the
    /// bottom of the 240 px frame, so the legs dangle over the front of the tab.
    static let legHang = 40.0 / 240 * Double(bodyHeight)

    /// Whether the hands are within 30 pt of the seat along the bar while idle.
    private func nearSeat(_ s: Swing, _ edge: (lo: Double, hi: Double, y: Double)) -> Bool {
        guard model.petIdle, let hands = s.hands else { return false }
        return abs(Double(hands.x - seatPoint(edge).x)) < 30
    }

    /// Starts a hop to `target`, or just puts it there when motion is reduced. Silk it holds is let go.
    private func startHop(_ s: Swing, to target: CGPoint, airborne: Bool) {
        for i in s.strands.indices { s.strands[i].held = false }
        s.anchor = nil
        s.shot = nil
        s.landing = false
        s.seat = target
        s.sinceSeat = 0
        s.act = nil
        s.nextAct = s.random(6...14)
        let from = s.feet
        let dx = Double(target.x - from.x), dy = Double(target.y - from.y)
        if abs(dx) > 4 { s.heading = dx >= 0 ? 1 : -1 }
        guard !Self.reduceMotion else {
            s.feet = target
            s.hop = nil
            s.phase = .seated
            return
        }
        let legs = min(3, 1 + Int((dx * dx + dy * dy).squareRoot() / 400))
        s.hop = Hop(from: from, to: target, legs: legs, rising: false, airborne: airborne)
        s.phase = .hopping
    }

    /// Spring up to a fresh strand on the bar, then swing as usual: when sessions start, or to
    /// swing over to a seat that moved far away (idle, so its destination is that seat).
    private func startRise(_ s: Swing, _ edge: (lo: Double, hi: Double, y: Double)) {
        let feet = s.feet
        s.hands = CGPoint(x: feet.x, y: feet.y - Self.bodyHeight)
        s.act = nil
        s.landing = false
        pickDestination(s, edge)
        if model.petIdle { s.destination = Double(seatPoint(edge).x) }
        s.heading = (s.destination ?? 0) >= Double(feet.x) ? 1 : -1
        rollStrand(s)
        let anchor = CGPoint(x: min(max(feet.x, edge.lo), edge.hi), y: edge.y)
        guard !Self.reduceMotion else {
            hang(s, at: anchor, angle: 0)
            s.phase = .swing
            return
        }
        let landing = CGPoint(x: anchor.x, y: anchor.y + s.reach + Self.bodyHeight)
        s.hop = Hop(from: feet, to: landing, legs: 1, rising: true, airborne: false)
        s.phase = .rising
    }

    /// Everything off the silk: sitting on the seat, hopping to a new one, or springing up to the
    /// bar. Time only runs while `moving`; with the tabs open it stays as it is.
    private func stepPerched(_ s: Swing, _ t: Double, moving: Bool, edge: (lo: Double, hi: Double, y: Double)) -> Frame {
        var dt = 0.0
        if moving, let last = s.last { dt = min(max(t - last, 0), 0.05) }
        s.last = moving ? t : nil
        let idle = model.petIdle
        let live = model.state == .collapsed

        if live {
            switch s.phase {
            case .seated, .hopping:
                let target = seatPoint(edge)
                let gap = hypot(Double(target.x - s.feet.x), Double(target.y - s.feet.y))
                if !idle {
                    startRise(s, edge)
                } else if s.phase == .hopping {
                    // Still dropping onto the seat: aim the drop at where it is now.
                    if let seat = s.seat, hypot(Double(target.x - seat.x), Double(target.y - seat.y)) > 6 {
                        s.feet = s.hop.map { $0.position } ?? s.feet
                        startHop(s, to: target, airborne: true)
                    }
                } else if Self.reduceMotion {
                    s.feet = target
                    s.seat = target
                } else if gap > 40 {
                    // Far: swing over to it from the bar.
                    s.seat = target
                    startRise(s, edge)
                } else if gap > 0.5 {
                    // A nudge: slide along without travelling.
                    s.feet.x += (target.x - s.feet.x) * min(1, 12 * dt)
                    s.feet.y += (target.y - s.feet.y) * min(1, 12 * dt)
                    s.seat = target
                    if gap > 3 { s.sinceSeat = 0 }
                }
            case .rising, .swing: break
            }
            if s.phase == .seated || s.phase == .hopping { s.idleTime += dt }
            if s.phase == .seated { advanceSitting(s, by: dt) }
            if var hop = s.hop {
                hop.elapsed += dt
                if hop.done {
                    s.hop = nil
                    s.feet = hop.to
                    if s.phase == .rising {
                        let anchor = CGPoint(x: hop.to.x, y: Double(edge.y))
                        hang(s, at: anchor, angle: 0)
                        s.phase = .swing
                    } else {
                        s.phase = .seated
                    }
                } else {
                    s.hop = hop
                    s.feet = hop.position
                }
            }
        }
        if s.phase != .swing { s.hands = CGPoint(x: s.feet.x, y: s.feet.y - Self.bodyHeight) }
        updateStrands(s, dt)

        var wanted = idleFrame(s)
        if wanted.sequence != s.sequence { wanted = idleFrame(s, fresh: true) }
        show(s, wanted, at: s.idleTime, by: dt)
        let hands = s.hands ?? .zero
        return Frame(hands: overlay(hands), angle: 0, heading: s.heading, layers: shownLayers(s), spin: 0,
                     strands: s.strands.map { ($0.points.map(overlay), max(0, min(1, $0.fade))) }, shot: nil,
                     settled: s.phase == .seated && moving)
    }

    /// The sit loop with a look or a yawn now and then, and a doze when nothing has changed for a while.
    private func advanceSitting(_ s: Swing, by dt: Double) {
        s.sinceSeat += dt
        if var act = s.act {
            act.elapsed += dt
            if act.elapsed >= act.length {
                s.act = nil
                s.nextAct = s.random(6...14)
            } else {
                s.act = act
            }
        } else if s.sinceSeat < Self.dozeAfter {
            s.nextAct -= dt
            if s.nextAct <= 0 {
                let look = s.chance(0.5)
                s.act = Act(name: look ? "look" : "yawn", length: look ? 6.0 / 5 : 4.0 / 4)
            }
        }
    }

    /// The idle drawing for now, and its sequence. `fresh` plays it from its first frame.
    private func idleFrame(_ s: Swing, fresh: Bool = false) -> (sequence: String, layers: [Layer]) {
        func names(_ name: String, _ order: [Int]) -> [String] { order.map { Self.idlePose("\(name)-\($0)") } }
        func once(_ frames: [String], rate: Double, time: Double) -> [Layer] {
            let place = min(time * rate, Double(frames.count - 1))
            let index = min(Int(place), frames.count - 2)
            return Self.between(frames[index], frames[index + 1], place - Double(index))
        }
        let time = fresh ? 0 : s.idleTime - s.sequenceStart
        switch s.phase {
        case .hopping, .rising:
            guard let hop = s.hop else { return ("hop", [Layer(name: Self.idlePose("hop-5"), opacity: 1)]) }
            let keys: [(at: Double, name: String)] = [(0, "hop-1"), (0.15, "hop-1"), (0.25, "hop-2"), (0.45, "hop-3"),
                                                      (0.72, "hop-4"), (0.86, "hop-5"), (1, "hop-5")]
            // Rising stops at the takeoff drawings; it never comes down.
            let place = hop.rising ? min(hop.phase, 0.45) : hop.phase
            let upper = keys.firstIndex { $0.at >= place } ?? keys.count - 1
            let lower = max(upper - 1, 0)
            let span = keys[upper].at - keys[lower].at
            let mix = span > 0 ? (place - keys[lower].at) / span : 0
            return ("hop", Self.between(Self.idlePose(keys[lower].name), Self.idlePose(keys[upper].name), mix))
        case .seated, .swing:
            if Self.reduceMotion { return ("sit", [Layer(name: Self.idlePose("sit-1"), opacity: 1)]) }
            if s.sinceSeat >= Self.dozeAfter { return ("doze", Self.loop(names("doze", [1, 2, 3, 4]), rate: 2, time: time)) }
            if let act = s.act {
                switch act.name {
                case "look": return ("look", once(names("look", [1, 2, 3, 4, 3, 2, 1]), rate: 5, time: act.elapsed))
                default: return ("yawn", once(names("yawn", [1, 2, 3, 4, 5]), rate: 4, time: act.elapsed))
                }
            }
            return ("sit", Self.loop(names("sit", [1, 2, 3, 4, 5, 6]), rate: 6, time: time))
        }
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
        // Idle: no rests; near its seat, no turning round either, just swing out and let go onto it.
        let leaving = nearSeat(s, edge)
        if model.petIdle { s.restLeft = 0 }
        if leaving && s.anchor != nil { s.leaveWait += h } else if !leaving { s.leaveWait = 0 }
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
                if behind && !leaving {
                    // The way is behind it: turn round at the top of the forward swing, never mid-arc.
                    if forward > 0.3 && !goingForward { s.heading = -s.heading }
                    return
                }
            }
            let forced = leaving && s.leaveWait > 2 && forward > 0.15 && goingForward
            if !resting && ((forward >= s.release && goingForward) || forced) {
                // Let go: the strand stays stuck to the edge and drops loose.
                if let held = s.strands.lastIndex(where: { $0.held }) { s.strands[held].held = false }
                s.anchor = nil
                if leaving { s.landing = true }
                s.airborne = 0
                s.aim = s.random(0.8...1.6)
                s.longShot = s.chance(0.25)
                s.spin = 0
                s.spinRate = 2 * Double.pi / 0.5
            }
            return
        }

        s.airborne += h
        if s.flightPose == "fly-flip" { s.spin = min(2 * Double.pi, s.spin + s.spinRate * h) }
        if s.shot == nil, s.airborne >= Self.shotDelay, !s.landing {
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
            s.shotTime = flight
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

extension View {
    /// Fades the cut-off edge of a scrolling card so it reads as "more below".
    func moreBelow(_ show: Bool) -> some View {
        overlay(alignment: .bottom) {
            if show {
                LinearGradient(colors: [.black.opacity(0), .black], startPoint: .top, endPoint: .bottom)
                    .frame(height: 14)
                    .allowsHitTesting(false)
            }
        }
    }
}

/// Natural height of a `ScrollingBody`'s content; its own key, so it doesn't mix with `CardHeightKey`.
struct BodyHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

/// Content at its full height up to `limit`, scrolling beyond that, so a long question keeps its
/// header and answer box in view. Back at the top whenever `resetKey` changes.
struct ScrollingBody<Content: View>: View {
    let limit: CGFloat
    let resetKey: Int
    @ViewBuilder var content: Content
    @State private var height: CGFloat = 0

    var body: some View {
        let overflows = height > limit
        ScrollView(.vertical) {
            content.fixedSize(horizontal: false, vertical: true)
                .background(GeometryReader { Color.clear.preference(key: BodyHeightKey.self, value: $0.size.height) })
                .padding(.bottom, overflows ? 14 : 0)
        }
        .onPreferenceChange(BodyHeightKey.self) { height = $0 }
        .scrollIndicators(overflows ? .visible : .hidden)
        .id(resetKey)
        .frame(height: height > 0 ? min(height, limit) : nil)
        .moreBelow(overflows)
    }
}

struct PeekView: View {
    @ObservedObject var model: NotchModel
    let agent: Agent

    var body: some View {
        Group {
            if let notice = model.notice, notice.agentID == agent.id {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "gauge.with.dots.needle.67percent").foregroundColor(.orange)
                    Text(notice.text)
                        .font(.system(size: 12.5)).foregroundColor(.white.opacity(0.9))
                        .lineLimit(3)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .contentShape(Rectangle())
                .onTapGesture { model.go(.collapsed) }
            } else if let request = model.permission(agent.id) {
                PermissionCard(model: model, agent: agent, request: request, showsAgent: true)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else if let q = model.question(agent.id) {
                QuestionCard(model: model, agent: agent, question: q, showsAgent: true, bodyLimit: QuestionCard.peekBodyLimit)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else if model.canReplyInPeek(agent.id) {
                ReplyPeek(model: model, agent: agent)
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
                    if let text = model.peekText(agent).map(Self.lastParagraph) {
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
        .onExitCommand { model.go(.collapsed) }
    }

    /// The last paragraph is usually the question or the summary.
    static func lastParagraph(_ text: String) -> String {
        let paragraph = text.components(separatedBy: "\n\n")
            .last { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? text
        return String(paragraph.prefix(400))
    }
}

/// A peek for an agent waiting on a plain reply: its whole last message (scrolling when long),
/// the reply it suggests, and a box to answer in.
struct ReplyPeek: View {
    @ObservedObject var model: NotchModel
    let agent: Agent

    var body: some View {
        let text = model.peekText(agent) ?? ""
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Circle().fill(agent.color).frame(width: 8, height: 8)
                Text(agent.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Spacer()
                Tag(text: agent.host)
                Button("Open ›") { model.open(agent.id) }
                    .buttonStyle(.plain).font(.system(size: 11)).foregroundColor(.secondary)
                    .pointerCursor()
                HideButton(model: model)
            }
            .frame(height: 22)
            if !text.isEmpty {
                ScrollingBody(limit: 260, resetKey: 0) {
                    Text(Bubble.markdown(text))
                        .font(.system(size: 13))
                        .foregroundColor(.white.opacity(0.9))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            if let suggested = model.suggestion(for: agent) {
                // Spelled out in full: as a placeholder it was cut off and read like a typed answer.
                Button { model.draft = suggested } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("Suggested reply").font(.system(size: 11, weight: .semibold)).foregroundColor(.orange)
                        Text(suggested).font(.system(size: 12.5)).foregroundColor(.white.opacity(0.85))
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.orange.opacity(0.15)))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .pointerCursor()
                .help("Click, or press Tab, to put it in the box")
            }
            PeekReplyBox(model: model, agent: agent)
        }
        .padding(.bottom, 4)
    }
}

/// Folds a peek away; what it showed stays in the list.
struct HideButton: View {
    @ObservedObject var model: NotchModel

    var body: some View {
        Button { model.go(.collapsed) } label: {
            Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
        }
        .buttonStyle(.plain).foregroundColor(.secondary)
        .pointerCursor()
        .help("Hide; it stays in the list (Esc)")
    }
}

/// The one-line reply box under a `ReplyPeek`; Tab takes the suggested reply.
struct PeekReplyBox: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                TextField("Reply…", text: $model.draft)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .focused($focused)
                    .onSubmit { model.send() }
                    .padding(.horizontal, 10)
                    .frame(height: 32)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.09)))
                    .disabled(model.sending)
                Button { model.send() } label: {
                    if model.sending {
                        ProgressView().controlSize(.small).frame(width: 24, height: 24)
                    } else {
                        Image(systemName: "arrow.up.circle.fill").font(.system(size: 24))
                    }
                }
                .buttonStyle(.plain)
                .pointerCursor()
                .foregroundColor(model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .secondary : .orange)
                .disabled(model.sending)
            }
            if let error = model.sendError {
                Text(error).font(.system(size: 11)).foregroundColor(.red).lineLimit(1)
            }
        }
        // Takes keys only once the pointer is over the peek, which makes the panel key.
        .onAppear { focused = true }
    }
}

// MARK: - Question

/// "Claude asks": the question with its options as buttons, ⌘1…⌘9 to pick, and a free-text answer.
struct QuestionCard: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    let question: PendingQuestion
    var showsAgent = false
    /// The question and its options scroll past this; the header and answer box stay put.
    var bodyLimit: CGFloat = 400
    @State private var typed = ""

    /// Keeps a peek's whole card within `NotchModel.maxCard` with room for the header and answer box.
    static let peekBodyLimit: CGFloat = 400
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
                if showsAgent {
                    Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                    HideButton(model: model)
                }
                if step > 0 {
                    Button("Back") { model.undoChoice() }.buttonStyle(.plain)
                        .font(.system(size: 11)).foregroundColor(.secondary)
                }
            }
            .frame(height: 22)

            ScrollingBody(limit: bodyLimit, resetKey: step) {
                VStack(alignment: .leading, spacing: 8) {
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
                }
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
    @State private var instead = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "lock.shield.fill").foregroundColor(.orange)
                Text("Permission needed").font(.system(size: 13, weight: .semibold)).foregroundColor(.orange)
                Spacer()
                if showsAgent {
                    Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                    HideButton(model: model)
                }
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
            option(request.canAlwaysAllow ? 3 : 2, "Deny", .red) { model.decide(.deny(nil), for: agent.id) }
            // Like the terminal's "No, and tell Claude what to do differently".
            TextField("Deny, and tell Claude what to do instead…", text: $instead)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .onSubmit {
                    let text = instead.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !text.isEmpty else { return }
                    model.decide(.deny(text), for: agent.id)
                }
                .padding(.horizontal, 12)
                .frame(height: 34)
                .background(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.12)))

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
    /// Leaves the header and a few lines of conversation above a long question.
    static let questionBodyLimit: CGFloat = 230

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
                QuestionCard(model: model, agent: agent, question: question, bodyLimit: Self.questionBodyLimit)
                    .fixedSize(horizontal: false, vertical: true)
                    .layoutPriority(1)
            } else if model.canReply(agent) {
                replyBox
            } else {
                HStack {
                    Text(agent.label == "Exited" ? "This session has exited. Open it to resume."
                         : agent.canReply ? "Open it in a terminal to reply there."
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
