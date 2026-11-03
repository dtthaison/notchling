import AppKit
import ConsoleCore
import SwiftUI

/// Layer-backed view that shows each streamed frame, keeping the device's aspect ratio.
private struct FrameHost: NSViewRepresentable {
    let capture: PhoneCapture

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.wantsLayer = true
        view.layer?.contentsGravity = .resizeAspect
        view.layer?.backgroundColor = NSColor(Token.neutral).cgColor
        view.setAccessibilityLabel("Màn hình thiết bị")
        capture.onFrame = { [weak view] surface in view?.layer?.contents = surface }
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {}
}

struct PhoneTab: View {
    @Bindable var model: ConsoleModel
    let session: Session
    @State private var capture = PhoneCapture()

    var body: some View {
        VStack(spacing: 0) {
            if capture.windows.count > 1 { picker }
            ZStack {
                FrameHost(capture: capture)
                    .opacity(isStreaming ? 1 : 0)
                if !isStreaming { stateView.panelEntrance() }
            }
        }
        .onAppear { capture.start(hints: model.viz.bundle?.deviceHints ?? []) }
        .onDisappear { capture.stop() }
        .onChange(of: model.viz.bundle?.deviceHints ?? []) { _, hints in capture.updateHints(hints) }
    }

    private var isStreaming: Bool {
        if case .streaming = capture.state { return true }
        return false
    }

    private var picker: some View {
        HStack(spacing: Space.sm) {
            Text("Thiết bị").font(TypeToken.labelSm).foregroundStyle(Token.muted)
            Picker("Cửa sổ thiết bị", selection: Binding(
                get: { capture.selectedWindowId ?? capture.windows.first?.id ?? 0 },
                set: { capture.select($0) })) {
                ForEach(capture.windows) { window in
                    Text(window.title.isEmpty ? window.appName : "\(window.title) · \(window.appName)").tag(window.id)
                }
            }
            .labelsHidden()
            .frame(maxWidth: 320)
            .accessibilityLabel("Chọn cửa sổ thiết bị")
            Spacer()
        }
        .padding(.horizontal, Space.md).frame(height: 36)
        .overlay(alignment: .bottom) { Divider1() }
    }

    @ViewBuilder
    private var stateView: some View {
        switch capture.state {
        case .idle, .scanning:
            SkeletonRows(count: 4).frame(maxHeight: .infinity, alignment: .top)
        case .noPermission:
            message(symbol: "lock.shield", title: "Chưa có quyền ghi màn hình",
                    detail: "Mở System Settings, Privacy & Security, Screen & System Audio Recording, rồi bật Agent Console. Có thể phải mở lại app sau khi bật.",
                    actions: [("Mở System Settings", { PhoneCapture.openScreenRecordingSettings() }),
                              ("Kiểm tra lại", { capture.start(hints: model.viz.bundle?.deviceHints ?? []) })])
        case .noWindow:
            message(symbol: "iphone.slash", title: "Chưa thấy cửa sổ máy ảo",
                    detail: "Mở DeviceHub (Xcode 27), Simulator hoặc Android Emulator rồi chọn một thiết bị. Console chỉ xem, không tự mở máy ảo.", actions: [])
        case .hidden(let title):
            message(symbol: "eye.slash", title: "Cửa sổ máy ảo đang ẩn",
                    detail: "\(title): mở DeviceHub để xem. Cửa sổ bị thu nhỏ hoặc ẩn thì không có hình để quay.", actions: [])
        case .streaming:
            EmptyView()
        case .failed(let reason):
            ErrorState(message: "Không quay được cửa sổ: \(reason)") { capture.start(hints: model.viz.bundle?.deviceHints ?? []) }
        }
    }

    private func message(symbol: String, title: String, detail: String, actions: [(String, () -> Void)]) -> some View {
        VStack(spacing: Space.md) {
            Image(systemName: symbol).font(.system(size: 26, weight: .light)).foregroundStyle(Token.muted).accessibilityHidden(true)
            Text(title).font(TypeToken.titleMd).foregroundStyle(Token.onSurface)
            Text(detail).font(TypeToken.bodyMd).foregroundStyle(Token.muted)
                .multilineTextAlignment(.center).frame(maxWidth: 400)
            if !actions.isEmpty {
                HStack(spacing: Space.sm) {
                    ForEach(Array(actions.enumerated()), id: \.offset) { _, action in
                        Button(action.0, action: action.1).buttonStyle(SecondaryButtonStyle())
                    }
                }
            }
        }
        .padding(Space.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }
}
