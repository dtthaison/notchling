import AppKit
import ConsoleCore
import SwiftUI

/// "Điện thoại": turn the bridge on, pair a phone, see paired devices and what they did.
struct BridgeSettingsView: View {
    @Bindable var model: ConsoleModel
    @State private var confirmRegenerate = false
    @State private var copiedFingerprint = false
    @State private var resumeOnStart = true

    private var bridge: BridgeController { model.bridge }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Điện thoại").font(TypeToken.titleMd).foregroundStyle(Token.onSurface).accessibilityAddTraits(.isHeader)
                Spacer()
                Button("Xong") { bridge.showingSettings = false }
                    .buttonStyle(SecondaryButtonStyle()).keyboardShortcut(.cancelAction)
            }
            .padding(Space.lg)
            Divider1()
            ScrollView {
                VStack(alignment: .leading, spacing: Space.xl) {
                    toggleSection
                    internetSection
                    hostSection
                    if bridge.isRunning { pairingSection }
                    devicesSection
                    logSection
                    if bridge.fingerprint != nil { certificateSection }
                }
                .padding(Space.lg)
            }
        }
        .frame(width: 620, height: 720)
        .background(Token.surface)
        .preferredColorScheme(.dark)
        .onAppear {
            bridge.refresh()
            resumeOnStart = model.terminals.resumeOnStart
        }
        .task {
            // Last seen times and the log move while the sheet is open.
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                bridge.refresh()
            }
        }
        .confirmationDialog("Tạo lại chứng chỉ?", isPresented: $confirmRegenerate) {
            Button("Tạo lại và bỏ ghép đôi tất cả", role: .destructive) { bridge.regenerateCertificate() }
            Button("Huỷ", role: .cancel) {}
        } message: {
            Text("Mọi điện thoại đã ghép sẽ bị gỡ và phải ghép đôi lại.")
        }
    }

    // MARK: - Sections

    private var toggleSection: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            Toggle(isOn: Binding(get: { bridge.isEnabled }, set: { bridge.setEnabled($0) })) {
                Text("Cho phép điện thoại kết nối").font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
            }
            .toggleStyle(.switch)
            .accessibilityLabel("Cho phép điện thoại kết nối")
            HStack(spacing: Space.sm) {
                statusDot
                Text(statusText).font(TypeToken.bodySm).foregroundStyle(statusColor)
            }
            Text("Tắt mặc định. Chỉ nhận kết nối từ máy này, mạng LAN riêng và Tailscale, qua TLS với chứng chỉ ghim vân tay.")
                .font(TypeToken.bodySm).foregroundStyle(Token.muted)
        }
    }

    // MARK: - Internet access (Cloudflare tunnel)

    private func accessBinding(_ keyPath: WritableKeyPath<AccessConfig, String>) -> Binding<String> {
        Binding(get: { bridge.access[keyPath: keyPath] }, set: { value in bridge.updateAccess { $0[keyPath: keyPath] = value } })
    }

    private var internetSection: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            Toggle(isOn: Binding(get: { bridge.access.enabled }, set: { value in bridge.updateAccess { $0.enabled = value } })) {
                Text("Truy cập qua internet").font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
            }
            .toggleStyle(.switch)
            .accessibilityLabel("Truy cập qua internet")
            Text("Cho điện thoại ngoài mạng nhà vào qua đường hầm Cloudflare. Mỗi kết nối phải có token Cloudflare Access hợp lệ, rồi vẫn cần token ghép đôi như thường.")
                .font(TypeToken.bodySm).foregroundStyle(Token.muted)
            Text("cloudflared → http://127.0.0.1:\(TunnelOriginServer.port)").font(TypeToken.codeSm).foregroundStyle(Token.muted).textSelection(.enabled)
            if bridge.access.enabled {
                VStack(alignment: .leading, spacing: Space.sm) {
                    accessField("Tên miền", text: accessBinding(\.hostname))
                    accessField("Team domain của Access", text: accessBinding(\.teamDomain))
                    accessField("AUD tag của ứng dụng Access", text: accessBinding(\.audience))
                    accessField("Service token: Client ID", text: accessBinding(\.clientId))
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Service token: Client Secret").font(TypeToken.labelSm).foregroundStyle(Token.muted)
                        SecureField("", text: accessBinding(\.clientSecret))
                            .textFieldStyle(.roundedBorder).font(TypeToken.codeSm)
                            .accessibilityLabel("Service token Client Secret")
                    }
                }
                if !bridge.access.isComplete {
                    Text("Điền đủ các ô trên để mã QR có kèm đường hầm.").font(TypeToken.bodySm).foregroundStyle(Token.statusInput)
                }
                Text(tunnelStatusText).font(TypeToken.bodySm).foregroundStyle(Token.muted)
            }
        }
    }

    private func accessField(_ title: String, text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(TypeToken.labelSm).foregroundStyle(Token.muted)
            TextField("", text: text)
                .textFieldStyle(.roundedBorder).font(TypeToken.codeSm).autocorrectionDisabled()
                .accessibilityLabel(title)
        }
    }

    private var tunnelStatusText: String {
        if let last = bridge.lastTunnelConnection { return "Kết nối qua đường hầm gần nhất: \(Self.format(last))" }
        return "Chưa có kết nối nào qua đường hầm kể từ khi mở app."
    }

    /// Sessions live in hostd, so they survive Agent Console restarting. If hostd itself is stopped (a reboot, an
    /// update), it can bring them back.
    @ViewBuilder
    private var hostSection: some View {
        if model.terminals.sessionsOutliveApp {
            VStack(alignment: .leading, spacing: Space.sm) {
                Toggle(isOn: Binding(get: { resumeOnStart }, set: { resumeOnStart = $0; model.terminals.resumeOnStart = $0 })) {
                    Text("Tự mở lại session khi hostd khởi động lại").font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
                }
                .toggleStyle(.switch)
                Text("Session do Console mở chạy trong hostd nên vẫn sống khi bạn thoát hay cài lại Agent Console. Sau khi khởi động lại máy, hostd chạy \"claude --resume\" cho các session đó.")
                    .font(TypeToken.bodySm).foregroundStyle(Token.muted)
            }
        }
    }

    private var statusDot: some View {
        Circle().fill(bridge.isRunning ? Token.statusRunning : (isFailed ? Token.statusError : Token.statusExited)).frame(width: 8, height: 8)
            .accessibilityHidden(true)
    }

    private var isFailed: Bool { if case .failed = bridge.status { return true } else { return false } }

    private var statusText: String {
        switch bridge.status {
        case .off: return "Đang tắt"
        case .starting: return "Đang khởi động..."
        case .running(let port): return "Đang chạy trên cổng \(port)"
        case .failed(let message): return "Lỗi: \(message)"
        }
    }

    private var statusColor: Color {
        switch bridge.status {
        case .running: return Token.statusRunning
        case .failed: return Token.statusError
        default: return Token.muted
        }
    }

    private var pairingSection: some View {
        VStack(alignment: .leading, spacing: Space.md) {
            Text("Ghép đôi").panelTitleStyle()
            if let pairing = bridge.pairing {
                HStack(alignment: .top, spacing: Space.lg) {
                    Image(nsImage: pairing.qr).interpolation(.none).resizable().frame(width: 200, height: 200)
                        .padding(Space.sm).background(Color.white, in: RoundedRectangle(cornerRadius: Radius.md))
                        .accessibilityLabel("Mã QR ghép đôi")
                    VStack(alignment: .leading, spacing: Space.sm) {
                        Text("Quét mã QR bằng app trên điện thoại, hoặc nhập mã:").font(TypeToken.bodySm).foregroundStyle(Token.muted)
                        Text(pairing.code).font(.custom("GeistMono-Medium", fixedSize: 22)).foregroundStyle(Token.onSurface).textSelection(.enabled)
                        TimelineView(.periodic(from: .now, by: 1)) { context in
                            let left = Int(pairing.expiresAt.timeIntervalSince(context.date))
                            Text(left > 0 ? "Hết hạn sau \(left / 60):\(String(format: "%02d", left % 60))" : "Đã hết hạn")
                                .labelSmStyle().foregroundStyle(left > 0 ? Token.muted : Token.statusError)
                        }
                        Text("Máy này sẽ hỏi lại trước khi cho điện thoại vào.").font(TypeToken.bodySm).foregroundStyle(Token.muted)
                        Text(pairing.hosts.joined(separator: "\n")).font(TypeToken.codeSm).foregroundStyle(Token.muted).textSelection(.enabled)
                        HStack(spacing: Space.sm) {
                            Button("Mã mới") { bridge.startPairing() }.buttonStyle(SecondaryButtonStyle())
                            Button("Đóng") { bridge.cancelPairing() }.buttonStyle(SecondaryButtonStyle())
                        }
                    }
                }
            } else {
                Button { bridge.startPairing() } label: { Label("Ghép đôi điện thoại", systemImage: "qrcode") }
                    .buttonStyle(PrimaryButtonStyle())
                    .accessibilityLabel("Ghép đôi điện thoại")
            }
        }
    }

    private var devicesSection: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            Text("Thiết bị đã ghép (\(bridge.devices.count))").panelTitleStyle()
            if bridge.devices.isEmpty {
                Text("Chưa có điện thoại nào.").font(TypeToken.bodySm).foregroundStyle(Token.muted)
            }
            ForEach(bridge.devices) { device in
                HStack(spacing: Space.md) {
                    Image(systemName: device.platform.lowercased().contains("android") ? "smartphone" : "iphone")
                        .foregroundStyle(Token.muted).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(device.name).font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
                        Text("\(device.platform) · ghép \(Self.format(device.pairedAt)) · thấy lần cuối \(device.lastSeen.map(Self.format) ?? "chưa")")
                            .font(TypeToken.bodySm).foregroundStyle(Token.muted)
                    }
                    Spacer()
                    Button("Thu hồi") { bridge.revoke(device) }.buttonStyle(SecondaryButtonStyle())
                        .accessibilityLabel("Thu hồi \(device.name)")
                }
                .padding(Space.sm)
                .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))
            }
        }
    }

    private var logSection: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            Text("Nhật ký điều khiển từ xa (50 gần nhất)").panelTitleStyle()
            if bridge.log.isEmpty {
                Text("Chưa có lệnh nào từ điện thoại.").font(TypeToken.bodySm).foregroundStyle(Token.muted)
            }
            ForEach(Array(bridge.log.enumerated()), id: \.offset) { _, entry in
                HStack(spacing: Space.sm) {
                    Text(Self.format(entry.time)).font(TypeToken.codeSm).foregroundStyle(Token.muted)
                    Text(entry.op).font(TypeToken.codeSm).foregroundStyle(Token.onSurface)
                    Text(entry.device).font(TypeToken.bodySm).foregroundStyle(Token.muted).lineLimit(1)
                    if let session = entry.session { Text(String(session.prefix(8))).font(TypeToken.codeSm).foregroundStyle(Token.muted) }
                    if let length = entry.length { Text("\(length) ký tự").labelSmStyle().foregroundStyle(Token.muted) }
                    if let address = entry.address { Text(address).font(TypeToken.codeSm).foregroundStyle(Token.muted) }
                    Spacer()
                }
            }
        }
    }

    private var certificateSection: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            Text("Chứng chỉ").panelTitleStyle()
            Text("Vân tay SHA-256").font(TypeToken.labelSm).foregroundStyle(Token.muted)
            Text(bridge.fingerprint ?? "").font(TypeToken.codeSm).foregroundStyle(Token.onSurface).textSelection(.enabled)
            HStack(spacing: Space.sm) {
                Button(copiedFingerprint ? "Đã chép" : "Chép vân tay") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(bridge.fingerprint ?? "", forType: .string)
                    copiedFingerprint = true
                }.buttonStyle(SecondaryButtonStyle())
                Button("Tạo lại chứng chỉ") { confirmRegenerate = true }.buttonStyle(SecondaryButtonStyle())
            }
        }
    }

    private static func format(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "dd/MM HH:mm:ss"
        return formatter.string(from: date)
    }
}

/// A phone entered a valid pairing code; the person at the Mac decides.
struct ApprovalSheet: View {
    let approval: PairingApproval
    @Bindable var model: ConsoleModel

    var body: some View {
        VStack(alignment: .leading, spacing: Space.lg) {
            HStack(spacing: Space.md) {
                Image(systemName: "iphone.gen3.radiowaves.left.and.right").font(.system(size: 22)).foregroundStyle(Token.statusInput)
                    .accessibilityHidden(true)
                Text("Cho phép điện thoại này kết nối?").font(TypeToken.titleMd).foregroundStyle(Token.onSurface)
                    .accessibilityAddTraits(.isHeader)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(approval.deviceName).font(TypeToken.bodyMdStrong).foregroundStyle(Token.onSurface)
                Text(approval.platform).font(TypeToken.codeSm).foregroundStyle(Token.muted)
            }
            Text("Điện thoại này sẽ xem được các session Claude trên Mac và nhắn vào những session Console mở. Chỉ cho phép khi chính bạn vừa quét mã.")
                .font(TypeToken.bodySm).foregroundStyle(Token.muted)
            HStack(spacing: Space.sm) {
                Spacer()
                Button("Từ chối") { model.bridge.resolveApproval(false) }
                    .buttonStyle(SecondaryButtonStyle()).keyboardShortcut(.cancelAction)
                Button("Cho phép") { model.bridge.resolveApproval(true) }
                    .buttonStyle(PrimaryButtonStyle()).keyboardShortcut(.defaultAction)
                    .accessibilityLabel("Cho phép \(approval.deviceName)")
            }
        }
        .padding(Space.xl)
        .frame(width: 460)
        .background(Token.surface)
        .preferredColorScheme(.dark)
        .interactiveDismissDisabled()
    }
}
