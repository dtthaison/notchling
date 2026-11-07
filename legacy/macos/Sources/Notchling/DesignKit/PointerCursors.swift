import AppKit
import ObjectiveC

/// The pointing hand over every enabled AppKit button and control in the app, including the ones
/// SwiftUI draws with them (default buttons, switches and checkboxes, pop-up and segmented pickers,
/// steppers). SwiftUI views of our own get it from `pointerCursor()` and the kit's button styles.
/// Done by giving those classes a `resetCursorRects` that adds the hand over their bounds, so it
/// covers the control only, not a Form row's label beside it.
enum PointerCursors {
    private static var installed = false

    static func install() {
        guard !installed else { return }
        installed = true
        let sel = #selector(NSView.resetCursorRects)
        for cls in [NSButton.self, NSPopUpButton.self, NSSegmentedControl.self, NSSwitch.self, NSStepper.self] as [AnyClass] {
            // The implementation it has now (its own or inherited), called first so nothing is lost.
            typealias Reset = @convention(c) (AnyObject, Selector) -> Void
            let original = unsafeBitCast(class_getMethodImplementation(cls, sel), to: Reset.self)
            let block: @convention(block) (NSView) -> Void = { view in
                original(view, sel)
                guard let control = view as? NSControl, control.isEnabled, !view.isHidden else { return }
                view.addCursorRect(view.bounds, cursor: .pointingHand)
            }
            // Replaces it on this class only (adding it when only inherited), so other views keep theirs.
            class_replaceMethod(cls, sel, imp_implementationWithBlock(block), "v@:")
        }
    }
}
