// Draws the app icon. Usage: swift Icon/make-icon.swift <output.png>
// The steam colours match the menu's agent states: needs input, working, completed.
import AppKit

let size: CGFloat = 1024
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let ctx = NSGraphicsContext.current!.cgContext
let space = CGColorSpaceCreateDeviceRGB()

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(red: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

func gradient(_ colors: [CGColor]) -> CGGradient {
    CGGradient(colorsSpace: space, colors: colors as CFArray, locations: nil)!
}

// MARK: Background: night-sky squircle on Apple's 824pt icon grid

let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
let tilePath = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)

ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: rgb(0x000000, 0.35))
ctx.addPath(tilePath)
ctx.setFillColor(rgb(0x1A1C4A))
ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addPath(tilePath)
ctx.clip()
ctx.drawLinearGradient(gradient([rgb(0x3A3F9E), rgb(0x1B1D55), rgb(0x0B0C26)]),
                       start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY), options: [])
// Soft warm glow behind the cup.
ctx.drawRadialGradient(gradient([rgb(0xFFB25A, 0.28), rgb(0xFFB25A, 0)]),
                       startCenter: CGPoint(x: 512, y: 430), startRadius: 0,
                       endCenter: CGPoint(x: 512, y: 430), endRadius: 380, options: [])
// Stars.
for (x, y, r, a) in [(215, 820, 7.0, 0.9), (290, 730, 4.0, 0.6), (190, 640, 3.5, 0.45),
                     (805, 845, 6.0, 0.85), (860, 735, 4.0, 0.55), (735, 905, 3.5, 0.5),
                     (330, 880, 3.0, 0.5)] as [(CGFloat, CGFloat, CGFloat, CGFloat)] {
    ctx.setFillColor(rgb(0xFFFFFF, a))
    ctx.fillEllipse(in: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2))
}
ctx.restoreGState()

// MARK: Saucer

let extend: CGGradientDrawingOptions = [.drawsBeforeStartLocation, .drawsAfterEndLocation]
// Shift the cup left so cup plus handle sits centred.
ctx.translateBy(x: -40, y: 0)

ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -8), blur: 18, color: rgb(0x000000, 0.4))
let saucer = CGRect(x: 512 - 280, y: 228, width: 560, height: 100)
ctx.addEllipse(in: saucer)
ctx.clip()
ctx.drawLinearGradient(gradient([rgb(0xF4F5FF), rgb(0xB9BDE3)]),
                       start: CGPoint(x: 0, y: saucer.maxY), end: CGPoint(x: 0, y: saucer.minY), options: [])
ctx.restoreGState()
ctx.setFillColor(rgb(0x9DA2D0, 0.55))
ctx.fillEllipse(in: CGRect(x: 512 - 175, y: 262, width: 350, height: 46))

// MARK: Cup

let cupFill = gradient([rgb(0xFFFFFF), rgb(0xE6E8FF), rgb(0xB8BCE6)])

// Handle: a thick ring whose left half the cup body covers.
ctx.saveGState()
ctx.setLineWidth(40)
ctx.setStrokeColor(rgb(0xD5D8F5))
ctx.strokeEllipse(in: CGRect(x: 650, y: 380, width: 170, height: 170))
ctx.restoreGState()

let body = CGMutablePath()
body.move(to: CGPoint(x: 302, y: 590))
body.addCurve(to: CGPoint(x: 512, y: 290), control1: CGPoint(x: 302, y: 400), control2: CGPoint(x: 390, y: 290))
body.addCurve(to: CGPoint(x: 722, y: 590), control1: CGPoint(x: 634, y: 290), control2: CGPoint(x: 722, y: 400))
body.closeSubpath()

ctx.saveGState()
ctx.addPath(body)
ctx.clip()
ctx.drawLinearGradient(cupFill, start: CGPoint(x: 330, y: 560), end: CGPoint(x: 720, y: 320), options: extend)
// Highlight down the left side.
ctx.setFillColor(rgb(0xFFFFFF, 0.55))
let shine = CGMutablePath()
shine.move(to: CGPoint(x: 345, y: 560))
shine.addCurve(to: CGPoint(x: 430, y: 360), control1: CGPoint(x: 345, y: 450), control2: CGPoint(x: 380, y: 390))
shine.addCurve(to: CGPoint(x: 385, y: 560), control1: CGPoint(x: 400, y: 400), control2: CGPoint(x: 385, y: 470))
shine.closeSubpath()
ctx.addPath(shine)
ctx.fillPath()
ctx.restoreGState()

// Rim and coffee.
let rim = CGRect(x: 302, y: 552, width: 420, height: 76)
ctx.setFillColor(rgb(0xF7F8FF))
ctx.fillEllipse(in: rim)
ctx.saveGState()
let coffee = rim.insetBy(dx: 22, dy: 12).offsetBy(dx: 0, dy: -2)
ctx.addEllipse(in: coffee)
ctx.clip()
ctx.drawLinearGradient(gradient([rgb(0x8A5434), rgb(0x4A2614)]),
                       start: CGPoint(x: 0, y: coffee.maxY), end: CGPoint(x: 0, y: coffee.minY), options: [])
ctx.restoreGState()

// MARK: Steam, one wisp per agent state, fading as it rises

for (x, top, color, sway) in [(420.0, 830.0, 0xFF9F0A, 1.0),   // needs input
                              (512.0, 885.0, 0x0A84FF, -1.0),  // working
                              (604.0, 830.0, 0x30D158, 1.0)]   // completed
    as [(CGFloat, CGFloat, UInt32, CGFloat)] {
    let wisp = CGMutablePath()
    let bottom: CGFloat = 645
    wisp.move(to: CGPoint(x: x, y: bottom))
    wisp.addCurve(to: CGPoint(x: x, y: top),
                  control1: CGPoint(x: x - 58 * sway, y: bottom + (top - bottom) * 0.35),
                  control2: CGPoint(x: x + 58 * sway, y: bottom + (top - bottom) * 0.65))

    ctx.saveGState()
    ctx.setShadow(offset: .zero, blur: 24, color: rgb(color, 0.8))
    ctx.addPath(wisp.copy(strokingWithWidth: 30, lineCap: .round, lineJoin: .round, miterLimit: 10))
    ctx.clip()
    ctx.drawLinearGradient(gradient([rgb(color), rgb(color, 0.15)]),
                           start: CGPoint(x: 0, y: bottom), end: CGPoint(x: 0, y: top + 15), options: extend)
    ctx.restoreGState()
}

NSGraphicsContext.current = nil
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.png"
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
print("Wrote \(out)")
