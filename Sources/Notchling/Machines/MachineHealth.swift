import Foundation

/// What the last poll of a machine returned: a status, or the error that stopped it.
struct LastReport: Equatable {
    var status: WorkerStatus?
    var at: Date?
    var error: String?

    init(status: WorkerStatus? = nil, at: Date? = nil, error: String? = nil) {
        self.status = status; self.at = at; self.error = error
    }

    /// The poll got a status back and reported no error.
    var succeeded: Bool { status != nil && error == nil }
}

/// The health words the Machines view shows. Pure: the poller supplies the reports.
enum MachineHealth {
    static let online = "online"
    static let offline = "offline"
    static let restarted = "restarted, waiting for unlock"
    static let paused = "production busy, paused"

    /// One word for a machine.
    /// - Parameters:
    ///   - fileVault: whether the machine is a FileVault Mac, which stops at the unlock screen after a reboot
    ///     (`Machine.fileVault`). A VPS never gets `restarted, waiting for unlock`.
    ///   - last: the poll just made.
    ///   - previous: the report before it that held a status (the poller keeps the last good one, so a run of
    ///     failed polls still sees what the machine was doing before it went quiet). nil if there was none.
    ///   - now: unused today; kept so a rule that depends on report age needs no signature change.
    static func word(fileVault: Bool, last: LastReport, previous: LastReport?, now: Date) -> String {
        if let status = last.status, last.error == nil {
            if fileVault, status.current != nil, let booted = status.bootedAt,
               let before = previous?.status?.bootedAt, booted > before {
                return restarted
            }
            return status.paused != nil ? paused : online
        }
        if fileVault, let before = previous?.status, before.current != nil || !before.ready.isEmpty || !before.needsYou.isEmpty {
            return restarted
        }
        return offline
    }

    /// "GitHub token on m1 expires in 14 days" once the token has 14 days or fewer; nil before that or when the
    /// worker does not know the expiry.
    static func tokenWarning(_ status: WorkerStatus) -> String? {
        guard let days = status.tokenExpiresInDays, days <= 14 else { return nil }
        let name = status.machine.isEmpty ? "this machine" : status.machine
        if days < 0 { return "GitHub token on \(name) has expired" }
        return "GitHub token on \(name) expires in \(days) \(days == 1 ? "day" : "days")"
    }
}
