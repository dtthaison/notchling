import AppKit
import SwiftUI

// Renders the README's account-switching demo: the real Accounts window driven by made-up
// accounts, with a pretend cursor, captured frame by frame to PNGs in argv[1].
// Built with -D DEMO for the stores' demo setters; nothing on this Mac is read or changed.

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "frames"
let fps = 20.0
let canvas = CGSize(width: 960, height: 680)
let scale: CGFloat = 4.0 / 3.0

try? FileManager.default.removeItem(atPath: outDir)
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

// MARK: - Made-up accounts

let work = ClaudeAccount(id: "a1111111:o1", email: "alex@acme.example", plan: "Max 20x")
let personal = ClaudeAccount(id: "a2222222:o2", email: "alex.personal@example.com", plan: "Pro")
let studio = ClaudeAccount(id: "a3333333:o3", email: "alex@studio.example", plan: "Max 5x")
func uuid(_ a: ClaudeAccount) -> String { String(a.id.prefix { $0 != ":" }) }

let now = Date()
func limit(_ percent: Double, in hours: Double) -> UsageWindow {
    UsageWindow(percent: percent, resetsAt: now.addingTimeInterval(hours * 3600))
}
let usage: [String: AccountUsage] = [
    work.id: AccountUsage(session: limit(94, in: 3.2), week: limit(71, in: 70)),
    personal.id: AccountUsage(session: limit(6, in: 4.6), week: limit(18, in: 120)),
    studio.id: AccountUsage(session: limit(0, in: 5), week: limit(42, in: 30)),
]
func spent(_ today: Double, _ week: Double, _ month: Double) -> CostTracker.Summary {
    var s = CostTracker.Summary()
    (s.today, s.week, s.month) = (today, week, month)
    return s
}

let accounts = AccountStore()
accounts.demo(accounts: [work, personal, studio], activeID: work.id, usage: usage)
let desktop = ClaudeDesktopStore()
desktop.demo(activeUuid: uuid(work), saved: [uuid(work), uuid(personal)])
let cost = CostTracker()
cost.demo(byAccount: [work.id: spent(38.20, 214.75, 802.10), personal.id: spent(0, 12.40, 61.90),
                      studio.id: spent(0, 48.30, 190.55)])
let tab = TabModel()
let view = AccountsView(accounts: accounts, antigravity: AntigravityStore(), desktop: desktop, cost: cost,
                        settings: SettingsModel(), signIn: SignInModel(store: accounts), tab: tab)

// MARK: - Fake desktop

final class Demo: ObservableObject {
    @Published var caption = ""
    @Published var cursor = CGPoint(x: 820, y: 560)
    @Published var clicks = 0
}

struct Desktop: View {
    @ObservedObject var demo: Demo

    var body: some View {
        ZStack(alignment: .topLeading) {
            LinearGradient(colors: [Color(red: 0.16, green: 0.13, blue: 0.32), Color(red: 0.05, green: 0.33, blue: 0.40),
                                    Color(red: 0.55, green: 0.42, blue: 0.62)],
                           startPoint: .topLeading, endPoint: .bottomTrailing)
            menuBar
            VStack(spacing: 0) {
                titleBar
                view
            }
            .background(Color(nsColor: .windowBackgroundColor))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.15), lineWidth: 1))
            .shadow(color: .black.opacity(0.45), radius: 24, y: 10)
            .offset(x: (canvas.width - 460) / 2, y: 46)
            VStack {
                Spacer()
                if !demo.caption.isEmpty {
                    Text(demo.caption)
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundColor(.white)
                        .padding(.horizontal, 18).padding(.vertical, 10)
                        .background(Capsule().fill(Color.black.opacity(0.6)))
                        .id(demo.caption)
                        .transition(.opacity)
                }
            }
            .frame(width: canvas.width)
            .padding(.bottom, 14)
            .animation(.easeInOut(duration: 0.3), value: demo.caption)
            Cursor(clicks: demo.clicks)
                .position(x: demo.cursor.x + 8, y: demo.cursor.y + 11)
                .animation(.easeInOut(duration: 0.7), value: demo.cursor)
        }
        .frame(width: canvas.width, height: canvas.height)
        .environment(\.colorScheme, .dark)
    }

    private var menuBar: some View {
        HStack(spacing: 18) {
            Text("Notchling").fontWeight(.bold)
            Text("File"); Text("Edit"); Text("Window")
            Spacer()
            Image(systemName: "cup.and.saucer.fill")
            Image(systemName: "wifi")
            Text("Tue 9:41")
        }
        .font(.system(size: 13))
        .foregroundColor(.white.opacity(0.92))
        .padding(.horizontal, 16)
        .frame(width: canvas.width, height: 32)
        .background(Color.black.opacity(0.28))
        .overlay(UnevenNotch().fill(Color.black).frame(width: 185, height: 32), alignment: .top)
    }

    private var titleBar: some View {
        ZStack {
            HStack(spacing: 8) {
                ForEach([Color.red, Color.yellow, Color.green], id: \.self) { Circle().fill($0.opacity(0.85)).frame(width: 12, height: 12) }
                Spacer()
            }
            .padding(.leading, 12)
            Text("Notchling").font(.system(size: 13, weight: .semibold)).foregroundColor(.secondary)
        }
        .frame(width: 460, height: 28)
    }
}

/// The camera notch: square at the top, rounded at the bottom.
struct UnevenNotch: Shape {
    func path(in r: CGRect) -> Path {
        Path(roundedRect: r.insetBy(dx: 0, dy: -10), cornerRadius: 10)
    }
}

/// The arrow cursor, with a ring that grows and fades on each click.
struct Cursor: View {
    let clicks: Int
    @State private var ring = false

    var body: some View {
        ZStack {
            Circle().stroke(Color.white.opacity(ring ? 0 : 0.8), lineWidth: 2)
                .frame(width: ring ? 34 : 6, height: ring ? 34 : 6)
                .offset(x: -8, y: -11)
            Image(nsImage: NSCursor.arrow.image)
        }
        .onChange(of: clicks) { _ in
            ring = false
            withAnimation(.easeOut(duration: 0.45)) { ring = true }
        }
    }
}

// MARK: - Window and capture

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let demo = Demo()

let window = NSWindow(contentRect: NSRect(origin: .zero, size: canvas), styleMask: [.borderless],
                      backing: .buffered, defer: false)
window.appearance = NSAppearance(named: .darkAqua)
let host = NSHostingView(rootView: Desktop(demo: demo))
if #available(macOS 13, *) { host.sizingOptions = [] }
host.frame = NSRect(origin: .zero, size: canvas)
window.contentView = host
// Behind everything and fully transparent on screen: it still animates and draws, but nobody sees it.
window.alphaValue = 0.0
window.level = .init(rawValue: Int(CGWindowLevelForKey(.desktopWindow)) - 1)
window.ignoresMouseEvents = true
window.setFrameOrigin(NSPoint(x: 0, y: 0))
window.orderFrontRegardless()

var frameIndex = 0
let writeQueue = DispatchQueue(label: "write")
func capture() {
    let px = (w: Int(canvas.width * scale), h: Int(canvas.height * scale))
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px.w, pixelsHigh: px.h,
                                     bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                     colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { return }
    rep.size = canvas
    host.cacheDisplay(in: host.bounds, to: rep)
    let index = frameIndex
    frameIndex += 1
    writeQueue.async {
        let data = rep.representation(using: .png, properties: [:])
        try? data?.write(to: URL(fileURLWithPath: String(format: "%@/f%04d.png", outDir, index)))
    }
}

// MARK: - Timeline

var steps: [(Double, () -> Void)] = []
func at(_ t: Double, _ action: @escaping () -> Void) { steps.append((t, action)) }
func move(_ t: Double, _ x: CGFloat, _ y: CGFloat) { at(t) { demo.cursor = CGPoint(x: x, y: y) } }
func click(_ t: Double) { at(t) { demo.clicks += 1 } }

// Where the cursor goes, read off a rendered frame (canvas points; the tip sits about 1, 6 above-left).
let switchSecond = CGPoint(x: 601, y: 337)   // Switch on the second Claude Code card
let desktopTab = CGPoint(x: 481, y: 153)     // the Claude Desktop segment
let switchDesktop = CGPoint(x: 601, y: 272)  // Switch on the second Desktop card

at(0.0) { demo.caption = "Every Claude account you sign in to is saved, with its limits" }
at(3.4) { demo.caption = "Running low? Switch Claude Code in one click, no new sign-in" }
move(3.6, switchSecond.x, switchSecond.y)
click(4.8)
at(4.8) { accounts.demo(accounts: [work, personal, studio], activeID: work.id, usage: usage, busy: true) }
at(5.4) { accounts.demo(accounts: [work, personal, studio], activeID: personal.id, usage: usage) }
at(7.6) { demo.caption = "Claude Desktop switches the same way" }
move(7.8, desktopTab.x, desktopTab.y)
click(8.8)
at(8.8) { tab.provider = .claudeDesktop }
move(9.8, switchDesktop.x, switchDesktop.y)
click(11.0)
at(11.0) {
    desktop.demo(activeUuid: uuid(work), saved: [uuid(work), uuid(personal)], busy: true)
    demo.caption = "Notchling reopens Claude Desktop on the other account"
}
at(12.6) { desktop.demo(activeUuid: uuid(personal), saved: [uuid(work), uuid(personal)]) }
move(14.0, 820, 560)
at(14.6) { demo.caption = "Notchling: free and open source" }
let end = 17.0

let start = Date().addingTimeInterval(1.0)
for (t, action) in steps {
    DispatchQueue.main.asyncAfter(deadline: .now() + 1.0 + t, execute: action)
}
let total = Int(end * fps)
let timer = Timer(timeInterval: 1 / fps, repeats: true) { timer in
    guard Date() >= start else { return }
    capture()
    if frameIndex >= total {
        timer.invalidate()
        writeQueue.async {
            print("wrote \(frameIndex) frames to \(outDir)")
            exit(0)
        }
    }
}
RunLoop.main.add(timer, forMode: .common)
app.run()
