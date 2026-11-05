import Foundation

public enum VisualizeTab: String, CaseIterable, Sendable, Identifiable {
    case phone, web, flow, diagram, entity, chart

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .phone: return "Phone"
        case .web: return "Web"
        case .flow: return "Flow"
        case .diagram: return "Diagram"
        case .entity: return "Entity"
        case .chart: return "Chart"
        }
    }
}

/// What the transcript says, beyond the folder.
public struct VizSignals: Equatable, Sendable {
    public var hasRecentMermaid = false
    public var hasLocalhostURL = false

    public init(hasRecentMermaid: Bool = false, hasLocalhostURL: Bool = false) {
        self.hasRecentMermaid = hasRecentMermaid
        self.hasLocalhostURL = hasLocalhostURL
    }

    public static let none = VizSignals()
}

/// Picks the Visualize tab worth opening first from what the session's folder contains.
public struct VisualizeSuggester {
    private let fileManager: FileManager

    public init(fileManager: FileManager = .default) {
        self.fileManager = fileManager
    }

    public func suggest(cwd: String, signals: VizSignals = .none) -> VisualizeTab {
        if signals.hasRecentMermaid { return .diagram }
        guard !cwd.isEmpty else { return signals.hasLocalhostURL ? .web : .flow }
        let root = URL(fileURLWithPath: cwd)
        let entries = (try? fileManager.contentsOfDirectory(atPath: root.path)) ?? []

        if entries.contains("pubspec.yaml") || entries.contains("android")
            || entries.contains(where: { $0.hasSuffix(".xcodeproj") || $0.hasSuffix(".xcworkspace") }) {
            return .phone
        }
        if hasWebFramework(at: root.appendingPathComponent("package.json")) || signals.hasLocalhostURL { return .web }
        if entries.contains("prisma") || exists(root.appendingPathComponent("prisma/schema.prisma"))
            || entries.contains(where: { $0.hasSuffix(".sql") })
            || containsDriftTables(root) {
            return .entity
        }
        return .flow
    }

    private func exists(_ url: URL) -> Bool { fileManager.fileExists(atPath: url.path) }

    private func hasWebFramework(at packageJSON: URL) -> Bool {
        guard let data = try? Data(contentsOf: packageJSON),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return false }
        let dependencies = (object["dependencies"] as? [String: Any] ?? [:])
            .merging(object["devDependencies"] as? [String: Any] ?? [:]) { first, _ in first }
        return ["next", "vite", "react"].contains { dependencies[$0] != nil }
    }

    /// Drift keeps tables in files named *tables.dart or under a `tables` folder.
    private func containsDriftTables(_ root: URL) -> Bool {
        let lib = root.appendingPathComponent("lib")
        guard let enumerator = fileManager.enumerator(at: lib, includingPropertiesForKeys: nil,
                                                      options: [.skipsHiddenFiles]) else { return false }
        var visited = 0
        for case let url as URL in enumerator {
            visited += 1
            if visited > 400 { return false }
            let name = url.lastPathComponent
            if name.hasSuffix("tables.dart") || name == "tables" || name.hasSuffix(".drift") { return true }
        }
        return false
    }
}

/// The checked-out branch of `cwd`, read from .git/HEAD (no git process).
public enum GitBranch {
    public static func current(cwd: String, fileManager: FileManager = .default) -> String? {
        var directory = URL(fileURLWithPath: cwd)
        for _ in 0..<6 {
            let head = directory.appendingPathComponent(".git/HEAD")
            if let text = try? String(contentsOf: head, encoding: .utf8) {
                let line = text.trimmingCharacters(in: .whitespacesAndNewlines)
                let prefix = "ref: refs/heads/"
                return line.hasPrefix(prefix) ? String(line.dropFirst(prefix.count)) : String(line.prefix(7))
            }
            // A worktree or submodule has .git as a file pointing elsewhere; not worth following here.
            if fileManager.fileExists(atPath: directory.appendingPathComponent(".git").path) { return nil }
            let parent = directory.deletingLastPathComponent()
            if parent == directory { break }
            directory = parent
        }
        return nil
    }
}
