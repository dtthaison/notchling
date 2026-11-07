import SwiftUI

extension PetView {
    static var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    /// How long it sits on one seat before dozing off.
    static let dozeAfter = 120.0

    /// Where it wants to sit (feet position): on top of the perch, kept inside the screen; with no
    /// perch, on the bottom edge of the menu bar just right of the notch.
    func seatPoint(_ edge: (lo: Double, hi: Double, y: Double)) -> CGPoint {
        guard let perch = model.perch else {
            return CGPoint(x: Double(model.notchSize.width) / 2 + 30, y: edge.y + Self.legHang)
        }
        return CGPoint(x: min(max(Double(perch.midX), edge.lo), edge.hi), y: Double(perch.minY) + Self.legHang)
    }

    /// How far its feet hang below what it sits on: in the sit drawings the seat is 40 px above the
    /// bottom of the 240 px frame, so the legs dangle over the front of the tab.
    static let legHang = 40.0 / 240 * Double(bodyHeight)

    /// Whether the hands are within 30 pt of the seat along the bar while idle.
    func nearSeat(_ s: Swing, _ edge: (lo: Double, hi: Double, y: Double)) -> Bool {
        guard model.petIdle, let hands = s.hands else { return false }
        return abs(Double(hands.x - seatPoint(edge).x)) < 30
    }

    /// Starts a hop to `target`, or just puts it there when motion is reduced. Silk it holds is let go.
    func startHop(_ s: Swing, to target: CGPoint, airborne: Bool) {
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
    func stepPerched(_ s: Swing, _ t: Double, moving: Bool, edge: (lo: Double, hi: Double, y: Double)) -> Frame {
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
    func hang(_ s: Swing, at anchor: CGPoint, angle: Double) {
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
    func advance(_ s: Swing, by h: Double, _ edge: (lo: Double, hi: Double, y: Double)) {
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
