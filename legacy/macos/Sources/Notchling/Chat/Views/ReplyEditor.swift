import AppKit
import SwiftUI

/// How tall the reply box is for some text at some width: one line when empty, growing to `maxLines`,
/// then it scrolls. It lays a copy of the text out apart from the live text view, at the width SwiftUI is
/// about to give it, so a window that just got wider or narrower is measured at its new width. (The
/// SwiftUI `TextField(axis: .vertical)` this replaced kept wrapping at the width it had before the resize
/// and worked its height out from that, which clipped the first line.)
final class ReplyMeasurer {
    static let maxLines = 8
    /// Narrower than this isn't a real layout (SwiftUI probes with 0), and wraps every character.
    static let minWidth: CGFloat = 40

    private let storage = NSTextStorage()
    private let manager = NSLayoutManager()
    private let container = NSTextContainer()
    private var last: (text: String, font: NSFont, width: CGFloat, height: CGFloat)?

    init() {
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
    }

    func lineHeight(for font: NSFont) -> CGFloat { ceil(manager.defaultLineHeight(for: font)) }

    /// The height of the whole text at `width`, one line at least, not capped.
    func contentHeight(of text: String, font: NSFont, width: CGFloat) -> CGFloat {
        let width = max(width, Self.minWidth)
        if let last, last.text == text, last.font == font, last.width == width { return last.height }
        storage.setAttributedString(NSAttributedString(string: text, attributes: [.font: font]))
        container.size = NSSize(width: width, height: CGFloat.greatestFiniteMagnitude)
        manager.ensureLayout(for: container)
        // A trailing newline starts an empty last line that only the extra line fragment accounts for.
        let laidOut = max(manager.usedRect(for: container).height, manager.extraLineFragmentRect.maxY)
        let height = ceil(max(laidOut, lineHeight(for: font)))
        last = (text, font, width, height)
        return height
    }

    /// The box's height: the text's, between one line and `maxLines`.
    func height(of text: String, font: NSFont, width: CGFloat) -> CGFloat {
        min(contentHeight(of: text, font: font, width: width), lineHeight(for: font) * CGFloat(Self.maxLines))
    }
}

/// The reply box: a growing plain-text editor (an NSTextView in a scroll view). Return sends, Option or
/// Shift Return adds a line. Tab completion and the slash-command list are handled one level up, by the
/// window's key handler (`ChatModel.handleKey`), before a key reaches the text view.
struct ReplyEditor: NSViewRepresentable {
    @Binding var text: String
    @Binding var focused: Bool
    let placeholder: String
    /// A suggestion shown as the placeholder reads as content rather than a hint.
    let placeholderIsSuggestion: Bool
    let font: NSFont
    let accessibilityLabel: String
    let onSubmit: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> ReplyScrollView {
        let (scroll, view) = Self.makeViews()
        view.delegate = context.coordinator
        view.onFocusChange = { [weak coordinator = context.coordinator] on in coordinator?.focusChanged(on) }
        return scroll
    }

    func updateNSView(_ scroll: ReplyScrollView, context: Context) {
        let coordinator = context.coordinator
        coordinator.parent = self
        let view: ReplyTextView = scroll.textView
        if view.font != font {
            view.font = font
            view.typingAttributes = [.font: font, .foregroundColor: NSColor(KitInk.text)]
            view.textStorage?.addAttributes([.font: font], range: NSRange(location: 0, length: view.textStorage?.length ?? 0))
        }
        // The kit's colors are dynamic; re-apply them when the appearance changes.
        if coordinator.scheme != context.environment.colorScheme {
            coordinator.scheme = context.environment.colorScheme
            view.textColor = NSColor(KitInk.text)
            view.typingAttributes[.foregroundColor] = NSColor(KitInk.text)
        }
        // Only a draft that differs replaces the text, and never mid-composition: that would drop the
        // marked text and move the cursor.
        if view.string != text, !view.hasMarkedText() {
            view.string = text
            view.setSelectedRange(NSRange(location: (text as NSString).length, length: 0))
        }
        view.placeholder = NSAttributedString(string: placeholder, attributes: [
            .font: font,
            .foregroundColor: NSColor(placeholderIsSuggestion ? KitInk.textMuted : KitInk.textFaint),
            .paragraphStyle: Self.truncating,
        ])
        view.setAccessibilityLabel(accessibilityLabel)
        // Act on the binding only when it changed, so a click that focused the box before the binding
        // caught up isn't undone by an unrelated update.
        if focused != coordinator.wantsFocus {
            coordinator.wantsFocus = focused
            coordinator.applyFocus(focused, to: view)
        }
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView scroll: ReplyScrollView, context: Context) -> CGSize? {
        var width = proposal.width ?? 300
        if !width.isFinite { width = 300 }
        let height = context.coordinator.measurer.height(of: text, font: font, width: width)
        return CGSize(width: width, height: height)
    }

    private static let truncating: NSParagraphStyle = {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        return style
    }()

    /// The scroll view and the text view in it, set up so the text wraps at the scroll view's current
    /// width (`widthTracksTextView`) and the height is the layout's alone. Also used by the tests.
    static func makeViews() -> (ReplyScrollView, ReplyTextView) {
        let view = ReplyTextView(usingTextLayoutManager: false)
        view.minSize = .zero
        view.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.autoresizingMask = [.width]
        view.textContainer?.widthTracksTextView = true
        view.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        view.textContainer?.lineFragmentPadding = 0
        view.textContainerInset = .zero
        view.isRichText = false
        view.importsGraphics = false
        view.allowsUndo = true
        view.drawsBackground = false
        // The box takes commands and prose: no curly quotes, dashes or other rewriting of what's typed.
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false
        view.isAutomaticLinkDetectionEnabled = false
        view.isContinuousSpellCheckingEnabled = false
        view.smartInsertDeleteEnabled = false
        view.font = KitType.body.nsFont
        view.textColor = NSColor(KitInk.text)

        let scroll = ReplyScrollView()
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.hasHorizontalScroller = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        // A legacy scroller would take width the measurer doesn't know about.
        scroll.scrollerStyle = .overlay
        scroll.verticalScrollElasticity = .none
        scroll.documentView = view
        scroll.textView = view
        return (scroll, view)
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ReplyEditor
        let measurer = ReplyMeasurer()
        /// The binding's last value that was acted on.
        var wantsFocus = false
        var scheme: ColorScheme?

        init(_ parent: ReplyEditor) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            if parent.text != view.string { parent.text = view.string }
        }

        func textView(_ view: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.insertNewline(_:)):
                // Never while an input method still has text marked: Return then picks the candidate.
                guard !view.hasMarkedText() else { return false }
                let flags = NSApp.currentEvent?.modifierFlags ?? []
                if flags.contains(.option) || flags.contains(.shift) {
                    view.insertNewlineIgnoringFieldEditor(nil)
                } else {
                    parent.onSubmit()
                }
                return true
            case #selector(NSResponder.insertTab(_:)):
                // Tab that completes was taken by the window's key handler; otherwise it moves on, as in a field.
                view.window?.selectNextKeyView(nil)
                return true
            case #selector(NSResponder.insertBacktab(_:)):
                view.window?.selectPreviousKeyView(nil)
                return true
            case #selector(NSResponder.cancelOperation(_:)):
                // Escape would open the text view's word-completion list.
                return true
            default:
                return false
            }
        }

        func focusChanged(_ on: Bool) {
            wantsFocus = on
            if parent.focused != on { parent.focused = on }
        }

        func applyFocus(_ on: Bool, to view: ReplyTextView, attempt: Int = 0) {
            // Not inside SwiftUI's update pass, and after the view has a window (the first update comes before it).
            DispatchQueue.main.asyncAfter(deadline: .now() + (attempt == 0 ? 0 : 0.1)) { [weak self, weak view] in
                guard let view else { return }
                guard let window = view.window else {
                    if attempt < 5 { self?.applyFocus(on, to: view, attempt: attempt + 1) }
                    return
                }
                if on, window.firstResponder !== view {
                    window.makeFirstResponder(view)
                } else if !on, window.firstResponder === view {
                    window.makeFirstResponder(nil)
                }
            }
        }
    }
}

/// Puts the scroll position back at the top while all the text fits: typing a new line scrolls the box
/// up one line before SwiftUI has made it taller, and it would stay scrolled with the first line cut off.
final class ReplyScrollView: NSScrollView {
    var textView: ReplyTextView!

    override func layout() {
        super.layout()
        guard let document = documentView, document.frame.height <= contentSize.height + 0.5,
              contentView.bounds.origin.y != 0 else { return }
        contentView.scroll(to: .zero)
        reflectScrolledClipView(contentView)
    }
}

final class ReplyTextView: NSTextView {
    /// Shown while the box is empty; drawn here so it sits exactly where the text will.
    var placeholder = NSAttributedString() {
        didSet { if placeholder != oldValue { needsDisplay = true } }
    }
    var onFocusChange: ((Bool) -> Void)?

    override var string: String {
        didSet { if string.isEmpty != oldValue.isEmpty { needsDisplay = true } }
    }

    override func didChangeText() {
        super.didChangeText()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty, placeholder.length > 0, let container = textContainer else { return }
        let origin = NSPoint(x: textContainerInset.width + container.lineFragmentPadding, y: textContainerInset.height)
        let rect = NSRect(x: origin.x, y: origin.y, width: max(0, bounds.width - origin.x * 2),
                          height: ceil(layoutManager?.defaultLineHeight(for: font ?? KitType.body.nsFont) ?? 16))
        placeholder.draw(with: rect, options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }

    /// Files dragged over the box go to the conversation's drop target (the paths go into the reply), not
    /// into this view, which would take the drop and insert nothing.
    override func updateDragTypeRegistration() {}

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func becomeFirstResponder() -> Bool {
        let done = super.becomeFirstResponder()
        if done { onFocusChange?(true) }
        return done
    }

    override func resignFirstResponder() -> Bool {
        let done = super.resignFirstResponder()
        if done { onFocusChange?(false) }
        return done
    }
}
