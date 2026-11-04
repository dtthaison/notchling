import Foundation

/// Recent distinct project folders: the cwd of known sessions first, then folders recovered from the
/// names of ~/.claude/projects/*, newest first.
public enum RecentProjects {
    public static func list(sessions: [Session], projectsDir: URL, limit: Int = 30,
                            directoryExists: (String) -> Bool = { path in
                                var isDirectory: ObjCBool = false
                                return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && isDirectory.boolValue
                            }) -> [String] {
        var seen = Set<String>()
        var result: [String] = []
        func add(_ path: String) {
            guard !path.isEmpty, path != "/", !seen.contains(path), directoryExists(path) else { return }
            seen.insert(path)
            result.append(path)
        }
        for session in sessions.sorted(by: { $0.lastActivity > $1.lastActivity }) { add(session.cwd) }

        let entries = JSONFile.contents(of: projectsDir).compactMap { url -> (String, Date)? in
            guard let modified = modificationDate(of: url) else { return nil }
            return (url.lastPathComponent, modified)
        }.sorted { $0.1 > $1.1 }
        for (name, _) in entries {
            if result.count >= limit { break }
            if let path = decode(directoryName: name, directoryExists: directoryExists) { add(path) }
        }
        return Array(result.prefix(limit))
    }

    /// "-Users-me-my-app" is "/Users/me/my-app" or "/Users/me/my/app": try joining dash-separated
    /// parts into one folder name wherever that folder exists.
    public static func decode(directoryName: String, directoryExists: (String) -> Bool) -> String? {
        guard directoryName.hasPrefix("-") else { return nil }
        let parts = directoryName.dropFirst().split(separator: "-", omittingEmptySubsequences: false).map(String.init)
        func search(_ index: Int, _ base: String) -> String? {
            if index == parts.count { return base }
            var name = ""
            for end in index..<parts.count {
                name += (end == index ? "" : "-") + parts[end]
                let candidate = base + "/" + name
                if directoryExists(candidate), let found = search(end + 1, candidate) { return found }
            }
            return nil
        }
        return search(0, "")
    }
}
