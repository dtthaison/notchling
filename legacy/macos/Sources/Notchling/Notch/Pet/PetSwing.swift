import SwiftUI

extension PetView {
    /// A strand of silk as a chain of points. `previous` holds last frame's points, which is what
    /// gives every point its inertia. The first point is stuck to the edge; while held, the last
    /// point is pinned to the hands, and once let go it hangs free and the strand fades.
    struct Strand {
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
    struct Shot {
        let target: CGPoint
        var travelled = 0.0
    }

    /// A ballistic trip between two seats (feet positions), in legs of crouch, flight and landing.
    /// Time is accumulated, not read off the clock, so a stalled frame never makes it leap.
    struct Hop {
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
    struct Act {
        let name: String
        let length: Double
        var elapsed = 0.0
    }

    /// Where it is in its day: on silk, sitting on a seat, dropping onto one off a swing, or
    /// springing up to the bar (to swing to a new seat, or when a session starts).
    enum Phase { case swing, seated, hopping, rising }

    /// Everything that carries over between frames.
    final class Swing: ObservableObject {
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
        /// The drawing of the sequence playing now.
        var layers: [Layer] = []
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
}
