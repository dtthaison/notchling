import AppKit
import ConsoleCore

/// Owns the phone side of Notchling: the session list the phone sees, the session host connection and the bridge
/// (TLS :47821 and the tunnel origin :47822). It starts once, at launch, and lives as long as the app.
@MainActor
final class PhoneService {
    static let shared = PhoneService()

    let model = PhoneModel()
    var bridge: BridgeController { model.bridge }
    private var started = false

    /// The account's limits for the phone's chat header ("5h", "Week"). Set by the app; empty until it is.
    var limitsSource: () -> [LimitInfo] = { [] }

    private init() {}

    func start() {
        guard !started else { return }
        started = true
        // Chat types into a terminal the host owns through this hook (see ReplySender).
        let owned = model.owned
        HostTerminals.owns = { pid in owned.isOwned(pid: pid) }
        HostTerminals.send = { text, pid in try PhoneModel.type(text, intoTerminalOwning: pid, in: owned) }
        HostTerminals.keys = { chunks, pid in try PhoneModel.keys(chunks, intoTerminalOwning: pid, in: owned) }
        model.start()
        bridge.attach(model: model)
    }

    /// `agentconsole://open?phones=1` and the menu's "Open Chat and Phones": paired phones come to the front too.
    func notifyPhonesToOpen() {
        bridge.notifyPhonesToOpen()
    }

    /// Sessions in the host keep running when Notchling quits; one that is a child of this app would end with it.
    func announceQuit() {
        let running = model.owned.running
        guard !running.isEmpty, model.terminals.sessionsOutliveApp else { return }
        bridge.stop()
    }
}
