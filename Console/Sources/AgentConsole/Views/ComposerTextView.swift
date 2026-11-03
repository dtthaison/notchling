import AppKit
import SwiftUI

/// Multi-line input. Return sends, Shift-Return inserts a newline, ⌘Return sends. It is an NSTextView so
/// Return is seen only after an input method (Telex, VNI) has finished composing: a Return that
/// commits a composition never sends by accident.
struct ComposerTextView: NSViewRepresentable {
    @Binding var text: String
    @Binding var contentHeight: CGFloat
    var isEnabled: Bool
    var onSubmit: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = NSTextView()
        textView.delegate = context.coordinator
        textView.isRichText = false
        textView.allowsUndo = true
        textView.drawsBackground = false
        textView.font = NSFont(name: "Geist-Regular", size: 13) ?? .systemFont(ofSize: 13)
        textView.textColor = NSColor(Token.onSurface)
        textView.insertionPointColor = NSColor(Token.primary)
        textView.textContainerInset = NSSize(width: 0, height: 2)
        textView.textContainer?.lineFragmentPadding = 0
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = true
        textView.setAccessibilityLabel("Tin nhắn gửi vào session")

        let scroll = NSScrollView()
        scroll.documentView = textView
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = false
        scroll.borderType = .noBorder
        context.coordinator.textView = textView
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let textView = scroll.documentView as? NSTextView else { return }
        if textView.string != text { textView.string = text }
        textView.isEditable = isEnabled
        textView.isSelectable = isEnabled
        textView.alphaValue = isEnabled ? 1 : 0.5
        context.coordinator.reportHeight()
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerTextView
        weak var textView: NSTextView?

        init(_ parent: ComposerTextView) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let textView else { return }
            parent.text = textView.string
            reportHeight()
        }

        func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.insertNewline(_:)) else { return false }
            let flags = NSApp.currentEvent?.modifierFlags ?? []
            if flags.contains(.shift) || flags.contains(.option) {
                textView.insertNewlineIgnoringFieldEditor(nil)
                return true
            }
            parent.onSubmit()
            return true
        }

        func reportHeight() {
            guard let textView, let layout = textView.layoutManager, let container = textView.textContainer else { return }
            layout.ensureLayout(for: container)
            let height = layout.usedRect(for: container).height + textView.textContainerInset.height * 2
            DispatchQueue.main.async { [parent] in
                if abs(parent.contentHeight - height) > 0.5 { parent.contentHeight = height }
            }
        }
    }
}
