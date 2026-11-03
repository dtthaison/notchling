import SwiftUI

/// Night Desk tokens from Console/DESIGN.md. Names follow the DESIGN.md token names in camelCase.
/// Nothing outside this folder writes a hex color, font name or radius.
enum Token {
    // colors
    static let neutral = Color(hex: 0x101319)
    static let surface = Color(hex: 0x171B23)
    static let surfaceRaised = Color(hex: 0x1E2330)
    static let border = Color(hex: 0x262C38)
    static let onSurface = Color(hex: 0xE5E9F0)
    static let muted = Color(hex: 0x9BA5B5)
    static let primary = Color(hex: 0x38BDF8)
    static let onPrimary = Color(hex: 0x04202E)
    static let statusInput = Color(hex: 0xF5B544)
    static let onStatusInput = Color(hex: 0x2A1B00)
    static let statusRunning = Color(hex: 0x34D399)
    static let statusError = Color(hex: 0xF87171)
    /// Only ever the 8 px dot: it is 3.7:1 on `surface`, so no text uses it.
    static let statusExited = Color(hex: 0x6B7488)
}

enum Radius {
    static let sm: CGFloat = 4
    static let md: CGFloat = 6
    static let lg: CGFloat = 8
}

enum Space {
    static let xs: CGFloat = 4
    static let sm: CGFloat = 8
    static let md: CGFloat = 12
    static let lg: CGFloat = 16
    static let xl: CGFloat = 24
}

enum Metric {
    static let topBarHeight: CGFloat = 40
    static let panelHeaderHeight: CGFloat = 32
    static let sessionRowHeight: CGFloat = 32
    static let subagentRowHeight: CGFloat = 28
    static let subagentIndent: CGFloat = 20
    static let inputHeight: CGFloat = 36
    static let buttonHeight: CGFloat = 28
    static let minWindow = CGSize(width: 1200, height: 760)
    static let defaultWindow = CGSize(width: 1440, height: 900)
    /// Tin dài tối đa 72ch: Geist 13 px is about 7 px per character.
    static let maxMessageWidth: CGFloat = 72 * 7
}

extension Color {
    init(hex: UInt32) {
        self.init(.sRGB,
                  red: Double((hex >> 16) & 0xFF) / 255,
                  green: Double((hex >> 8) & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255,
                  opacity: 1)
    }
}

/// DESIGN.md typography. Fixed point sizes: a dense desktop cockpit, and the tokens are in px.
enum TypeToken {
    static let panelTitle = Font.custom("Geist-SemiBold", fixedSize: 11)
    static let titleMd = Font.custom("Geist-SemiBold", fixedSize: 15)
    static let bodyMd = Font.custom("Geist-Regular", fixedSize: 13)
    static let bodyMdStrong = Font.custom("Geist-SemiBold", fixedSize: 13)
    static let bodySm = Font.custom("Geist-Regular", fixedSize: 12)
    static let labelSm = Font.custom("Geist-Medium", fixedSize: 11)
    static let codeSm = Font.custom("GeistMono-Regular", fixedSize: 12)
}

extension View {
    /// 1.5 line spacing for 13 px text is about 6.5 extra points over the 13 px glyph height.
    func bodyMdStyle() -> some View {
        font(TypeToken.bodyMd).lineSpacing(4).foregroundStyle(Token.onSurface)
    }

    func panelTitleStyle() -> some View {
        font(TypeToken.panelTitle).tracking(0.66).textCase(.uppercase).foregroundStyle(Token.muted)
    }

    func labelSmStyle() -> some View {
        font(TypeToken.labelSm).monospacedDigit()
    }
}
