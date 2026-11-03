import SwiftUI

/// Pressed buttons shrink to 0.98 (DESIGN.md); Reduce Motion keeps only a dim.
struct PressModifier: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let isPressed: Bool

    func body(content: Content) -> some View {
        content
            .scaleEffect(isPressed && !reduceMotion ? 0.98 : 1)
            .opacity(isPressed && reduceMotion ? 0.85 : 1)
            .animation(reduceMotion ? nil : Motion.press, value: isPressed)
    }
}

/// The one primary button on screen: `primary` fill, `on-primary` text.
struct PrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(TypeToken.labelSm)
            .foregroundStyle(Token.onPrimary)
            .padding(.horizontal, Space.md)
            .frame(height: Metric.buttonHeight)
            .background(Token.primary.opacity(isEnabled ? 1 : 0.4), in: RoundedRectangle(cornerRadius: Radius.md))
            .modifier(PressModifier(isPressed: configuration.isPressed))
            .focusRing()
    }
}

struct SecondaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(TypeToken.labelSm)
            .foregroundStyle(isEnabled ? Token.onSurface : Token.muted)
            .padding(.horizontal, Space.md)
            .frame(height: Metric.buttonHeight)
            .background(Token.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.md))
            .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))
            .modifier(PressModifier(isPressed: configuration.isPressed))
            .focusRing()
    }
}

/// Icon-only button with a 24 px target.
struct IconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(Token.muted)
            .frame(width: 24, height: 24)
            .background(configuration.isPressed ? Token.surfaceRaised : Color.clear, in: RoundedRectangle(cornerRadius: Radius.sm))
            .contentShape(Rectangle())
            .focusRing()
    }
}

extension View {
    /// 2 px `primary` ring while the control has keyboard focus.
    func focusRing() -> some View {
        modifier(FocusRingModifier())
    }

    /// Panel and menu entrance: scale 0.96 and fade in 200 ms, ease-out.
    func panelEntrance() -> some View {
        modifier(EntranceModifier())
    }
}

private struct FocusRingModifier: ViewModifier {
    @Environment(\.isFocused) private var isFocused

    func body(content: Content) -> some View {
        content.overlay(
            RoundedRectangle(cornerRadius: Radius.md)
                .stroke(Token.primary, lineWidth: 2)
                .opacity(isFocused ? 1 : 0)
                .allowsHitTesting(false))
    }
}

private struct EntranceModifier: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var shown = false

    func body(content: Content) -> some View {
        content
            .opacity(shown ? 1 : 0)
            .scaleEffect(shown || reduceMotion ? 1 : 0.96)
            .onAppear {
                withAnimation(reduceMotion ? Motion.fade : Motion.enter) { shown = true }
            }
    }
}

enum Motion {
    /// cubic-bezier(0.22, 1, 0.36, 1): the ease-out from uix-skill motion rules.
    static let enter = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.2)
    static let hover = Animation.easeOut(duration: 0.15)
    static let stateChange = Animation.easeInOut(duration: 0.25)
    static let fade = Animation.easeOut(duration: 0.15)
    static let press = Animation.easeOut(duration: 0.12)
}

/// 1 px `border` divider; layers are separated by surface steps and this line, never shadows.
struct Divider1: View {
    var vertical = false

    var body: some View {
        Token.border.frame(width: vertical ? 1 : nil, height: vertical ? nil : 1)
    }
}
