import AppKit
import SwiftUI

/// The vault's own windows, opened for an agent by `VaultServer`: the approval before a secret marked
/// "ask first" is used, and the form when an agent asks for a secret to be added or updated. Both are
/// called on the server's connection thread and block it until the user answers.
enum VaultPrompt {
    enum Decision { case deny, once, always }

    static func approve(_ entries: [VaultEntry], command: String, cwd: String) -> Decision {
        wait(title: "Use a Secret", default: .deny) { answer in
            VaultApproveView(entries: entries, command: command, cwd: cwd, decide: answer)
        }
    }

    static func request(_ request: VaultRequest) -> VaultEntry? {
        let result: VaultEntry?? = wait(title: "Secret Vault", default: nil) { answer in
            VaultSheet(form: VaultForm(request: request), finish: { answer($0) }, done: { answer(nil) })
        }
        return result ?? nil
    }

    /// The form for this secret in its own panel (a reminder's Renew…, or a click on the reminder): for a
    /// renewal the value is empty and the how-to above it. Doesn't wait. Call on the main thread.
    static func open(_ entry: VaultEntry, renew: Bool) {
        let controller = PromptPanel(title: renew ? "Renew Secret" : "Secret Vault")
        let close: () -> Void = { [weak controller] in controller?.close() }
        controller.show(AnyView(VaultSheet(form: VaultForm(entry, renew: renew), finish: { _ in close() }, done: close)))
    }

    /// Shows `content` in a floating panel on the main thread and waits for its answer (closing the panel
    /// answers `fallback`).
    private static func wait<T, V: View>(title: String, default fallback: T,
                                         _ content: @escaping (@escaping (T) -> Void) -> V) -> T {
        let done = DispatchSemaphore(value: 0)
        var result = fallback
        DispatchQueue.main.async {
            let controller = PromptPanel(title: title)
            var answered = false
            let answer: (T) -> Void = { value in
                guard !answered else { return }
                answered = true
                result = value
                controller.close()
                done.signal()
            }
            controller.onClose = { answer(fallback) }
            controller.show(AnyView(content(answer)))
        }
        done.wait()
        return result
    }
}

/// A titled floating panel that brings the app forward, since an agent's request comes while the user is
/// in another app. Kept alive by `open` until it closes.
private final class PromptPanel: NSObject, NSWindowDelegate {
    private static var open: [PromptPanel] = []
    private let panel: NSPanel
    var onClose: (() -> Void)?

    init(title: String) {
        panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 480, height: 300),
                        styleMask: [.titled, .closable], backing: .buffered, defer: false)
        super.init()
        panel.title = title
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.delegate = self
    }

    func show(_ view: AnyView) {
        let host = NSHostingView(rootView: view)
        panel.contentView = host
        panel.setContentSize(host.fittingSize)
        panel.center()
        Self.open.append(self)
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
    }

    func close() {
        onClose = nil
        panel.close()
    }

    func windowWillClose(_ notification: Notification) {
        onClose?()
        onClose = nil
        DispatchQueue.main.async { Self.open.removeAll { $0 === self } }
    }
}

struct VaultApproveView: View {
    let entries: [VaultEntry]
    let command: String
    let cwd: String
    let decide: (VaultPrompt.Decision) -> Void

    private var names: String { entries.map(\.name).joined(separator: ", ") }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: "key.horizontal").font(.system(size: 18)).foregroundColor(KitInk.accentText)
                Text("Allow an agent to use \(names)?").kitType(.title).foregroundColor(KitInk.text)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text("FOLDER").kitType(.section).foregroundColor(KitInk.textFaint)
                Text(cwd).kitType(.mono).foregroundColor(KitInk.textMuted).lineLimit(1).truncationMode(.head)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text("COMMAND").kitType(.section).foregroundColor(KitInk.textFaint)
                ScrollView {
                    Text(command).kitType(.mono).foregroundColor(KitInk.text).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(10)
                }
                .frame(maxHeight: 160)
                .fixedSize(horizontal: false, vertical: true)
                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.card))
                .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).strokeBorder(KitInk.hairline))
            }
            Text("The command gets the values as environment variables; its output comes back with them hidden. Deny it if the command would print, send or save a value somewhere you don't expect.")
                .kitType(.meta).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                Button("Always Allow") { decide(.always) }
                    .buttonStyle(.kit(.ghost))
                    .help("Stop asking for \(names); you can turn it back on in the vault")
                Spacer()
                Button("Deny") { decide(.deny) }
                    .buttonStyle(.kit(.secondary)).keyboardShortcut(.cancelAction)
                Button("Allow Once") { decide(.once) }
                    .buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 500)
        .background(KitInk.window)
        .part("Secret Approval")
    }
}
