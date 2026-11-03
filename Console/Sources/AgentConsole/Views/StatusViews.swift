import ConsoleCore
import SwiftUI

extension SessionState {
    var color: Color {
        switch self {
        case .needsInput: return Token.statusInput
        case .running: return Token.statusRunning
        case .done: return Token.muted
        case .exited: return Token.statusExited
        }
    }

    /// The label never uses `status-exited` (3.7:1): the dot does, the words use `muted`.
    var labelColor: Color {
        switch self {
        case .needsInput: return Token.statusInput
        case .running: return Token.statusRunning
        case .done, .exited: return Token.muted
        }
    }

    var symbol: String {
        switch self {
        case .needsInput: return "exclamationmark.bubble.fill"
        case .running: return "bolt.fill"
        case .done: return "checkmark"
        case .exited: return "power"
        }
    }

    /// Words for VoiceOver and group headers.
    var vietnamese: String {
        switch self {
        case .needsInput: return "Cần input"
        case .running: return "Đang chạy"
        case .done: return "Xong"
        case .exited: return "Đã thoát"
        }
    }
}

/// 8 px status dot. "Đang chạy" breathes between 0.45 and 1 opacity over 1.6 s, the only loop in the app.
struct StatusDot: View {
    let state: SessionState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var dimmed = false

    var body: some View {
        Circle()
            .fill(state.color)
            .frame(width: 8, height: 8)
            .opacity(state == .running && !reduceMotion && dimmed ? 0.45 : 1)
            .animation(Motion.stateChange, value: state)
            .onAppear { startBreathing() }
            .onChange(of: state) { _, _ in startBreathing() }
            .onChange(of: reduceMotion) { _, _ in startBreathing() }
            .accessibilityHidden(true)
    }

    private func startBreathing() {
        guard state == .running, !reduceMotion else {
            withAnimation(nil) { dimmed = false }
            return
        }
        dimmed = false
        withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) { dimmed = true }
    }
}

struct InputBadge: View {
    var text = "Cần input"

    var body: some View {
        Text(text)
            .labelSmStyle()
            .foregroundStyle(Token.onStatusInput)
            .padding(.horizontal, 6)
            .frame(height: 16)
            .background(Token.statusInput, in: RoundedRectangle(cornerRadius: Radius.sm))
            .accessibilityHidden(true)
    }
}

/// Short relative time with even-width digits, refreshed every 30 s.
struct RelativeTimeLabel: View {
    let date: Date

    var body: some View {
        TimelineView(.periodic(from: .now, by: 30)) { context in
            Text(Self.text(since: date, now: context.date))
                .labelSmStyle()
                .foregroundStyle(Token.muted)
        }
    }

    static func text(since date: Date, now: Date) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(date)))
        switch seconds {
        case ..<60: return "Vừa xong"
        case ..<3600: return "\(seconds / 60) phút"
        case ..<86_400: return "\(seconds / 3600) giờ"
        default: return "\(seconds / 86_400) ngày"
        }
    }

    static func spoken(since date: Date, now: Date) -> String {
        let text = Self.text(since: date, now: now)
        return text == "Vừa xong" ? "vừa cập nhật" : "cập nhật \(text) trước"
    }
}

/// Loading state: static rows in the exact 32 px footprint. No shimmer: nothing but the running dot loops.
struct SkeletonRows: View {
    var count = 5

    var body: some View {
        VStack(spacing: 0) {
            ForEach(0..<count, id: \.self) { index in
                HStack(spacing: Space.sm) {
                    Circle().fill(Token.surfaceRaised).frame(width: 8, height: 8)
                    RoundedRectangle(cornerRadius: Radius.sm).fill(Token.surfaceRaised)
                        .frame(width: CGFloat(120 + (index * 37) % 90), height: 10)
                    Spacer()
                    RoundedRectangle(cornerRadius: Radius.sm).fill(Token.surfaceRaised).frame(width: 36, height: 8)
                }
                .padding(.horizontal, Space.md)
                .frame(height: Metric.sessionRowHeight)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Đang tải danh sách session")
    }
}

struct ErrorState: View {
    let message: String
    let retry: () -> Void

    var body: some View {
        VStack(spacing: Space.md) {
            Image(systemName: "exclamationmark.triangle").font(.system(size: 18)).foregroundStyle(Token.statusError)
                .accessibilityHidden(true)
            Text(message).font(TypeToken.bodySm).foregroundStyle(Token.statusError).multilineTextAlignment(.center)
            Button("Thử lại", action: retry).buttonStyle(SecondaryButtonStyle())
                .accessibilityLabel("Thử lại")
        }
        .padding(Space.lg)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct PanelHeader<Trailing: View>: View {
    let title: String
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(spacing: Space.sm) {
            Text(title).panelTitleStyle().accessibilityAddTraits(.isHeader)
            Spacer(minLength: Space.sm)
            trailing()
        }
        .padding(.horizontal, Space.md)
        .frame(height: Metric.panelHeaderHeight)
        .background(Token.surface)
        .overlay(alignment: .bottom) { Divider1() }
    }
}
