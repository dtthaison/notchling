import Foundation

/// The models Codex offers, from the list it caches (`~/.codex/models_cache.json`, refreshed by Codex
/// itself): the ones its own picker lists (`visibility: "list"`), in its order, with each one's
/// reasoning efforts. Nothing here is hard-coded, so new models show up when Codex learns of them.
struct CodexModel: Equatable {
    let slug: String
    let name: String
    let summary: String
    let efforts: [String]
    let defaultEffort: String?
}

enum CodexModels {
    static var cacheURL: URL { CodexStore.home.appendingPathComponent("models_cache.json") }

    private static let lock = NSLock()
    private static var cached: (modified: Date, models: [CodexModel])?

    /// Re-parsed only when Codex rewrites its cache: the composer asks on every keystroke.
    static func load() -> [CodexModel] {
        let modified = (try? FileManager.default.attributesOfItem(atPath: cacheURL.path))?[.modificationDate] as? Date ?? .distantPast
        lock.lock(); defer { lock.unlock() }
        if let cached, cached.modified == modified { return cached.models }
        let models = (try? Data(contentsOf: cacheURL)).map(parse) ?? []
        cached = (modified, models)
        return models
    }

    static func parse(_ data: Data) -> [CodexModel] {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let models = root["models"] as? [[String: Any]] else { return [] }
        return models
            .filter { ($0["visibility"] as? String ?? "list") == "list" }
            .sorted { ($0["priority"] as? Int ?? .max) < ($1["priority"] as? Int ?? .max) }
            .compactMap { m in
                guard let slug = m["slug"] as? String else { return nil }
                let efforts = (m["supported_reasoning_levels"] as? [[String: Any]] ?? []).compactMap { $0["effort"] as? String }
                return CodexModel(slug: slug, name: m["display_name"] as? String ?? slug,
                                  summary: m["description"] as? String ?? "",
                                  efforts: efforts, defaultEffort: m["default_reasoning_level"] as? String)
            }
    }
}

/// The model and effort picked for a Codex session's next turns; nil keeps Codex's default.
struct CodexPick: Equatable, Codable {
    var model: String?
    var effort: String?
}
