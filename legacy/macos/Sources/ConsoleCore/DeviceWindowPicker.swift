import Foundation

public struct DeviceWindowInfo: Equatable, Sendable, Identifiable {
    public let id: UInt32
    public let appBundleId: String
    public let appName: String
    public let title: String
    public let isOnScreen: Bool
    public let isAppActive: Bool
    public let width: Double
    public let height: Double

    public init(id: UInt32, appBundleId: String, appName: String, title: String, isOnScreen: Bool,
                isAppActive: Bool, width: Double, height: Double) {
        self.id = id; self.appBundleId = appBundleId; self.appName = appName; self.title = title
        self.isOnScreen = isOnScreen; self.isAppActive = isAppActive; self.width = width; self.height = height
    }

    /// iOS devices in Xcode 27's DeviceHub (`com.apple.dt.Devices`) or the old Simulator, and Android emulators.
    public static func isDeviceWindow(bundleId: String, appName: String, width: Double, height: Double) -> Bool {
        guard width >= 150, height >= 300 else { return false }
        if bundleId == "com.apple.dt.Devices" || bundleId == "com.apple.iphonesimulator" { return true }
        let name = appName.lowercased()
        return name.contains("qemu-system") || name.contains("android emulator")
    }
}

public enum DeviceWindowPicker {
    /// The window whose title matches a device named in the transcript, else one that is on screen in the
    /// active app, else any on-screen one, else the first.
    public static func pick(from windows: [DeviceWindowInfo], hints: [String]) -> DeviceWindowInfo? {
        let lowered = hints.map { $0.lowercased() }
        if let match = windows.first(where: { window in
            let title = window.title.lowercased()
            return !title.isEmpty && lowered.contains { title.contains($0) || $0.contains(title) }
        }) { return match }
        return windows.first(where: { $0.isOnScreen && $0.isAppActive })
            ?? windows.first(where: \.isOnScreen)
            ?? windows.first
    }
}
