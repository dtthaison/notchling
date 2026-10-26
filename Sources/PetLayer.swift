import AppKit
import SwiftUI

/// Hosts the pet's drawing in Core Animation layers, which the system composites without laying
/// anything out or re-rendering anything each tick: moving a layer costs far less than redrawing
/// a SwiftUI view tree 60 times a second.
struct PetLayerHost: NSViewRepresentable {
    let moving: Bool
    let gathered: Bool
    let needs: Int
    /// Where this view's left edge is in the overlay's frame.
    let origin: CGFloat
    /// Runs the pet's physics up to a time and returns what to draw.
    let frameAt: (Double, Bool) -> PetView.Frame

    func makeNSView(context: Context) -> PetLayerView { PetLayerView() }

    func updateNSView(_ view: PetLayerView, context: Context) {
        view.update(moving: moving, gathered: gathered, needs: needs, origin: origin, frameAt: frameAt)
    }

    static func dismantleNSView(_ view: PetLayerView, coordinator: ()) { view.stopTicking() }
}

/// Silk, body and badge as layers in one container. Positions arrive in the overlay's frame (y
/// down, from the shape's corner) and are converted here to the layer's own (y up, from this
/// view's corner). Every per-tick change is made without implicit animations.
final class PetLayerView: NSView {
    private let silkLayer = CALayer()
    private var strandLayers: [CAShapeLayer] = []
    private var dabLayers: [CAShapeLayer] = []
    private let shotLayer = CAShapeLayer()
    /// Hangs from the hands and turns with the strand; the body inside it is mirrored with the heading.
    private let hung = CALayer()
    private let body = CALayer()
    private var poseLayers: [CALayer] = []
    private let badge = CALayer()
    private let fallback = CATextLayer()

    private var images: [String: CGImage] = [:]
    private var badgeNumber = 0
    private var moving = false
    private var gathered: Bool?
    private var needs = 0
    private var origin: CGFloat = 0
    private var frameAt: ((Double, Bool) -> PetView.Frame)?
    /// A `CADisplayLink` on macOS 14 and later, a 60 Hz timer before that.
    private var ticker: AnyObject?

    private static let silkColor = NSColor(PetView.silk).cgColor

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = false

        for shape in [shotLayer] {
            shape.fillColor = nil
            shape.strokeColor = Self.silkColor
            shape.lineWidth = 1.2
            shape.lineCap = .round
            shape.opacity = 0.9
        }
        shotLayer.isHidden = true
        silkLayer.addSublayer(shotLayer)
        layer?.addSublayer(silkLayer)

        hung.anchorPoint = CGPoint(x: 0.5, y: 1)
        hung.addSublayer(body)
        badge.isHidden = true
        hung.addSublayer(badge)
        fallback.string = "✻"
        fallback.fontSize = 15
        fallback.font = NSFont.systemFont(ofSize: 15, weight: .bold)
        fallback.foregroundColor = NSColor(Color.claude).cgColor
        fallback.alignmentMode = .center
        fallback.isHidden = true
        body.addSublayer(fallback)
        layer?.addSublayer(hung)
    }

    required init?(coder: NSCoder) { nil }

    deinit { stopTicking() }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        silkLayer.frame = bounds
        for shape in strandLayers + dabLayers + [shotLayer] { shape.frame = silkLayer.bounds }
        CATransaction.commit()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil { stopTicking() } else if moving { startTicking() }
    }

    func update(moving: Bool, gathered: Bool, needs: Int, origin: CGFloat,
                frameAt: @escaping (Double, Bool) -> PetView.Frame) {
        self.frameAt = frameAt
        self.needs = needs
        self.origin = origin
        if gathered != self.gathered {
            // The silk fades out as the tabs open, and back in when they close.
            let animated = self.gathered != nil
            CATransaction.begin()
            CATransaction.setDisableActions(!animated)
            CATransaction.setAnimationDuration(0.2)
            silkLayer.opacity = gathered ? 0 : 1
            CATransaction.commit()
            self.gathered = gathered
        }
        self.moving = moving
        if moving { startTicking() } else {
            stopTicking()
            // Held still: draw the one frame it should be showing.
            render(frameAt(Date().timeIntervalSinceReferenceDate, false))
        }
    }

    private func startTicking() {
        guard ticker == nil, window != nil else { return }
        if #available(macOS 14, *) {
            let link = displayLink(target: self, selector: #selector(tick))
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 60, preferred: 60)
            link.add(to: .main, forMode: .common)
            ticker = link
        } else {
            let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { [weak self] _ in self?.tick() }
            RunLoop.main.add(timer, forMode: .common)
            ticker = timer
        }
    }

    func stopTicking() {
        if #available(macOS 14, *), let link = ticker as? CADisplayLink { link.invalidate() }
        (ticker as? Timer)?.invalidate()
        ticker = nil
    }

    @objc private func tick() {
        guard moving, let frameAt else { return }
        render(frameAt(Date().timeIntervalSinceReferenceDate, true))
    }

    // MARK: Drawing

    private func ensure(_ pool: inout [CAShapeLayer], count: Int, filled: Bool) {
        while pool.count < count {
            let shape = CAShapeLayer()
            shape.frame = silkLayer.bounds
            if filled {
                shape.fillColor = Self.silkColor
                shape.strokeColor = nil
            } else {
                shape.fillColor = nil
                shape.strokeColor = Self.silkColor
                shape.lineWidth = 1.2
                shape.lineCap = .round
            }
            silkLayer.addSublayer(shape)
            pool.append(shape)
        }
    }

    private func render(_ frame: PetView.Frame) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let height = bounds.height
        let gathered = self.gathered ?? false
        func local(_ point: CGPoint) -> CGPoint { CGPoint(x: point.x - origin, y: height - point.y) }

        // Silk: a smooth line per strand, a dab where it sticks, and the streak of a flying shot.
        ensure(&strandLayers, count: frame.strands.count, filled: false)
        ensure(&dabLayers, count: frame.strands.count, filled: true)
        for (index, shape) in strandLayers.enumerated() {
            guard index < frame.strands.count, let first = frame.strands[index].points.first else {
                shape.isHidden = true
                dabLayers[index].isHidden = true
                continue
            }
            let strand = frame.strands[index]
            let points = strand.points.map(local)
            let path = CGMutablePath()
            path.move(to: points[0])
            for i in 1..<points.count {
                let mid = CGPoint(x: (points[i - 1].x + points[i].x) / 2, y: (points[i - 1].y + points[i].y) / 2)
                path.addQuadCurve(to: mid, control: points[i - 1])
            }
            path.addLine(to: points[points.count - 1])
            shape.path = path
            shape.opacity = Float(0.9 * strand.opacity)
            shape.isHidden = false
            let dab = dabLayers[index]
            let spot = local(first)
            dab.path = CGPath(ellipseIn: CGRect(x: spot.x - 1.5, y: spot.y - 1.5, width: 3, height: 3), transform: nil)
            dab.opacity = Float(strand.opacity)
            dab.isHidden = false
        }
        if let shot = frame.shot {
            let path = CGMutablePath()
            path.move(to: local(shot.from))
            path.addLine(to: local(shot.to))
            shotLayer.path = path
            shotLayer.isHidden = false
        } else {
            shotLayer.isHidden = true
        }

        // Pet: hung from the hands, turned by the strand's angle about them.
        let width = PetView.bodyWidth, bodyHeight = PetView.bodyHeight
        let size = CGRect(x: 0, y: 0, width: width, height: bodyHeight)
        hung.bounds = size
        hung.position = local(frame.hands)
        hung.setAffineTransform(CGAffineTransform(rotationAngle: gathered ? 0 : frame.angle))
        body.bounds = size
        body.position = CGPoint(x: width / 2, y: bodyHeight / 2)
        body.setAffineTransform(CGAffineTransform(scaleX: frame.heading, y: 1))

        if PetView.poses.count == PetView.poseNames.count {
            fallback.isHidden = true
            let layers = gathered ? [PetView.Layer(name: "swing-6", opacity: 1)] : frame.layers
            while poseLayers.count < layers.count {
                let pose = CALayer()
                pose.contentsGravity = .resize
                pose.minificationFilter = .trilinear
                pose.magnificationFilter = .linear
                body.addSublayer(pose)
                poseLayers.append(pose)
            }
            for (index, pose) in poseLayers.enumerated() {
                guard index < layers.count, let image = cgImage(layers[index].name) else {
                    pose.isHidden = true
                    continue
                }
                pose.isHidden = false
                pose.contentsScale = window?.backingScaleFactor ?? 2
                pose.contents = image
                pose.bounds = size
                pose.position = CGPoint(x: width / 2, y: bodyHeight / 2)
                pose.opacity = Float(layers[index].opacity)
                // Only the flip pose turns about its own middle, once round.
                let spin = layers[index].name == "fly-flip" ? -frame.spin : 0
                pose.setAffineTransform(CGAffineTransform(rotationAngle: spin))
            }
        } else {
            for pose in poseLayers { pose.isHidden = true }
            fallback.isHidden = false
            fallback.contentsScale = window?.backingScaleFactor ?? 2
            fallback.bounds = CGRect(x: 0, y: 0, width: width, height: 20)
            fallback.position = CGPoint(x: width / 2, y: bodyHeight / 2)
        }

        // Badge: riding on the body, turning with it but not mirrored.
        if needs > 0 && !gathered, let (image, pointSize) = badgeImage(needs) {
            badge.isHidden = false
            badge.contents = image
            badge.contentsScale = window?.backingScaleFactor ?? 2
            badge.bounds = CGRect(origin: .zero, size: pointSize)
            badge.position = CGPoint(x: width + 3, y: bodyHeight - (bodyHeight * 0.5 + 7))
        } else {
            badge.isHidden = true
        }
    }

    private func cgImage(_ name: String) -> CGImage? {
        if let cached = images[name] { return cached }
        guard let image = PetView.poses[name]?.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
        images[name] = image
        return image
    }

    /// The orange capsule with its number and a soft shadow baked in, drawn again only when the
    /// number changes. The image is padded evenly, so its middle is the capsule's.
    private func badgeImage(_ number: Int) -> (CGImage, CGSize)? {
        struct Cache { static var image: CGImage?; static var size = CGSize.zero; static var number = 0; static var scale = CGFloat(0) }
        let scale = window?.backingScaleFactor ?? 2
        if number == Cache.number, scale == Cache.scale, let image = Cache.image { return (image, Cache.size) }
        let descriptor = NSFont.systemFont(ofSize: 9.5, weight: .bold).fontDescriptor.withDesign(.rounded)
        let font = descriptor.flatMap { NSFont(descriptor: $0, size: 9.5) } ?? NSFont.systemFont(ofSize: 9.5, weight: .bold)
        let text = NSAttributedString(string: "\(number)", attributes: [.font: font, .foregroundColor: NSColor.white])
        let textSize = text.size()
        let capsule = CGSize(width: max(14, textSize.width + 7), height: max(14, textSize.height))
        let pad: CGFloat = 4
        let pointSize = CGSize(width: capsule.width + 2 * pad, height: capsule.height + 2 * pad)
        guard let context = CGContext(data: nil, width: Int(pointSize.width * scale), height: Int(pointSize.height * scale),
                                      bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.scaleBy(x: scale, y: scale)
        let rect = CGRect(x: pad, y: pad, width: capsule.width, height: capsule.height)
        context.saveGState()
        context.setShadow(offset: .zero, blur: 3, color: NSColor.black.withAlphaComponent(0.35).cgColor)
        context.setFillColor(NSColor.orange.cgColor)
        context.addPath(CGPath(roundedRect: rect, cornerWidth: rect.height / 2, cornerHeight: rect.height / 2, transform: nil))
        context.fillPath()
        context.restoreGState()
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
        text.draw(at: CGPoint(x: rect.midX - textSize.width / 2, y: rect.midY - textSize.height / 2))
        NSGraphicsContext.restoreGraphicsState()
        guard let image = context.makeImage() else { return nil }
        Cache.image = image
        Cache.size = pointSize
        Cache.number = number
        Cache.scale = scale
        return (image, pointSize)
    }
}
