import AppKit
import SwiftUI

// Renders the README demo: the real NotchView driven by made-up agents on a fake desktop,
// captured frame by frame to PNGs in argv[1].

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "frames"
let fps = 20.0
let canvas = CGSize(width: 960, height: 540)
let scale: CGFloat = 4.0 / 3.0  // 1280 x 720 frames

try? FileManager.default.removeItem(atPath: outDir)
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
let transcriptDir = outDir + "/../transcripts"
try? FileManager.default.createDirectory(atPath: transcriptDir, withIntermediateDirectories: true)

// MARK: - Made-up transcripts

func user(_ text: String) -> [String: Any] { ["type": "user", "message": ["role": "user", "content": text]] }
func assistant(_ text: String, tools: [(String, String, [String: Any])] = []) -> [String: Any] {
    var blocks: [[String: Any]] = text.isEmpty ? [] : [["type": "text", "text": text]]
    for (id, name, input) in tools { blocks.append(["type": "tool_use", "id": id, "name": name, "input": input]) }
    return ["type": "assistant", "message": ["role": "assistant", "content": blocks]]
}
func result(_ id: String) -> [String: Any] {
    ["type": "user", "message": ["role": "user", "content": [["type": "tool_result", "tool_use_id": id, "content": "ok"]]]]
}

func writeTranscript(_ name: String, _ lines: [[String: Any]]) -> String {
    let path = "\(transcriptDir)/\(name).jsonl"
    let text = lines.compactMap { try? JSONSerialization.data(withJSONObject: $0) }
        .compactMap { String(data: $0, encoding: .utf8) }.joined(separator: "\n") + "\n"
    try? text.write(toFile: path, atomically: true, encoding: .utf8)
    return path
}

let authPath = writeTranscript("auth", [
    user("Add rate limiting to the login endpoint"),
    assistant("I'll look at how login is wired up first.", tools: [
        ("t1", "Read", ["file_path": "/src/auth/login.ts"]), ("t2", "Read", ["file_path": "/src/auth/session.ts"])]),
    result("t1"), result("t2"),
    assistant("Added a sliding-window limiter: 5 attempts per minute per IP, with a Retry-After header.",
              tools: [("t3", "Edit", ["file_path": "/src/auth/login.ts"])]),
    result("t3"),
    assistant("", tools: [("t4", "Bash", ["command": "npm test -- --coverage", "description": "Run the test suite"])]),
])

let landingPath = writeTranscript("landing", [
    user("Make the landing page hero feel less busy"),
    assistant("I tried three layouts and screenshotted each.", tools: [
        ("l1", "Edit", ["file_path": "/site/components/Hero.tsx"]), ("l2", "Bash", ["command": "npm run shots"])]),
    result("l1"), result("l2"),
    assistant("", tools: [("q1", "AskUserQuestion", ["questions": [[
        "header": "Hero", "question": "Which hero layout should I keep?", "multiSelect": false,
        "options": [["label": "Split with screenshot", "description": "Text left, product shot right"],
                    ["label": "Centered headline", "description": "Big type, one button"],
                    ["label": "Short video loop", "description": "Autoplays muted"]]]]])]),
])

let flakyPath = writeTranscript("flaky", [
    user("Find out why checkout.test.ts fails on CI but not locally"),
    assistant("", tools: [("f1", "Grep", ["pattern": "new Date("])]),
    result("f1"),
    assistant("It depends on the system time zone. Pinning it in the test setup."),
    // A tool call on its own is what sets the row's "doing now" line.
    assistant("", tools: [("f2", "Edit", ["file_path": "/test/checkout.test.ts"])]),
])

let docsPath = writeTranscript("docs", [
    user("Document the new webhook retries"),
    assistant("Here's the plan: a short overview, the retry schedule as a table, and an example payload.",
              tools: [("d1", "Read", ["file_path": "/docs/webhooks.md"]), ("d2", "Write", ["file_path": "/docs/webhooks/retries.md"])]),
    result("d1"), result("d2"),
    assistant("Wrote docs/webhooks/retries.md and linked it from the webhooks page. Building the docs site to check the links now."),
    assistant("", tools: [("d3", "Bash", ["command": "npm run docs:build", "description": "Building the docs site"])]),
])

let releasePath = writeTranscript("release", [
    user("Draft release notes for 2.4 from the merged PRs"),
    assistant("", tools: [("r1", "Bash", ["command": "gh pr list --state merged --limit 40"])]),
    result("r1"),
    assistant("Release notes for 2.4 are in CHANGELOG.md: 3 features, 7 fixes, and a note about the new config flag."),
])

// MARK: - Made-up agents

let now = Date()
func agent(_ id: String, _ name: String, _ group: AgentGroup, _ label: String, host: String,
           target: AgentTarget, path: String, ago: TimeInterval) -> Agent {
    Agent(id: id, name: name, project: name, cwd: "/Users/demo/\(name)", group: group, label: label,
          detail: nil, since: now.addingTimeInterval(-ago), target: target, transcriptPath: path, host: host)
}

var auth = agent("auth", "shop-api: rate limiting", .needsInput, "Waiting for you", host: "Background",
                 target: .job("auth"), path: authPath, ago: 40)
let landing = agent("landing", "website: hero layout", .needsInput, "Waiting for you", host: "Background",
                    target: .job("landing"), path: landingPath, ago: 180)
let flaky = agent("flaky", "fix flaky checkout test", .working, "Working", host: "Terminal",
                  target: .process(4242), path: flakyPath, ago: 75)
let docs = agent("docs", "docs: webhook retries", .working, "Working", host: "Background",
                 target: .job("docs"), path: docsPath, ago: 20)
let release = agent("release", "release notes 2.4", .completed, "Done", host: "VS Code",
                    target: .resume(sessionID: "release", cwd: "/Users/demo/app"), path: releasePath, ago: 5)

let permission = HookRequest([
    "id": "perm1", "session_id": "auth", "pid": NSNumber(value: 4243), "created": now.timeIntervalSince1970,
    "hook_event_name": "PermissionRequest", "tool_name": "Bash",
    "tool_input": ["command": "npm test -- --coverage", "description": "Run the test suite"],
    "permission_suggestions": [["type": "addRules", "rules": [["toolName": "Bash", "ruleContent": "npm test:*"]]]],
])!

// MARK: - Fake desktop

final class Demo: ObservableObject {
    @Published var caption = ""
}

struct Desktop: View {
    @ObservedObject var demo: Demo
    let model: NotchModel

    var body: some View {
        ZStack(alignment: .top) {
            LinearGradient(colors: [Color(red: 0.16, green: 0.13, blue: 0.32), Color(red: 0.05, green: 0.33, blue: 0.40),
                                    Color(red: 0.55, green: 0.42, blue: 0.62)],
                           startPoint: .topLeading, endPoint: .bottomTrailing)
            menuBar
            NotchView(model: model).frame(width: canvas.width, height: canvas.height)
            VStack {
                Spacer()
                if !demo.caption.isEmpty {
                    Text(demo.caption)
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundColor(.white)
                        .padding(.horizontal, 18).padding(.vertical, 10)
                        .background(Capsule().fill(Color.black.opacity(0.55)))
                        .id(demo.caption)
                        .transition(.opacity)
                }
            }
            .padding(.bottom, 22)
            .animation(.easeInOut(duration: 0.3), value: demo.caption)
        }
        .frame(width: canvas.width, height: canvas.height)
        .environment(\.colorScheme, .dark)
    }

    private var menuBar: some View {
        HStack(spacing: 18) {
            Text("Terminal").fontWeight(.bold)
            Text("Shell"); Text("Edit"); Text("View"); Text("Window")
            Spacer()
            Image(systemName: "cup.and.saucer.fill")
            Image(systemName: "wifi")
            Text("Tue 9:41")
        }
        .font(.system(size: 13))
        .foregroundColor(.white.opacity(0.92))
        .padding(.horizontal, 16)
        .frame(height: 32)
        .background(Color.black.opacity(0.28))
    }
}

// MARK: - Window and capture

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let model = NotchModel()
model.notchSize = CGSize(width: 185, height: 32)
model.screenSpan = (left: canvas.width / 2, right: canvas.width / 2)
model.width = .medium
model.showPet = true
let demo = Demo()

let window = NSWindow(contentRect: NSRect(origin: .zero, size: canvas), styleMask: [.borderless],
                      backing: .buffered, defer: false)
let host = NSHostingView(rootView: Desktop(demo: demo, model: model))
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

model.agents = [auth, landing, flaky, docs]
model.refreshSnapshots { _, _ in }

at(0.0) { demo.caption = "Notchling lives in your Mac's notch while Claude Code works" }
at(2.8) {
    model.agents = [auth, landing, flaky, docs, release]
    model.refreshSnapshots { _, _ in model.peek(release) }
    demo.caption = "It peeks out when an agent finishes"
}
at(6.0) { model.go(.collapsed) }
at(6.6) { model.go(.list); model.tab = .needsInput; demo.caption = "Hover to see every session, by what needs you" }
at(8.6) { model.tab = .working }
at(10.2) { model.tab = .completed }
at(11.8) { model.go(.collapsed) }
at(12.4) {
    model.requests = ["auth": permission]
    model.peek(auth)
    demo.caption = "Approve permission prompts without finding the terminal"
}
at(15.6) {
    model.go(.collapsed)
    model.requests = [:]
    auth.group = .working
    auth.label = "Working"
    model.agents = [auth, landing, flaky, docs, release]
}
at(16.0) { demo.caption = "Read the conversation and reply to background agents" }
at(16.4) { model.open("docs") }
let reply = "Also add a section on idempotency keys"
for (i, _) in reply.enumerated() {
    at(17.4 + Double(i) * 0.06) { model.draft = String(reply.prefix(i + 1)) }
}
at(21.6) { model.go(.collapsed); model.draft = ""; demo.caption = "Notchling: free and open source" }
let end = 24.0

let start = Date().addingTimeInterval(1.5)  // let the first transcripts load and the pet start
for (t, action) in steps {
    DispatchQueue.main.asyncAfter(deadline: .now() + 1.5 + t, execute: action)
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
