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

/// Which URL the Web tab opens for a session: the newest localhost URL in the transcript, else a launch
/// config in `<cwd>/.claude/launch.json`, else one in `~/.claude/launch.json` that mentions the folder.
public enum WebURLResolver {
    public static func resolve(transcriptURLs: [String], cwd: String,
                               home: URL = FileManager.default.homeDirectoryForCurrentUser) -> URL? {
        if let latest = transcriptURLs.reversed().lazy.compactMap({ localURL($0) }).first { return latest }
        if !cwd.isEmpty {
            let projectFile = URL(fileURLWithPath: cwd).appendingPathComponent(".claude/launch.json")
            if let url = fromLaunchFile(projectFile, matchingCwd: nil) { return url }
            let homeFile = home.appendingPathComponent(".claude/launch.json")
            if let url = fromLaunchFile(homeFile, matchingCwd: cwd) { return url }
        }
        return nil
    }

    public static func localURL(_ string: String) -> URL? {
        guard let url = URL(string: string), let scheme = url.scheme, ["http", "https"].contains(scheme),
              let host = url.host, host == "localhost" || host == "127.0.0.1" else { return nil }
        return url
    }

    private static func fromLaunchFile(_ file: URL, matchingCwd cwd: String?) -> URL? {
        guard let launch = JSONFile.decode(LaunchFile.self, at: file) else { return nil }
        let candidates = (launch.configurations ?? []).filter { configuration in
            guard let cwd else { return true }
            return configuration.cwd == cwd || (configuration.runtimeArgs ?? []).contains(cwd)
        }
        // A dev server is the one you look at while coding; production previews come second.
        let ordered = candidates.sorted { ($0.name ?? "").contains("dev") && !($1.name ?? "").contains("dev") }
        for configuration in ordered {
            if let url = configuration.url.flatMap({ URL(string: $0) }), ["http", "https"].contains(url.scheme ?? "") { return url }
            if let port = configuration.port, (1...65_535).contains(port) { return URL(string: "http://localhost:\(port)") }
        }
        return nil
    }
}
