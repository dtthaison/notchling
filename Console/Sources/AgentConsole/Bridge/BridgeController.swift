import AppKit
import ConsoleCore
import CoreImage
import CoreImage.CIFilterBuiltins
import Observation
import UserNotifications

/// A phone asking to pair, waiting for the user's answer on the Mac.
struct PairingApproval: Identifiable {
    let id = UUID()
    let deviceName: String
    let platform: String
    fileprivate let resume: (Bool) -> Void
}

struct PairingInfo {
    let code: String
    let expiresAt: Date
    let payload: String
    let hosts: [String]
    let qr: NSImage
}

/// Owns the bridge: the on/off setting, the TLS listener, pairing, paired devices and the audit view.
@MainActor
@Observable
final class BridgeController {
    enum Status: Equatable {
        case off
        case starting
        case running(port: UInt16)
        case failed(String)
    }

    private(set) var status: Status = .off
    private(set) var fingerprint: String?
    private(set) var devices: [PairedDevice] = []
    private(set) var log: [AuditEntry] = []
    private(set) var pairing: PairingInfo?
    /// "Truy cập qua internet" settings, mirrored from bridge/access.json.
    private(set) var access = AccessConfig()
    private(set) var lastTunnelConnection: Date?
    var approval: PairingApproval?
    var showingSettings = false

    @ObservationIgnored private let defaults = UserDefaults.standard
    @ObservationIgnored private var core: BridgeCore?
    @ObservationIgnored private var server: BridgeServer?
    @ObservationIgnored private var tunnelOrigin: TunnelOriginServer?
    @ObservationIgnored private var directory = BridgeIdentity.defaultDirectory

    /// The setting, kept here (not read from UserDefaults each time) so the switch follows it.
    private(set) var isEnabled = UserDefaults.standard.bool(forKey: "bridgeEnabled")
    var isRunning: Bool { if case .running = status { return true } else { return false } }
    var port: UInt16? { if case .running(let port) = status { return port } else { return nil } }

    func attach(model: ConsoleModel) {
        guard core == nil else { return }
        let backend = LiveBridgeBackend(model: model, controller: self)
        core = BridgeCore(directory: directory, backend: backend, fetchAccessKeys: Self.accessKeyFetch)
        self.backend = backend
        access = core?.access.config ?? access
        refresh()
        if isEnabled { start() }
    }

    @ObservationIgnored private var backend: LiveBridgeBackend?

    /// Cloudflare's keys come from the team's certs URL. Debug builds can point `-debugAccessCertsFile` at a
    /// local JWKS file instead, to exercise the tunnel path without Cloudflare.
    private static var accessKeyFetch: AccessKeySet.Fetch {
        #if DEBUG
        if let path = UserDefaults.standard.string(forKey: "debugAccessCertsFile") {
            return { _ in try Data(contentsOf: URL(fileURLWithPath: path)) }
        }
        #endif
        return AccessKeySet.liveFetch
    }

    func setEnabled(_ on: Bool) {
        isEnabled = on
        defaults.set(on, forKey: "bridgeEnabled")
        if on { start() } else { stop() }
    }

    func start() {
        guard let core, let backend, server == nil else { return }
        status = .starting
        let directory = self.directory
        let port = BridgeServer.defaultPort
        Task {
            do {
                let identity = try await Task.detached { try BridgeIdentity.loadOrCreate(in: directory) }.value
                let server = BridgeServer(core: core, backend: backend)
                let name = Host.current().localizedName ?? ProcessInfo.processInfo.hostName
                do {
                    try await server.start(identity: identity, port: port, serviceName: name)
                } catch {
                    throw PortError.busy(port, error)
                }
                let origin = TunnelOriginServer(core: core, backend: backend)
                do {
                    try await origin.start()
                } catch {
                    server.stop()
                    throw PortError.busy(TunnelOriginServer.port, error)
                }
                self.server = server
                self.tunnelOrigin = origin
                fingerprint = identity.fingerprint
                status = .running(port: port)
                UNUserNotificationCenter.current().requestAuthorization(options: [.alert]) { _, error in
                    if let error { ConsoleLog.error("BridgeController.start", "notifications", "\(error)") }
                }
                refresh()
            } catch {
                ConsoleLog.error("BridgeController.start", directory.path, "\(error)")
                status = .failed(error.localizedDescription)
                self.server = nil
            }
        }
    }

    enum PortError: LocalizedError {
        case busy(UInt16, Error)
        var errorDescription: String? {
            if case .busy(let port, _) = self { return "Cổng \(port) đang bị app khác dùng" }
            return nil
        }
    }

    func stop() {
        server?.stop()
        server = nil
        tunnelOrigin?.stop()
        tunnelOrigin = nil
        core?.codes.cancel()
        pairing = nil
        status = .off
        if let approval { self.approval = nil; approval.resume(false) }
    }

    /// `agentconsole://open?phones=1` with the bridge on: tell connected phones to open. Off means nothing happens.
    func notifyPhonesToOpen() {
        guard isRunning, let core else {
            ConsoleLog.logger.notice("open?phones=1 ignored: the bridge is off")
            return
        }
        core.handleOpen(phones: true)
        refresh()
    }

    func refresh() {
        guard let core else { return }
        devices = core.devices.devices
        log = core.audit.recent(limit: 50)
        access = core.access.config
        lastTunnelConnection = core.lastTunnelConnection
    }

    func updateAccess(_ change: (inout AccessConfig) -> Void) {
        guard let core else { return }
        core.access.update(change)
        access = core.access.config
    }

    func revoke(_ device: PairedDevice) {
        guard let core else { return }
        do { try core.revoke(deviceId: device.id) } catch { ConsoleLog.error("BridgeController.revoke", device.id, "\(error)") }
        refresh()
    }

    /// New certificate: every paired phone stops matching, so they are all removed too.
    func regenerateCertificate() {
        let wasRunning = isRunning
        stop()
        BridgeIdentity.erase(in: directory)
        if let core { for device in core.devices.devices { try? core.revoke(deviceId: device.id) } }
        fingerprint = nil
        refresh()
        if wasRunning || isEnabled { start() }
    }

    /// A takeover started from the Mac is audited here; one from a phone is audited by the router, and the
    /// Mac gets a notification so a resumed session never appears unannounced.
    func recordTakeover(sessionName: String, from origin: String, pty: any TerminalHandle, stoppedPid: Int32?) {
        if origin == "Mac" {
            core?.audit.append(device: "mac", op: "takeover", stoppedPid: stoppedPid.map(Int.init))
            refresh()
            return
        }
        let content = UNMutableNotificationContent()
        content.title = "Điện thoại đã tiếp quản session"
        content.body = stoppedPid == nil ? sessionName : "\(sessionName) (đã dừng bản trên Claude desktop/VS Code)"
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)) { error in
            if let error { ConsoleLog.error("BridgeController.recordTakeover", sessionName, "\(error)") }
        }
    }

    // MARK: - Pairing

    func startPairing() {
        guard let core, let port, let fingerprint else { return }
        let issued = core.codes.issue()
        let addresses = LocalAddresses.current()
        let name = Host.current().localizedName ?? ProcessInfo.processInfo.hostName
        var hosts = addresses.lan
        if let tailscale = addresses.tailscale { hosts.append(tailscale) }
        let bonjour = ProcessInfo.processInfo.hostName.hasSuffix(".local") ? ProcessInfo.processInfo.hostName : ProcessInfo.processInfo.hostName + ".local"
        hosts.append(bonjour)
        guard let payload = PairingPayload.make(name: name, port: Int(port), fingerprint: fingerprint, hosts: hosts,
                                                code: issued.code, access: core.access.config) else { return }
        pairing = PairingInfo(code: issued.code, expiresAt: issued.expiresAt, payload: payload, hosts: hosts, qr: Self.qrImage(payload))
    }

    func cancelPairing() {
        core?.codes.cancel()
        pairing = nil
    }

    static func qrImage(_ text: String) -> NSImage {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return NSImage() }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 8, y: 8))
        let representation = NSCIImageRep(ciImage: scaled)
        let image = NSImage(size: representation.size)
        image.addRepresentation(representation)
        return image
    }

    // MARK: - Approval

    /// Called from the bridge when a phone pairs with a valid code: the user decides on the Mac.
    func requestApproval(deviceName: String, platform: String) async -> Bool {
        guard approval == nil else { return false }
        NSApp.activate(ignoringOtherApps: true)
        return await withCheckedContinuation { continuation in
            approval = PairingApproval(deviceName: deviceName, platform: platform) { continuation.resume(returning: $0) }
        }
    }

    func resolveApproval(_ allow: Bool) {
        guard let current = approval else { return }
        approval = nil
        current.resume(allow)
        if allow { cancelPairing() }
        refresh()
    }
}
