import Foundation

public enum SendError: Error, LocalizedError, Equatable {
    case viewOnly
    case notOwned
    case failed(String)

    public var errorDescription: String? {
        switch self {
        case .viewOnly: return "Session mở ngoài Console — chỉ xem."
        case .notOwned: return "Console không còn giữ terminal của session này."
        case .failed(let why): return why
        }
    }
}

public protocol MessageSender: Sendable {
    func send(_ text: String, to session: Session) async throws
}

/// Picks the channel by `Session.control`. A view-only session never reaches a sender.
public struct InputRouter: Sendable {
    private let job: MessageSender
    private let pty: MessageSender

    public init(job: MessageSender, pty: MessageSender) {
        self.job = job
        self.pty = pty
    }

    public func send(_ text: String, to session: Session) async throws {
        switch session.control {
        case .job: try await job.send(text, to: session)
        case .consolePTY: try await pty.send(text, to: session)
        case .viewOnly: throw SendError.viewOnly
        }
    }
}

/// What a terminal expects for a pasted reply: bracketed paste keeps newlines from submitting early,
/// then Enter submits.
public enum TerminalKeys {
    public static func replyChunks(_ text: String) -> [String] {
        [bracketed(text), "\r"]
    }

    /// `text` as one bracketed paste. ESC (and its one-byte C1 form, CSI) is dropped from the text first: a
    /// pasted `ESC[201~` would end the paste early and the rest would be typed as keys (Enter submits, digits
    /// answer a dialog, Esc interrupts).
    public static func bracketed(_ text: String) -> String {
        let flat = text.replacingOccurrences(of: "\r\n", with: "\n")
        let safe = String(String.UnicodeScalarView(flat.unicodeScalars.filter { $0 != "\u{1b}" && $0 != "\u{9b}" }))
        return "\u{1b}[200~" + safe + "\u{1b}[201~"
    }
}

/// Sends into a Console-owned pty.
public struct PTYMessageSender: MessageSender {
    private let owned: OwnedSessions
    private let pause: TimeInterval

    public init(owned: OwnedSessions, pause: TimeInterval = 0.25) {
        self.owned = owned
        self.pause = pause
    }

    public func send(_ text: String, to session: Session) async throws {
        guard let pid = session.pid, let pty = owned.pty(owning: pid) else { throw SendError.notOwned }
        for chunk in TerminalKeys.replyChunks(text) {
            pty.write(chunk)
            try await Task.sleep(nanoseconds: UInt64(pause * 1_000_000_000))
        }
    }
}
