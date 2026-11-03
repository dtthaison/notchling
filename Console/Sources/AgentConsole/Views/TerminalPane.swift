import AppKit
import ConsoleCore
import SwiftTerm
import SwiftUI

/// The live terminal of a Console-owned session, attached to the same pty the replies go through.
/// The user answers TUI prompts (such as the folder-trust question) here; Console never answers them.
struct TerminalPane: NSViewRepresentable {
    let pty: any TerminalHandle

    func makeCoordinator() -> Coordinator { Coordinator(pty: pty) }

    func makeNSView(context: Context) -> TerminalView {
        let view = TerminalView(frame: .zero)
        view.terminalDelegate = context.coordinator
        view.font = NSFont(name: "GeistMono-Regular", size: 12) ?? .monospacedSystemFont(ofSize: 12, weight: .regular)
        view.nativeBackgroundColor = NSColor(Token.neutral)
        view.nativeForegroundColor = NSColor(Token.onSurface)
        view.setAccessibilityLabel("Terminal của session Claude")
        context.coordinator.attach(view)
        return view
    }

    func updateNSView(_ view: TerminalView, context: Context) {}

    static func dismantleNSView(_ view: TerminalView, coordinator: Coordinator) {
        coordinator.detach()
    }

    @MainActor
    final class Coordinator: NSObject, TerminalViewDelegate {
        private let pty: any TerminalHandle
        private var token: UUID?

        init(pty: any TerminalHandle) { self.pty = pty }

        func attach(_ view: TerminalView) {
            // Output arrives on the pty's queue; hop to main to feed the view.
            token = pty.addListener { [weak view] data in
                let bytes = Array(data)
                DispatchQueue.main.async { view?.feed(byteArray: bytes[...]) }
            }
        }

        func detach() {
            if let token { pty.removeListener(token) }
            token = nil
        }

        func send(source: TerminalView, data: ArraySlice<UInt8>) { pty.write(Data(data)) }
        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
            guard newCols > 0, newRows > 0 else { return }
            pty.resize(columns: UInt16(newCols), rows: UInt16(newRows))
        }
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {}
        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
            guard let url = URL(string: link), ["http", "https"].contains(url.scheme) else { return }
            NSWorkspace.shared.open(url)
        }
        func bell(source: TerminalView) { NSSound.beep() }
        func clipboardCopy(source: TerminalView, content: Data) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setData(content, forType: .string)
        }
        func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
}

struct TerminalContainer: View {
    let pty: any TerminalHandle

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: Space.sm) {
                Image(systemName: "info.circle").font(.system(size: 11)).accessibilityHidden(true)
                Text("Terminal do Console mở. Prompt như tin cậy thư mục là của bạn: trả lời tại đây, Console không tự bấm.")
                    .font(TypeToken.bodySm)
            }
            .foregroundStyle(Token.muted)
            .padding(.horizontal, Space.md).padding(.vertical, Space.sm)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Token.surface)
            .overlay(alignment: .bottom) { Divider1() }
            // One terminal view per session. Without the id SwiftUI reuses the view of the session shown before when
            // two Console-owned sessions are selected one after the other, and it keeps showing the first one's
            // pty (its coordinator is made once). Going through a session without a terminal rebuilt it, which is
            // why the chat only "switched" after opening a third session.
            TerminalPane(pty: pty)
                .id(pty.id)
                .padding(Space.sm)
                .background(Token.neutral)
        }
    }
}
