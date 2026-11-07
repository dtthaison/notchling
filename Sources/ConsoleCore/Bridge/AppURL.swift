import Foundation

/// `agentconsole://open[?phones=1]`: how Notchling (or anything else) asks Console to come to the front.
public enum AppURL {
    public enum Action: Equatable, Sendable {
        /// Bring the window up; with `phones`, also tell every connected phone to open.
        case open(phones: Bool)
    }

    public static let scheme = "agentconsole"

    /// Nil for anything that is not a known action. Unknown parameters are ignored.
    public static func parse(_ url: URL) -> Action? {
        guard url.scheme?.lowercased() == scheme, url.host?.lowercased() == "open" else { return nil }
        let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []
        let phones = items.first { $0.name == "phones" }?.value == "1"
        let unknown = items.map(\.name).filter { $0 != "phones" }
        if !unknown.isEmpty {
            ConsoleLog.logger.notice("AppURL: ignoring unknown parameters \(unknown.joined(separator: ","), privacy: .public)")
        }
        return .open(phones: phones)
    }
}
