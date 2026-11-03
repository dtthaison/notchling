import AppKit
import UserNotifications
import ConsoleCore
import SwiftUI

@main
struct ConsoleApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model: ConsoleModel

    init() {
        // `AgentConsole --count`: one read of the real ~/.claude, print the totals, exit.
        if CommandLine.arguments.contains("--count") {
            let sessions = SessionCatalog().snapshot()
            let subagents = sessions.reduce(0) { $0 + $1.subagents.count }
            print("sessions=\(sessions.count) subagents=\(subagents)")
            exit(0)
        }
        FontLoader.registerAll()
        _model = State(initialValue: ConsoleModel())
    }

    var body: some Scene {
        Window("Agent Console", id: "main") {
            RootView(model: model)
                .frame(minWidth: Metric.minWindow.width, minHeight: Metric.minWindow.height)
                .background(WindowConfigurator())
                .task {
                    delegate.model = model
                    model.store.start()
                    model.startDemoTerminalIfRequested()
                }
        }
        .defaultSize(width: Metric.defaultWindow.width, height: Metric.defaultWindow.height)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Session mới") { model.showingNewSession = true }
                    .keyboardShortcut("n", modifiers: .command)
            }
        }
    }
}

/// Night Desk is dark only. Also remembers the window frame between launches.
private struct WindowConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            guard let window = view.window else { return }
            window.appearance = NSAppearance(named: .darkAqua)
            window.backgroundColor = NSColor(Token.neutral)
            window.minSize = Metric.minWindow
            window.setFrameAutosaveName("AgentConsoleMainWindow")
        }
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {}
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var model: ConsoleModel? {
        didSet { if model != nil { drainPendingURLs() } }
    }
    /// URLs that arrive during launch, before the model exists.
    private var pendingURLs: [URL] = []

    func application(_ application: NSApplication, open urls: [URL]) {
        pendingURLs += urls
        drainPendingURLs()
    }

    private func drainPendingURLs() {
        guard let model else { return }
        let urls = pendingURLs
        pendingURLs = []
        for url in urls {
            guard let action = AppURL.parse(url) else {
                ConsoleLog.error("AppDelegate.open", url.absoluteString, "not an agentconsole://open URL, ignored")
                continue
            }
            switch action {
            case .open(let phones):
                bringToFront()
                if phones { model.bridge.notifyPhonesToOpen() }
            }
        }
    }

    /// Brings the main window forward; a minimized one is restored. (Closing the last window quits the
    /// app, so a URL that arrives with no window launches it fresh.)
    private func bringToFront() {
        NSApp.activate(ignoringOtherApps: true)
        for window in NSApp.windows where window.canBecomeMain {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.appearance = NSAppearance(named: .darkAqua)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    /// Sessions in the session host keep running when the app quits. Only when there is no host (sessions are
    /// children of this app) do they end with it, so ask first.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let model else { return .terminateNow }
        model.bridge.stop()
        let running = model.owned.running
        guard !running.isEmpty else { return .terminateNow }

        if model.terminals.sessionsOutliveApp {
            let content = UNMutableNotificationContent()
            content.title = "Session vẫn chạy nền"
            content.body = "\(running.count) session do Console mở vẫn chạy trong hostd. Mở lại Agent Console để quay lại."
            UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)) { _ in }
            return .terminateNow
        }

        let alert = NSAlert()
        alert.messageText = "Thoát Agent Console?"
        alert.informativeText = "Có \(running.count) session do Console mở đang chạy. Thoát sẽ dừng chúng."
        alert.addButton(withTitle: "Dừng và thoát")
        alert.addButton(withTitle: "Ở lại")
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        model.terminateOwned()
        return .terminateNow
    }
}
