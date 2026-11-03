import AppKit
import ConsoleCore
import CoreGraphics
import Observation
import ScreenCaptureKit

/// Streams a device window (DeviceHub, Simulator, Android Emulator) with ScreenCaptureKit at 30 fps into a
/// layer. It only reads the screen; it never boots, creates or launches a device.
@MainActor
@Observable
final class PhoneCapture: NSObject {
    enum State: Equatable {
        case idle
        case noPermission
        case scanning
        case noWindow
        case hidden(title: String)
        case streaming(title: String)
        case failed(String)
    }

    private(set) var state: State = .idle
    private(set) var windows: [DeviceWindowInfo] = []
    var selectedWindowId: UInt32?

    /// Frames land here; the host view sets them as its layer contents.
    @ObservationIgnored var onFrame: ((IOSurface) -> Void)?

    @ObservationIgnored private var hints: [String] = []
    @ObservationIgnored private var stream: SCStream?
    @ObservationIgnored private var streamingWindowId: UInt32?
    @ObservationIgnored private var scanTask: Task<Void, Never>?
    @ObservationIgnored private var wanted = false
    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    @ObservationIgnored private var frameSink: FrameSink?

    /// Opens the Screen Recording pane. Granting is the user's call; nothing is changed here.
    static func openScreenRecordingSettings() {
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") else { return }
        NSWorkspace.shared.open(url)
    }

    func start(hints: [String]) {
        self.hints = hints
        wanted = true
        if observers.isEmpty {
            // Quiet in the background: no stream while the app is not the active one.
            let center = NotificationCenter.default
            observers.append(center.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.pause() }
            })
            observers.append(center.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { if self?.wanted == true { self?.beginScanning() } }
            })
        }
        if NSApp.isActive || NSApp.windows.contains(where: { $0.isKeyWindow }) { beginScanning() }
    }

    func updateHints(_ hints: [String]) { self.hints = hints }

    func stop() {
        wanted = false
        pause()
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers = []
    }

    func select(_ id: UInt32?) {
        selectedWindowId = id
        Task { await stopStream() }
        beginScanning(immediately: true)
    }

    private func pause() {
        scanTask?.cancel()
        scanTask = nil
        Task { await stopStream() }
    }

    private func beginScanning(immediately: Bool = false) {
        scanTask?.cancel()
        if state == .idle { state = .scanning }
        scanTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.scanOnce()
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
    }

    private func scanOnce() async {
        guard CGPreflightScreenCaptureAccess() else {
            await stopStream()
            windows = []
            state = .noPermission
            return
        }
        let content: SCShareableContent
        do {
            content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        } catch {
            ConsoleLog.error("PhoneCapture.scanOnce", "SCShareableContent", "\(error)")
            state = .failed(error.localizedDescription)
            return
        }
        let candidates: [(SCWindow, DeviceWindowInfo)] = content.windows.compactMap { window in
            let bundleId = window.owningApplication?.bundleIdentifier ?? ""
            let appName = window.owningApplication?.applicationName ?? ""
            guard DeviceWindowInfo.isDeviceWindow(bundleId: bundleId, appName: appName,
                                                  width: window.frame.width, height: window.frame.height) else { return nil }
            return (window, DeviceWindowInfo(id: window.windowID, appBundleId: bundleId, appName: appName,
                                             title: window.title ?? "", isOnScreen: window.isOnScreen,
                                             isAppActive: window.owningApplication?.processID == NSWorkspace.shared.frontmostApplication?.processIdentifier,
                                             width: window.frame.width, height: window.frame.height))
        }
        windows = candidates.map(\.1)
        guard !candidates.isEmpty else {
            await stopStream()
            state = .noWindow
            return
        }
        let chosen = selectedWindowId.flatMap { id in candidates.first { $0.1.id == id }?.1 }
            ?? DeviceWindowPicker.pick(from: windows, hints: hints)
        guard let chosen, let scWindow = candidates.first(where: { $0.1.id == chosen.id })?.0 else {
            state = .noWindow
            return
        }
        let title = chosen.title.isEmpty ? chosen.appName : chosen.title
        guard chosen.isOnScreen else {
            await stopStream()
            state = .hidden(title: title)
            return
        }
        if streamingWindowId != chosen.id { await startStream(scWindow) }
        if stream != nil { state = .streaming(title: title) }
    }

    private func startStream(_ window: SCWindow) async {
        await stopStream()
        let configuration = SCStreamConfiguration()
        let scale = min(2.0, 1400 / max(window.frame.height, 1))
        configuration.width = Int(window.frame.width * scale)
        configuration.height = Int(window.frame.height * scale)
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        configuration.showsCursor = false
        configuration.queueDepth = 3
        let sink = FrameSink { [weak self] surface in
            DispatchQueue.main.async { MainActor.assumeIsolated { self?.onFrame?(surface) } }
        }
        let newStream = SCStream(filter: SCContentFilter(desktopIndependentWindow: window), configuration: configuration, delegate: nil)
        do {
            try newStream.addStreamOutput(sink, type: .screen, sampleHandlerQueue: DispatchQueue(label: "local.agentconsole.phone"))
            try await newStream.startCapture()
            stream = newStream
            frameSink = sink
            streamingWindowId = window.windowID
        } catch {
            ConsoleLog.error("PhoneCapture.startStream", "window \(window.windowID)", "\(error)")
            state = .failed(error.localizedDescription)
        }
    }

    private func stopStream() async {
        guard let current = stream else { return }
        stream = nil
        frameSink = nil
        streamingWindowId = nil
        do { try await current.stopCapture() } catch { ConsoleLog.error("PhoneCapture.stopStream", "", "\(error)") }
    }
}

private final class FrameSink: NSObject, SCStreamOutput, @unchecked Sendable {
    private let handler: (IOSurface) -> Void

    init(handler: @escaping (IOSurface) -> Void) { self.handler = handler }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sampleBuffer.isValid else { return }
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]]
        if let raw = attachments?.first?[.status] as? Int, SCFrameStatus(rawValue: raw) != .complete { return }
        guard let buffer = sampleBuffer.imageBuffer, let surface = CVPixelBufferGetIOSurface(buffer)?.takeUnretainedValue() else { return }
        handler(surface)
    }
}
