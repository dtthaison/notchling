import AppKit
import ConsoleCore
import SwiftUI

/// The Phone button at the top right of Chat's header. It opens the pairing panel as a popover; a green dot on the
/// icon says the bridge is on.
struct PhoneButton: View {
    @ObservedObject var bridge: BridgeController
    @State private var shown = false

    var body: some View {
        Button { shown.toggle() } label: {
            ZStack(alignment: .topTrailing) {
                KitIconView(icon: .phone, size: 13)
                if bridge.isRunning {
                    Circle().fill(KitInk.done).frame(width: 6, height: 6).offset(x: 3, y: -3)
                }
            }
        }
        .buttonStyle(.kit(shown ? .secondary : .ghost))
        .help(bridge.isRunning ? "Phone: connected to the bridge. Pair or manage phones" : "Phone: pair a phone with this Mac")
        .accessibilityLabel("Phone")
        .popover(isPresented: $shown, arrowEdge: .bottom) { PhonePanel(bridge: bridge, model: PhoneService.shared.model) }
        .part("Phone Button", .bottomTrailing)
    }
}

/// Pairing, paired phones, the bridge switch and tunnel status. This is the old Agent Console "Điện thoại" sheet,
/// drawn with the Chat kit.
struct PhonePanel: View {
    @ObservedObject var bridge: BridgeController
    let model: PhoneModel
    @State private var confirmRegenerate = false
    @State private var copiedFingerprint = false
    @State private var resumeOnStart = true
    @State private var showInternet = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                KitIconView(icon: .phone, size: 14)
                Text("Phone").kitType(.title).foregroundColor(KitInk.text)
                Spacer()
                statusLabel
            }
            .padding(.horizontal, 16).padding(.vertical, 12)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    switchSection
                    if bridge.isRunning { pairingSection }
                    devicesSection
                    internetSection
                    hostSection
                    if bridge.fingerprint != nil { certificateSection }
                    activitySection
                }
                .padding(16)
            }
        }
        .frame(width: 400, height: 560)
        .background(KitInk.window)
        .onAppear {
            bridge.refresh()
            resumeOnStart = model.terminals.resumeOnStart
        }
        .task {
            // Last-seen times and the log move while the panel is open.
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                bridge.refresh()
            }
        }
        .alert("Make a new certificate?", isPresented: $confirmRegenerate) {
            Button("Make New and Unpair All", role: .destructive) { bridge.regenerateCertificate() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Every paired phone is removed and has to pair again.")
        }
    }

    // MARK: Sections

    private var statusLabel: some View {
        HStack(spacing: 6) {
            Circle().fill(statusColor).frame(width: 7, height: 7)
            Text(statusText).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
        }
    }

    private var switchSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            Toggle(isOn: Binding(get: { bridge.isEnabled }, set: { bridge.setEnabled($0) })) {
                Text("Let phones connect").kitType(.heading).foregroundColor(KitInk.text)
            }
            .toggleStyle(.switch)
            Text("Off by default. Accepts this Mac, the private LAN and Tailscale over TLS with a pinned fingerprint.")
                .kitType(.meta).foregroundColor(KitInk.textMuted)
        }
    }

    private var pairingSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle("Pair")
            if let pairing = bridge.pairing {
                HStack(alignment: .top, spacing: 14) {
                    Image(nsImage: pairing.qr).interpolation(.none).resizable().frame(width: 150, height: 150)
                        .padding(6).background(Color.white, in: RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous))
                        .accessibilityLabel("Pairing QR code")
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Scan with the phone app, or type the code:").kitType(.meta).foregroundColor(KitInk.textMuted)
                        Text(pairing.code).font(.system(size: 20, weight: .medium, design: .monospaced)).foregroundColor(KitInk.text).textSelection(.enabled)
                        TimelineView(.periodic(from: .now, by: 1)) { context in
                            let left = Int(pairing.expiresAt.timeIntervalSince(context.date))
                            Text(left > 0 ? "Expires in \(left / 60):\(String(format: "%02d", left % 60))" : "Expired")
                                .kitType(.meta).foregroundColor(left > 0 ? KitInk.textMuted : KitInk.needs)
                        }
                        Text("This Mac asks again before the phone gets in.").kitType(.meta).foregroundColor(KitInk.textFaint)
                        HStack(spacing: 8) {
                            Button("New Code") { bridge.startPairing() }.buttonStyle(.kit(.secondary))
                            Button("Close") { bridge.cancelPairing() }.buttonStyle(.kit(.ghost))
                        }
                    }
                }
                Text(pairing.hosts.joined(separator: "\n")).kitType(.mono).foregroundColor(KitInk.textFaint).textSelection(.enabled)
            } else {
                Button { bridge.startPairing() } label: { Label("Pair a Phone", systemImage: "qrcode") }
                    .buttonStyle(.kit(.primary))
            }
        }
    }

    private var devicesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("Paired phones (\(bridge.devices.count))")
            if bridge.devices.isEmpty {
                Text("No phones yet.").kitType(.meta).foregroundColor(KitInk.textMuted)
            }
            ForEach(bridge.devices) { device in
                HStack(spacing: 10) {
                    KitIconView(icon: .phone, size: 13).foregroundColor(KitInk.textMuted)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(device.name).kitType(.heading).foregroundColor(KitInk.text)
                        Text("\(device.platform) · paired \(Self.format(device.pairedAt)) · last seen \(device.lastSeen.map(Self.format) ?? "never")")
                            .kitType(.meta).foregroundColor(KitInk.textMuted)
                    }
                    Spacer(minLength: 4)
                    Button("Revoke") { bridge.revoke(device) }.buttonStyle(.kit(.secondary))
                        .accessibilityLabel("Revoke \(device.name)")
                }
                .padding(10).kitCard(radius: KitMetric.radiusMd)
            }
        }
    }

    private var internetSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle(isOn: Binding(get: { bridge.access.enabled }, set: { value in bridge.updateAccess { $0.enabled = value } })) {
                Text("Reach this Mac over the internet").kitType(.heading).foregroundColor(KitInk.text)
            }
            .toggleStyle(.switch)
            Text("Through a Cloudflare tunnel. Every connection needs a valid Cloudflare Access token, then the pairing token as usual.")
                .kitType(.meta).foregroundColor(KitInk.textMuted)
            Text("cloudflared → http://127.0.0.1:\(TunnelOriginServer.port)").kitType(.mono).foregroundColor(KitInk.textFaint).textSelection(.enabled)
            Text(tunnelStatusText).kitType(.meta).foregroundColor(KitInk.textMuted)
            if bridge.access.enabled {
                Button { withAnimation(KitMotion.appear) { showInternet.toggle() } } label: {
                    Label(showInternet ? "Hide Tunnel Settings" : "Tunnel Settings", systemImage: showInternet ? "chevron.down" : "chevron.right")
                }
                .buttonStyle(.kit(.ghost))
                if showInternet {
                    VStack(alignment: .leading, spacing: 8) {
                        field("Domain", accessBinding(\.hostname))
                        field("Access team domain", accessBinding(\.teamDomain))
                        field("Access application AUD tag", accessBinding(\.audience))
                        field("Service token: Client ID", accessBinding(\.clientId))
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Service token: Client Secret").kitType(.meta).foregroundColor(KitInk.textMuted)
                            SecureField("", text: accessBinding(\.clientSecret)).modifier(KitField(focused: false))
                                .accessibilityLabel("Service token client secret")
                        }
                        if !bridge.access.isComplete {
                            Text("Fill every field so the QR code carries the tunnel too.").kitType(.meta).foregroundColor(KitInk.accentText)
                        }
                    }
                }
            }
        }
    }

    /// Sessions live in hostd, so they survive Notchling restarting. If hostd itself stops (a reboot, an update), it
    /// can bring them back.
    @ViewBuilder private var hostSection: some View {
        if model.terminals.sessionsOutliveApp {
            VStack(alignment: .leading, spacing: 6) {
                Toggle(isOn: Binding(get: { resumeOnStart }, set: { resumeOnStart = $0; model.terminals.resumeOnStart = $0 })) {
                    Text("Reopen sessions when hostd restarts").kitType(.heading).foregroundColor(KitInk.text)
                }
                .toggleStyle(.switch)
                Text("Sessions a phone opens run in hostd, so they keep going when you quit or update Notchling. After a restart of the Mac, hostd runs \"claude --resume\" for them.")
                    .kitType(.meta).foregroundColor(KitInk.textMuted)
            }
        }
    }

    private var certificateSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("Certificate")
            Text("SHA-256 fingerprint").kitType(.meta).foregroundColor(KitInk.textMuted)
            Text(bridge.fingerprint ?? "").kitType(.mono).foregroundColor(KitInk.text).textSelection(.enabled)
            HStack(spacing: 8) {
                Button(copiedFingerprint ? "Copied" : "Copy Fingerprint") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(bridge.fingerprint ?? "", forType: .string)
                    copiedFingerprint = true
                }.buttonStyle(.kit(.secondary))
                Button("New Certificate…") { confirmRegenerate = true }.buttonStyle(.kit(.ghost))
            }
        }
    }

    private var activitySection: some View {
        VStack(alignment: .leading, spacing: 6) {
            sectionTitle("Remote activity")
            if bridge.log.isEmpty {
                Text("No commands from a phone yet.").kitType(.meta).foregroundColor(KitInk.textMuted)
            }
            ForEach(Array(bridge.log.prefix(8).enumerated()), id: \.offset) { _, entry in
                HStack(spacing: 8) {
                    Text(Self.format(entry.time)).kitType(.mono).foregroundColor(KitInk.textFaint)
                    Text(entry.op).kitType(.mono).foregroundColor(KitInk.text)
                    Text(entry.device).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                    if let session = entry.session { Text(String(session.prefix(8))).kitType(.mono).foregroundColor(KitInk.textFaint) }
                    Spacer(minLength: 0)
                }
            }
        }
    }

    // MARK: Helpers

    private func sectionTitle(_ text: String) -> some View {
        Text(text).kitType(.section).foregroundColor(KitInk.textFaint)
    }

    private func field(_ title: String, _ text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).kitType(.meta).foregroundColor(KitInk.textMuted)
            TextField("", text: text).modifier(KitField(focused: false)).accessibilityLabel(title)
        }
    }

    private func accessBinding(_ keyPath: WritableKeyPath<AccessConfig, String>) -> Binding<String> {
        Binding(get: { bridge.access[keyPath: keyPath] }, set: { value in bridge.updateAccess { $0[keyPath: keyPath] = value } })
    }

    private var tunnelStatusText: String {
        if let last = bridge.lastTunnelConnection { return "Last tunnel connection: \(Self.format(last))" }
        return "No tunnel connection since Notchling opened."
    }

    private var statusText: String {
        switch bridge.status {
        case .off: return "Off"
        case .starting: return "Starting…"
        case .running(let port): return "On, port \(port)"
        case .failed(let message): return "Error: \(message)"
        }
    }

    private var statusColor: Color {
        switch bridge.status {
        case .running: return KitInk.done
        case .failed: return KitInk.needs
        default: return KitInk.textFaint
        }
    }

    private static func format(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "dd/MM HH:mm:ss"
        return formatter.string(from: date)
    }
}
