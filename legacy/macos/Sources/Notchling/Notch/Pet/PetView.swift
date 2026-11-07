import SwiftUI

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
/// time the strand takes to stick. Only one drawing shows at a time: two drawn over each other,
/// even mid-blend, read as a blurred double of the character.
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

    @StateObject var swing = Swing()

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
}
