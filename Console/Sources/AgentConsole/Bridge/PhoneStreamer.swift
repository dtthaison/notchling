import AppKit
import ConsoleCore
import CoreImage
import ScreenCaptureKit

/// A device window as JPEG frames for one phone: ScreenCaptureKit, scaled to the requested width and rate,
/// handed to the connection's FrameGate, which drops frames while the last one is still being sent.
final class PhoneStreamer: @unchecked Sendable {
    private let lock = NSLock()
    private var streams: [UUID: (SCStream, Sink)] = [:]

    func start(connection: BridgeConnection, options: PhoneOptions) async throws {
        guard CGPreflightScreenCaptureAccess() else {
            throw BridgeFailure(.internal, "Agent Console chưa có quyền Ghi màn hình (System Settings › Quyền riêng tư & Bảo mật › Ghi màn hình).")
        }
        await stop(connection: connection)

        let content: SCShareableContent
        do {
            content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        } catch {
            throw BridgeFailure(.internal, "Không liệt kê được cửa sổ: \(error.localizedDescription)")
        }
        let candidates: [(SCWindow, DeviceWindowInfo)] = content.windows.compactMap { window in
            let bundleId = window.owningApplication?.bundleIdentifier ?? ""
            let appName = window.owningApplication?.applicationName ?? ""
            guard DeviceWindowInfo.isDeviceWindow(bundleId: bundleId, appName: appName, width: window.frame.width, height: window.frame.height) else { return nil }
            return (window, DeviceWindowInfo(id: window.windowID, appBundleId: bundleId, appName: appName, title: window.title ?? "",
                                             isOnScreen: window.isOnScreen, isAppActive: false,
                                             width: window.frame.width, height: window.frame.height))
        }
        let infos = candidates.map(\.1)
        let chosen = options.deviceWindowId.flatMap { id in infos.first { $0.id == id } } ?? DeviceWindowPicker.pick(from: infos, hints: [])
        guard let chosen, let window = candidates.first(where: { $0.1.id == chosen.id })?.0 else {
            throw BridgeFailure(.notFound, "Không thấy cửa sổ máy ảo (DeviceHub, Simulator hoặc Android Emulator).")
        }
        guard chosen.isOnScreen else {
            throw BridgeFailure(.notFound, "Cửa sổ máy ảo đang ẩn — mở DeviceHub để xem.")
        }

        let scale = Double(options.width) / max(window.frame.width, 1)
        let configuration = SCStreamConfiguration()
        configuration.width = max(2, Int(window.frame.width * scale))
        configuration.height = max(2, Int(window.frame.height * scale))
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(max(1, options.fps)))
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        configuration.showsCursor = false
        configuration.queueDepth = 3

        let sink = Sink(connection: connection)
        let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: window), configuration: configuration, delegate: nil)
        do {
            try stream.addStreamOutput(sink, type: .screen, sampleHandlerQueue: DispatchQueue(label: "local.agentconsole.bridge.phone"))
            try await stream.startCapture()
        } catch {
            throw BridgeFailure(.internal, "Không quay được cửa sổ: \(error.localizedDescription)")
        }
        lock.withLock { streams[connection.id] = (stream, sink) }
    }

    func stop(connection: BridgeConnection) async {
        let entry = lock.withLock { streams.removeValue(forKey: connection.id) }
        guard let (stream, _) = entry else { return }
        do { try await stream.stopCapture() } catch { ConsoleLog.error("PhoneStreamer.stop", "", "\(error)") }
    }

    private final class Sink: NSObject, SCStreamOutput, @unchecked Sendable {
        private let connection: BridgeConnection
        private let context = CIContext()

        init(connection: BridgeConnection) { self.connection = connection }

        func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
            guard type == .screen, sampleBuffer.isValid, let buffer = sampleBuffer.imageBuffer else { return }
            let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]]
            if let raw = attachments?.first?[.status] as? Int, SCFrameStatus(rawValue: raw) != .complete { return }
            // Encoding costs more than dropping: skip it when the last frame has not left yet.
            guard let jpeg = context.jpegRepresentation(of: CIImage(cvPixelBuffer: buffer),
                                                        colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
                                                        options: [kCGImageDestinationLossyCompressionQuality as CIImageRepresentationOption: 0.6]) else { return }
            connection.frameGate.offer(jpeg: jpeg, via: connection.io)
        }
    }
}
