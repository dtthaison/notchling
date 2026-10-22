import Foundation
import IOKit.pwr_mgt

enum AwakeMode: String, CaseIterable {
    case off, on, whileWorking

    var title: String {
        switch self {
        case .off: return "Off"
        case .on: return "Always On"
        case .whileWorking: return "While Agents Are Working"
        }
    }
}

/// Holds an IOKit power assertion that keeps the Mac from idle-sleeping.
final class AwakeController {
    private var assertionID: IOPMAssertionID = 0
    private(set) var isActive = false

    var mode: AwakeMode {
        didSet { UserDefaults.standard.set(mode.rawValue, forKey: "mode") }
    }

    /// When true the display stays on too; otherwise only the system stays awake.
    var keepDisplayOn: Bool {
        didSet {
            UserDefaults.standard.set(keepDisplayOn, forKey: "keepDisplayOn")
            if isActive { stop(); start() }
        }
    }

    init() {
        let defaults = UserDefaults.standard
        keepDisplayOn = defaults.bool(forKey: "keepDisplayOn")
        if let raw = defaults.string(forKey: "mode"), let saved = AwakeMode(rawValue: raw) {
            mode = saved
        } else {
            // Version 1 stored a plain on/off flag.
            mode = defaults.bool(forKey: "active") ? .on : .off
        }
    }

    /// Starts or stops the assertion to match the mode and whether any agent is working.
    func apply(agentsWorking: Bool) {
        let want = mode == .on || (mode == .whileWorking && agentsWorking)
        if want { start() } else { stop() }
    }

    @discardableResult
    func start() -> Bool {
        guard !isActive else { return true }
        let type = keepDisplayOn
            ? kIOPMAssertionTypePreventUserIdleDisplaySleep
            : kIOPMAssertionTypePreventUserIdleSystemSleep
        let result = IOPMAssertionCreateWithName(
            type as CFString,
            IOPMAssertionLevel(kIOPMAssertionLevelOn),
            "Notchling: keeping the Mac awake for running agents" as CFString,
            &assertionID
        )
        isActive = result == kIOReturnSuccess
        return isActive
    }

    func stop() {
        guard isActive else { return }
        IOPMAssertionRelease(assertionID)
        isActive = false
    }
}
