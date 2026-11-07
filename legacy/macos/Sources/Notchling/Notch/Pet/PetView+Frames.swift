import SwiftUI

extension PetView {
    private func smoothstep(_ low: Double, _ high: Double, _ value: Double) -> Double {
        let t = min(max((value - low) / (high - low), 0), 1)
        return t * t * (3 - 2 * t)
    }

    /// The swing drawing for how far out it is, measured in the direction it's heading: -0.8 (legs
    /// tucked back) to +0.6 (kicked forward) maps onto the arc, and the drawing nearest that spot in
    /// `swingPositions` shows. It lets go at 0.45–0.8
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

    /// Whichever of two neighbouring drawings is nearer, `mix` of the way from one to the other.
    /// They cut rather than blend, since two half-opaque drawings overlap as a blurred double.
    static func between(_ from: String, _ to: String, _ mix: Double) -> [Layer] {
        [Layer(name: mix < 0.5 ? from : to, opacity: 1)]
    }

    /// A looping sequence at `rate` drawings a second.
    static func loop(_ frames: [String], rate: Double, time: Double) -> [Layer] {
        let place = time * rate
        let index = Int(place) % frames.count
        return between(frames[index], frames[(index + 1) % frames.count], place - place.rounded(.down))
    }

    /// The spot the tabs gather it to, under the camera.
    var middle: CGPoint {
        overlay(CGPoint(x: 0, y: model.notchSize.height + TabBar.height / 2 - Self.bodyHeight / 2))
    }

    /// Where silk can stick: the bottom of the menu bar across the whole screen, in the
    /// camera-centred frame (x = 0 under the camera, y = 0 at the top of the screen).
    var edge: (lo: Double, hi: Double, y: Double) {
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
            // Before the shot leaves (0.07 s) it already shows the shot's first drawing: a flight pose
            // there flashed for a few frames and read as a jerk.
            return ("flight", [Layer(name: "shoot-1", opacity: 1)])
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

    /// A different sequence cuts straight in; fading it over the old one showed two poses at once.
    func show(_ s: Swing, _ wanted: (sequence: String, layers: [Layer]), at t: Double, by dt: Double) {
        if wanted.sequence != s.sequence {
            s.sequence = wanted.sequence
            s.sequenceStart = t
        }
        s.layers = wanted.layers
    }

    func shownLayers(_ s: Swing) -> [Layer] { s.layers }

    /// Rolls the look of the next strand. Far from where it's heading it takes long, high swings
    /// to cover ground; near it, short ones to land precisely, and sometimes a rest.
    func rollStrand(_ s: Swing) {
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

    func startRest(_ s: Swing) {
        s.restLeft = s.random(1.5...4)
        // Held for the whole rest, so the ball and cheer poses show long enough to read.
        let calm = model.needsCount == 0 && s.restLeft >= 3
        s.restPose = calm ? "sleepy" : ["wave", "wave", "hang-cheer", "hang-ball"].randomElement(using: &s.rng) ?? "wave"
    }

    /// Somewhere else along the bar, at least 250 pt from where it is (or the farther end if the
    /// bar is too short for that), so it always seems to be going somewhere.
    func pickDestination(_ s: Swing, _ edge: (lo: Double, hi: Double, y: Double)) {
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
    func overlay(_ point: CGPoint) -> CGPoint {
        CGPoint(x: point.x + shape.width / 2, y: point.y)
    }

    /// Moves every strand on: the held one follows the hands, loose ones spring up and fade.
    func updateStrands(_ s: Swing, _ dt: Double) {
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

    func step(to t: Double, moving: Bool) -> Frame {
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
}
