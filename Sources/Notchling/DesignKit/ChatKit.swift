import AppKit
import SwiftUI

// The Chat window's design kit (BAMA design-kit method). Spec: docs/chat-kit.md.
// Views read KitInk, KitType, KitMetric, KitMotion and KitIcon; only this file reads KitPalette.

// MARK: - Layer 1: palette

private enum KitPalette {
    static let night0 = Color(hex: 0x0F0E0D)
    static let night1 = Color(hex: 0x151311)
    static let night2 = Color(hex: 0x1B1917)
    static let night3 = Color(hex: 0x242120)
    static let night4 = Color(hex: 0x2F2B28)
    static let night5 = Color(hex: 0x3B3632)
    static let paper0 = Color(hex: 0xFFFFFF)
    static let paper1 = Color(hex: 0xF4F0EA)
    static let paper2 = Color(hex: 0xFAF8F5)
    static let paper4 = Color(hex: 0xECE6DD)
    static let paper5 = Color(hex: 0xDED5C9)
    static let snow = Color(hex: 0xF3EEE8)
    static let fog = Color(hex: 0xA69D93)
    static let smoke = Color(hex: 0x736B62)
    static let soot = Color(hex: 0x1F1B17)
    static let stone = Color(hex: 0x6C6359)
    static let pebble = Color(hex: 0x9B9187)
    static let orange300 = Color(hex: 0xFFB47E)
    static let orange500 = Color(hex: 0xF7873B)
    static let orange700 = Color(hex: 0xB4540E)
    static let lime500 = Color(hex: 0x7DCB2C)
    static let blue400 = Color(hex: 0x6EB0FF)
    static let blue600 = Color(hex: 0x2C78D4)

    /// Avatar gradients: warm and cool pairs that read on both surfaces.
    static let avatars: [(Color, Color)] = [
        (Color(hex: 0xF7873B), Color(hex: 0xE0533A)), (Color(hex: 0x7DCB2C), Color(hex: 0x2E9E5B)),
        (Color(hex: 0x6EB0FF), Color(hex: 0x4A62E8)), (Color(hex: 0xC48BFF), Color(hex: 0x8A4FE0)),
        (Color(hex: 0xFFC94D), Color(hex: 0xF0913A)), (Color(hex: 0x4FD1C5), Color(hex: 0x2B8FB3)),
        (Color(hex: 0xFF8FB1), Color(hex: 0xD9507E)), (Color(hex: 0xB7A68F), Color(hex: 0x7D6B55)),
    ]
}

// MARK: - Layer 2: meaning

enum KitInk {
    static let window = Color(light: KitPalette.paper2, dark: KitPalette.night2)
    static let sidebar = Color(light: KitPalette.paper1, dark: KitPalette.night1)
    static let card = Color(light: KitPalette.paper0, dark: KitPalette.night3)
    static let raised = Color(light: KitPalette.paper4, dark: KitPalette.night4)
    static let selection = Color(light: KitPalette.orange500.opacity(0.12), dark: KitPalette.orange500.opacity(0.16))
    static let hairline = Color(light: KitPalette.paper5, dark: KitPalette.night5)
    static let text = Color(light: KitPalette.soot, dark: KitPalette.snow)
    static let textMuted = Color(light: KitPalette.stone, dark: KitPalette.fog)
    static let textFaint = Color(light: KitPalette.pebble, dark: KitPalette.smoke)
    static let accent = KitPalette.orange500
    static let accentText = Color(light: KitPalette.orange700, dark: KitPalette.orange300)
    /// Text on an accent fill: dark, because white fails on orange.
    static let onAccent = Color(light: KitPalette.soot, dark: KitPalette.night0)
    static let accentWash = Color(light: KitPalette.orange500.opacity(0.14), dark: KitPalette.orange500.opacity(0.18))
    static let needs = KitPalette.orange500
    static let working = Color(light: KitPalette.blue600, dark: KitPalette.blue400)
    static let done = KitPalette.lime500
    static let shadow = Color(light: Color.black.opacity(0.08), dark: Color.black.opacity(0.45))

    static func status(_ group: AgentGroup) -> Color {
        switch group {
        case .needsInput: return needs
        case .working: return working
        case .completed: return done
        }
    }

    /// A session keeps its colors: picked from its name.
    static func avatar(for name: String) -> (Color, Color) {
        var hash: UInt64 = 1469598103934665603
        for byte in name.utf8 { hash = (hash ^ UInt64(byte)) &* 1099511628211 }
        // The low bits of FNV-1a spread poorly over a few buckets; use the high ones.
        return KitPalette.avatars[Int((hash >> 40) % UInt64(KitPalette.avatars.count))]
    }
}

// MARK: - Layer 3: scales

enum KitMetric {
    static let sidebarWidth: CGFloat = 272
    static let titleBar: CGFloat = 38
    static let control: CGFloat = 28
    static let sessionRow: CGFloat = 52
    static let radiusSm: CGFloat = 6
    static let radiusMd: CGFloat = 10
    static let radiusCard: CGFloat = 14
    static let radiusComposer: CGFloat = 18
}

enum KitMotion {
    private static var reduced: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }
    static var hover: Animation? { reduced ? nil : .easeOut(duration: 0.12) }
    static var press: Animation? { reduced ? nil : .spring(response: 0.25, dampingFraction: 0.7) }
    static var appear: Animation? { reduced ? nil : .easeOut(duration: 0.2) }
}

enum KitType {
    case title, heading, body, label, meta, section, mono

    var font: Font {
        switch self {
        case .title: return .system(size: 17, weight: .semibold)
        case .heading: return .system(size: 13, weight: .semibold)
        case .body: return .system(size: 13)
        case .label: return .system(size: 12, weight: .medium)
        case .meta: return .system(size: 11)
        case .section: return .system(size: 10.5, weight: .semibold)
        case .mono: return .system(size: 12, design: .monospaced)
        }
    }
}

extension View {
    func kitType(_ type: KitType) -> some View {
        Group {
            if type == .section { font(type.font).tracking(0.6).textCase(.uppercase) } else { font(type.font) }
        }
    }

    /// The kit draws its own focus rings (macOS 14+ can turn the system one off).
    @ViewBuilder func kitNoFocusRing() -> some View {
        if #available(macOS 14.0, *) { focusEffectDisabled() } else { self }
    }

    /// A card: fill, hairline, radius.
    func kitCard(radius: CGFloat = KitMetric.radiusCard, fill: Color = KitInk.card) -> some View {
        background(RoundedRectangle(cornerRadius: radius, style: .continuous).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
    }
}

// MARK: - Icons

enum KitIcon {
    case needs, working, done, terminal, resume, send, tools, question, permission, claude, search, info, back, sessions

    var symbol: String {
        switch self {
        case .needs: return "exclamationmark.bubble.fill"
        case .working: return "circle.dotted"
        case .done: return "checkmark.circle.fill"
        case .terminal: return "terminal"
        case .resume: return "arrow.clockwise"
        case .send: return "arrow.up"
        case .tools: return "wrench.and.screwdriver"
        case .question: return "questionmark.bubble"
        case .permission: return "lock.shield"
        case .claude: return "sparkle"
        case .search: return "magnifyingglass"
        case .info: return "info.circle"
        case .back: return "arrow.uturn.backward"
        case .sessions: return "bubble.left.and.bubble.right"
        }
    }

    static func status(_ group: AgentGroup) -> KitIcon {
        switch group {
        case .needsInput: return .needs
        case .working: return .working
        case .completed: return .done
        }
    }
}

struct KitIconView: View {
    let icon: KitIcon
    var size: CGFloat = 12

    var body: some View {
        Image(systemName: icon.symbol).font(.system(size: size, weight: .medium))
    }
}

// MARK: - Part states

enum KitPartState: String, CaseIterable {
    case rest, hover, pressed, selected, focused, disabled
}

private struct ForcedPartStateKey: EnvironmentKey {
    static let defaultValue: KitPartState? = nil
}

extension EnvironmentValues {
    /// Set by the gallery so hover and pressed show up in a snapshot.
    var kitForcedState: KitPartState? {
        get { self[ForcedPartStateKey.self] }
        set { self[ForcedPartStateKey.self] = newValue }
    }
}

// MARK: - Buttons

struct KitButtonStyle: ButtonStyle {
    enum Kind { case primary, secondary, ghost }
    var kind: Kind = .secondary

    func makeBody(configuration: Configuration) -> some View {
        KitButtonBody(configuration: configuration, kind: kind)
    }
}

extension ButtonStyle where Self == KitButtonStyle {
    static func kit(_ kind: KitButtonStyle.Kind = .secondary) -> KitButtonStyle { KitButtonStyle(kind: kind) }
}

private struct KitButtonBody: View {
    let configuration: ButtonStyleConfiguration
    let kind: KitButtonStyle.Kind
    @Environment(\.kitForcedState) private var forced
    @Environment(\.isEnabled) private var isEnabled
    @State private var hovering = false

    private var state: KitPartState {
        forced ?? (!isEnabled ? .disabled : configuration.isPressed ? .pressed : hovering ? .hover : .rest)
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
        configuration.label
            .kitType(.label)
            .padding(.horizontal, 12)
            .frame(height: KitMetric.control)
            .foregroundColor(kind == .primary ? KitInk.onAccent : KitInk.text)
            .background(shape.fill(fill))
            .overlay(shape.strokeBorder(kind == .secondary ? KitInk.hairline : .clear, lineWidth: 1))
            .overlay(shape.strokeBorder(KitInk.accent, lineWidth: state == .focused ? 2 : 0))
            .opacity(state == .disabled ? 0.4 : 1)
            .scaleEffect(state == .pressed ? 0.97 : 1)
            .animation(KitMotion.press, value: state)
            .contentShape(shape)
            .onHover { inside in
                hovering = inside
                if isEnabled { if inside { NSCursor.pointingHand.push() } else { NSCursor.pop() } }
            }
    }

    private var fill: Color {
        let lit = state == .hover || state == .pressed
        switch kind {
        case .primary: return KitInk.accent.opacity(state == .pressed ? 0.8 : lit ? 0.9 : 1)
        case .secondary: return lit ? KitInk.raised : KitInk.card
        case .ghost: return lit ? KitInk.raised : .clear
        }
    }
}

// MARK: - Session row

struct KitSessionRowStyle: ButtonStyle {
    var selected: Bool
    var focused: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        RowBody(configuration: configuration, selected: selected, focused: focused)
    }

    private struct RowBody: View {
        let configuration: Configuration
        let selected: Bool
        let focused: Bool
        @Environment(\.kitForcedState) private var forced
        @State private var hovering = false

        var body: some View {
            let state = forced ?? (configuration.isPressed ? .pressed : selected ? .selected : hovering ? .hover : .rest)
            let shape = RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            configuration.label
                .padding(.horizontal, 10)
                .frame(maxWidth: .infinity, minHeight: KitMetric.sessionRow, alignment: .leading)
                .background(shape.fill(fill(state)))
                .overlay(shape.strokeBorder(KitInk.accent.opacity(state == .selected ? 0.35 : 0), lineWidth: 1))
                .overlay(shape.strokeBorder(KitInk.accent, lineWidth: focused && selected ? 2 : 0))
                .contentShape(shape)
                .onHover { hovering = $0 }
                .animation(KitMotion.hover, value: hovering)
        }

        private func fill(_ state: KitPartState) -> Color {
            switch state {
            case .selected: return KitInk.selection
            case .pressed: return KitInk.raised
            case .hover: return KitInk.raised.opacity(0.6)
            default: return .clear
            }
        }
    }
}

// MARK: - Option (a question's answer)

struct KitOptionStyle: ButtonStyle {
    let number: Int

    func makeBody(configuration: Configuration) -> some View {
        OptionBody(configuration: configuration, number: number)
    }

    private struct OptionBody: View {
        let configuration: Configuration
        let number: Int
        @Environment(\.kitForcedState) private var forced
        @Environment(\.isEnabled) private var isEnabled
        @State private var hovering = false

        var body: some View {
            let state = forced ?? (!isEnabled ? .disabled : configuration.isPressed ? .pressed : hovering ? .hover : .rest)
            let shape = RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            HStack(alignment: .top, spacing: 10) {
                Text("\(number)")
                    .kitType(.label)
                    .foregroundColor(state == .hover || state == .pressed ? KitInk.onAccent : KitInk.accentText)
                    .frame(width: 22, height: 22)
                    .background(Circle().fill(state == .hover || state == .pressed ? KitInk.accent : KitInk.selection))
                configuration.label
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(10)
            .background(shape.fill(state == .hover || state == .pressed ? KitInk.raised : KitInk.card))
            .overlay(shape.strokeBorder(state == .pressed ? KitInk.accent : KitInk.hairline, lineWidth: 1))
            .opacity(state == .disabled ? 0.4 : 1)
            .contentShape(shape)
            .onHover { inside in
                hovering = inside
                if isEnabled { if inside { NSCursor.pointingHand.push() } else { NSCursor.pop() } }
            }
            .animation(KitMotion.hover, value: hovering)
        }
    }
}

// MARK: - Small parts

/// Initials on the session's own gradient, with an optional status badge.
struct KitAvatar: View {
    let name: String
    var size: CGFloat = 30
    var status: AgentGroup?
    var badgeRing: Color = KitInk.sidebar

    var body: some View {
        let colors = KitInk.avatar(for: name)
        Text(Self.initials(name))
            .font(.system(size: size * 0.4, weight: .bold, design: .rounded))
            .foregroundColor(.white)
            .frame(width: size, height: size)
            .background(Circle().fill(LinearGradient(colors: [colors.0, colors.1], startPoint: .topLeading, endPoint: .bottomTrailing)))
            .overlay(alignment: .bottomTrailing) {
                if let status {
                    Circle().fill(KitInk.status(status))
                        .frame(width: size * 0.32, height: size * 0.32)
                        .overlay(Circle().strokeBorder(badgeRing, lineWidth: 2))
                        .offset(x: 2, y: 2)
                }
            }
            .accessibilityHidden(true)
    }

    static func initials(_ name: String) -> String {
        let words = name.split(whereSeparator: { !$0.isLetter && !$0.isNumber })
        let letters = words.prefix(2).compactMap(\.first)
        return letters.isEmpty ? "?" : String(letters).uppercased()
    }
}

/// Claude's mark: a sparkle on an orange disc.
struct KitClaudeMark: View {
    var size: CGFloat = 24

    var body: some View {
        KitIconView(icon: .claude, size: size * 0.5)
            .foregroundColor(KitInk.onAccent)
            .frame(width: size, height: size)
            .background(Circle().fill(LinearGradient(colors: [Color(hex: 0xFFB47E), KitInk.accent],
                                                     startPoint: .topLeading, endPoint: .bottomTrailing)))
            .accessibilityHidden(true)
    }
}

/// Status as a colored capsule: icon and word, never color alone.
struct KitStatusPill: View {
    let group: AgentGroup
    let text: String

    var body: some View {
        HStack(spacing: 4) {
            KitIconView(icon: .status(group), size: 10)
            Text(text).kitType(.label)
        }
        .foregroundColor(group == .needsInput ? KitInk.accentText : KitInk.status(group))
        .padding(.horizontal, 8).frame(height: 20)
        .background(Capsule().fill(KitInk.status(group).opacity(0.16)))
    }
}

/// A plain field with the kit's fill and focus border.
struct KitField: ViewModifier {
    var focused: Bool

    func body(content: Content) -> some View {
        content
            .textFieldStyle(.plain)
            .kitType(.body)
            .foregroundColor(KitInk.text)
            .padding(.horizontal, 10)
            .frame(minHeight: KitMetric.control)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.card))
            .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                .strokeBorder(focused ? KitInk.accent : KitInk.hairline, lineWidth: focused ? 1.5 : 1))
    }
}

/// The pet from the notch, for empty states. A missing drawing falls back to Claude's mark.
struct KitPet: View {
    let pose: String
    var size: CGFloat = 120

    var body: some View {
        if let image = Bundle.main.url(forResource: pose, withExtension: "png").flatMap(NSImage.init(contentsOf:)) {
            Image(nsImage: image).resizable().interpolation(.high).frame(width: size, height: size)
        } else {
            KitClaudeMark(size: size * 0.5)
        }
    }
}

// MARK: - Window chrome

/// Hides the title bar and keeps the traffic lights where AppKit puts them, over the sidebar's top.
struct KitWindowChrome: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { ChromeView() }
    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class ChromeView: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard let window else { return }
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            window.styleMask.insert(.fullSizeContentView)
            window.isMovableByWindowBackground = false
        }
    }
}

// MARK: - Gallery

/// Every part in every state, for review in light and dark (rendered by a probe, never shown in the app).
struct KitGallery: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Chat kit").kitType(.title).foregroundColor(KitInk.text)
            row("Buttons") {
                ForEach([KitButtonStyle.Kind.primary, .secondary, .ghost], id: \.self) { kind in
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach([KitPartState.rest, .hover, .pressed, .focused, .disabled], id: \.self) { state in
                            Button(state.rawValue.capitalized) {}.buttonStyle(.kit(kind))
                                .environment(\.kitForcedState, state)
                        }
                    }
                }
            }
            row("Session rows") {
                VStack(spacing: 4) {
                    ForEach([KitPartState.rest, .hover, .pressed, .selected], id: \.self) { state in
                        Button {} label: { sampleRow(state.rawValue) }
                            .buttonStyle(KitSessionRowStyle(selected: state == .selected, focused: false))
                            .environment(\.kitForcedState, state)
                    }
                    Button {} label: { sampleRow("selected + focused") }
                        .buttonStyle(KitSessionRowStyle(selected: true, focused: true))
                }
                .frame(width: 260)
                .padding(8)
                .background(KitInk.sidebar)
            }
            row("Options") {
                VStack(spacing: 6) {
                    ForEach(Array([KitPartState.rest, .hover, .pressed].enumerated()), id: \.offset) { i, state in
                        Button {} label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Option \(state.rawValue)").kitType(.heading).foregroundColor(KitInk.text)
                                Text("What this choice means").kitType(.meta).foregroundColor(KitInk.textMuted)
                            }
                        }
                        .buttonStyle(KitOptionStyle(number: i + 1))
                        .environment(\.kitForcedState, state)
                    }
                }
                .frame(width: 300)
            }
            row("Pills, avatars, field") {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        KitStatusPill(group: .needsInput, text: "Needs input")
                        KitStatusPill(group: .working, text: "Working")
                        KitStatusPill(group: .completed, text: "Done")
                    }
                    HStack {
                        ForEach(["design system", "task-status-check", "referral", "artifact-id-lookup", "wifi"], id: \.self) {
                            KitAvatar(name: $0, size: 30, status: .needsInput)
                        }
                        KitClaudeMark()
                    }
                    TextField("Search", text: .constant("")).modifier(KitField(focused: false)).frame(width: 220)
                    TextField("Search", text: .constant("focused")).modifier(KitField(focused: true)).frame(width: 220)
                }
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(KitInk.window)
    }

    private func row<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).kitType(.section).foregroundColor(KitInk.textFaint)
            HStack(alignment: .top, spacing: 16) { content() }
        }
    }

    private func sampleRow(_ state: String) -> some View {
        HStack(spacing: 10) {
            KitAvatar(name: "design system", size: 30, status: .needsInput)
            VStack(alignment: .leading, spacing: 2) {
                Text("design system").kitType(.heading).foregroundColor(KitInk.text)
                Text(state).kitType(.meta).foregroundColor(KitInk.textMuted)
            }
        }
    }
}

// MARK: - Color helpers

extension Color {
    init(hex: UInt32) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255,
                  green: Double((hex >> 8) & 0xFF) / 255, blue: Double(hex & 0xFF) / 255)
    }

    /// A color that follows light and dark mode.
    init(light: Color, dark: Color) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? NSColor(dark) : NSColor(light)
        })
    }
}
