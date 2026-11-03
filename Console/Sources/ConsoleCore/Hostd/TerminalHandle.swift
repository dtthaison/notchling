import Foundation

/// A pty Console can show and type into: either one it runs itself (`PTYSession`) or one that lives in the
/// session host (`RemotePTY`). The rest of the app only knows this.
public protocol TerminalHandle: AnyObject, Identifiable, Sendable where ID == UUID {
    var id: UUID { get }
    var cwd: URL { get }
    var model: String? { get }
    var startedAt: Date { get }
    var rootPid: Int32 { get }
    var isRunning: Bool { get }
    func onExit(_ handler: @escaping () -> Void)
    /// The listener gets everything already on screen first (one chunk), then live output.
    @discardableResult func addListener(_ handler: @escaping (Data) -> Void) -> UUID
    func removeListener(_ token: UUID)
    func write(_ text: String)
    func write(_ data: Data)
    func resize(columns: UInt16, rows: UInt16)
    /// Asks the process to end (SIGTERM, then SIGKILL after a moment).
    func terminate()
    /// Blocking version for quitting an app that owns its sessions.
    func terminateAndWait(grace: TimeInterval)
}

extension TerminalHandle {
    public func terminateAndWait() { terminateAndWait(grace: 2) }
}
