import Foundation

private struct LaunchFile: Decodable {
    struct Configuration: Decodable {
        let name: String?
        let runtimeArgs: [String]?
        let port: Int?
        let url: String?
        let cwd: String?
    }
    let configurations: [Configuration]?
}

/// Which URL the Web tab opens for a session. Page URLs the transcript mentioned come first (newest first, one per
/// origin, API paths cut back to the origin, queries dropped because they carry tokens), and a server that is actually
/// listening beats one that is not. Then a launch config in `<cwd>/.claude/launch.json`, then one in
/// `~/.claude/launch.json` that mentions the folder. If nothing listens, the newest candidate still comes back so the
/// phone can say which server is down.
public enum WebURLResolver {
    public static func resolve(transcriptURLs: [String], cwd: String,
                               home: URL = FileManager.default.homeDirectoryForCurrentUser,
                               isListening: (Int) -> Bool = TCPProbe.isListening) -> URL? {
        let pages = pageCandidates(transcriptURLs)
        if let live = pages.first(where: { isLive($0, isListening) }) { return live }
        let launch = launchURLs(cwd: cwd, home: home)
        if let live = launch.first(where: { isLive($0, isListening) }) { return live }
        return pages.first ?? launch.first
    }

    /// Page URLs of servers that are listening right now, newest first, for the phone's suggestion chips.
    public static func liveCandidates(transcriptURLs: [String], cwd: String,
                                      home: URL = FileManager.default.homeDirectoryForCurrentUser,
                                      isListening: (Int) -> Bool = TCPProbe.isListening, limit: Int = 5) -> [URL] {
        var seen = Set<String>()
        var result: [URL] = []
        for url in pageCandidates(transcriptURLs) + launchURLs(cwd: cwd, home: home) where isLive(url, isListening) {
            guard seen.insert(origin(of: url)).inserted else { continue }
            result.append(url)
            if result.count == limit { break }
        }
        return result
    }

    /// Newest first, one per origin. A path under /api/ (or an asset path) says nothing about the page, so it becomes
    /// the origin; a query or fragment is dropped.
    static func pageCandidates(_ transcriptURLs: [String]) -> [URL] {
        var seen = Set<String>()
        var result: [URL] = []
        for text in transcriptURLs.reversed() {
            guard let url = localURL(text), var parts = URLComponents(url: url, resolvingAgainstBaseURL: false) else { continue }
            parts.query = nil
            parts.fragment = nil
            let path = parts.path
            if path.hasPrefix("/api") || path.contains("/api/") || path.hasPrefix("/_next") || path.hasPrefix("/static")
                || path.hasPrefix("/assets") || path.contains("/auth/") || path.hasPrefix("/auth") {
                parts.path = ""
            }
            guard let page = parts.url, seen.insert(origin(of: page)).inserted else { continue }
            result.append(page)
        }
        return result
    }

    static func origin(of url: URL) -> String { "\(url.scheme ?? "http")://\(url.host ?? ""):\(url.port ?? (url.scheme == "https" ? 443 : 80))" }

    private static func isLive(_ url: URL, _ isListening: (Int) -> Bool) -> Bool {
        isListening(url.port ?? (url.scheme == "https" ? 443 : 80))
    }

    private static func launchURLs(cwd: String, home: URL) -> [URL] {
        guard !cwd.isEmpty else { return [] }
        let projectFile = URL(fileURLWithPath: cwd).appendingPathComponent(".claude/launch.json")
        let homeFile = home.appendingPathComponent(".claude/launch.json")
        return fromLaunchFile(projectFile, matchingCwd: nil) + fromLaunchFile(homeFile, matchingCwd: cwd)
    }

    public static func localURL(_ string: String) -> URL? {
        guard let url = URL(string: string), let scheme = url.scheme, ["http", "https"].contains(scheme),
              let host = url.host, host == "localhost" || host == "127.0.0.1" else { return nil }
        return url
    }

    private static func fromLaunchFile(_ file: URL, matchingCwd cwd: String?) -> [URL] {
        guard let launch = JSONFile.decode(LaunchFile.self, at: file) else { return [] }
        let candidates = (launch.configurations ?? []).filter { configuration in
            guard let cwd else { return true }
            return configuration.cwd == cwd || (configuration.runtimeArgs ?? []).contains(cwd)
        }
        // A dev server is the one you look at while coding; production previews come second.
        let ordered = candidates.sorted { ($0.name ?? "").contains("dev") && !($1.name ?? "").contains("dev") }
        return ordered.compactMap { configuration in
            if let url = configuration.url.flatMap({ URL(string: $0) }), ["http", "https"].contains(url.scheme ?? "") { return url }
            if let port = configuration.port, (1...65_535).contains(port) { return URL(string: "http://localhost:\(port)") }
            return nil
        }
    }
}

/// A cheap check that something accepts connections on a local port.
public enum TCPProbe {
    public static func isListening(_ port: Int) -> Bool {
        guard (1...65_535).contains(port) else { return false }
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { return false }
        defer { close(descriptor) }
        _ = fcntl(descriptor, F_SETFL, fcntl(descriptor, F_GETFL) | O_NONBLOCK)
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = in_port_t(port).bigEndian
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        if result == 0 { return true }
        guard errno == EINPROGRESS else { return false }
        var pollDescriptor = pollfd(fd: descriptor, events: Int16(POLLOUT), revents: 0)
        guard poll(&pollDescriptor, 1, 200) > 0 else { return false }
        var failure: Int32 = 0
        var length = socklen_t(MemoryLayout<Int32>.size)
        getsockopt(descriptor, SOL_SOCKET, SO_ERROR, &failure, &length)
        return failure == 0
    }
}
