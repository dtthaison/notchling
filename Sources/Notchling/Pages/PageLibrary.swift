import Foundation

/// A local copy of every page Claude Code publishes as an Artifact. Artifacts are private to the login
/// that published them, and the source file often sits in a job's tmp folder that goes with the job,
/// so each publish is also kept here, readable whatever account is active:
///   pages/<slug>/index.html (or .md) + supporting files
///   pages/pages.json    the list, one entry per artifact url (source path when there is none)
///   pages/index.html    a plain list of everything, regenerated after each save
/// The copy runs in the `Notchling --hook` process (PostToolUse for Artifact), so it never needs the app.
enum PageLibrary {
    static let dir = HookBridge.dir.appendingPathComponent("pages")
    static let listURL = dir.appendingPathComponent("pages.json")
    static let indexURL = dir.appendingPathComponent("index.html")

    /// Biggest page or supporting file copied; anything larger is skipped.
    private static let maxFileBytes = 16 * 1024 * 1024
    private static let maxTotalBytes = 64 * 1024 * 1024

    struct Entry: Equatable {
        var folder: String
        var file: String
        var title: String
        var url: String
        var source: String
        var project: String
        var account: String
        var updated: Double
        /// claude.ai's id of the version last published from this Mac ("" when unknown), so a publish
        /// made for its owner can tell when the page changed elsewhere since.
        var version = ""

        init(folder: String, file: String, title: String, url: String, source: String,
             project: String, account: String, updated: Double, version: String = "") {
            self.folder = folder; self.file = file; self.title = title; self.url = url
            self.source = source; self.project = project; self.account = account; self.updated = updated
            self.version = version
        }

        init?(_ obj: [String: Any]) {
            guard let folder = obj["folder"] as? String, let file = obj["file"] as? String else { return nil }
            self.init(folder: folder, file: file, title: obj["title"] as? String ?? folder,
                      url: obj["url"] as? String ?? "", source: obj["source"] as? String ?? "",
                      project: obj["project"] as? String ?? "", account: obj["account"] as? String ?? "",
                      updated: (obj["updated"] as? NSNumber)?.doubleValue ?? 0, version: obj["version"] as? String ?? "")
        }

        var json: [String: Any] {
            ["folder": folder, "file": file, "title": title, "url": url, "source": source,
             "project": project, "account": account, "updated": updated, "version": version]
        }

        var localURL: URL { PageLibrary.dir.appendingPathComponent(folder).appendingPathComponent(file) }

        /// The last part of the claude.ai link; nil for a page that never got one.
        var artifactID: String? { url.isEmpty ? nil : url.components(separatedBy: "/").last }

        /// Every word (lowercased) is in the title, project, account or link. The link holds the id,
        /// so a pasted id or link matches too.
        func matches(_ words: [Substring]) -> Bool {
            let text = "\(title) \(project) \(account) \(url)".lowercased()
            return words.allSatisfy { text.contains($0) }
        }
    }

    // MARK: - Reading (app side)

    /// When the list was last written, for callers that poll and only need to read it after a change.
    static var listModified: Date? {
        (try? listURL.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
    }

    /// Every saved page, newest first.
    static func entries() -> [Entry] {
        let list = HookBridge.readJSON(listURL)?["pages"] as? [[String: Any]] ?? []
        return list.compactMap(Entry.init).sorted { $0.updated > $1.updated }
    }

    // MARK: - Finding (for agents: `Notchling --page`, and failed Artifact reads)

    /// The artifact id in a claude.ai link, or the text itself when it looks like a bare id.
    static func artifactID(in text: String) -> String? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if let r = t.range(of: #"claude\.ai/(?:code/)?artifact/[A-Za-z0-9_-]+"#, options: .regularExpression) {
            return t[r].components(separatedBy: "/").last
        }
        return t.range(of: #"^[A-Za-z0-9_-]{16,}$"#, options: .regularExpression) != nil ? t : nil
    }

    /// The page with this link or id, else the pages whose text holds every word of the query.
    static func find(_ query: String) -> [Entry] { find(query, in: entries()) }

    /// `find` over a list already read, for the search panel, which searches on every key press.
    static func find(_ query: String, in all: [Entry]) -> [Entry] {
        if let id = artifactID(in: query) {
            let exact = all.filter { $0.artifactID == id }
            if !exact.isEmpty { return exact }
            let loose = all.filter { $0.artifactID?.lowercased() == id.lowercased() }
            if !loose.isEmpty { return loose }
        }
        let words = query.lowercased().split(separator: " ")
        return words.isEmpty ? all : all.filter { $0.matches(words) }
    }

    /// Plain lines an agent can act on: where the copy is, and who can republish it.
    static func describe(_ e: Entry) -> String {
        let fm = FileManager.default
        let folder = dir.appendingPathComponent(e.folder)
        let others = ((try? fm.subpathsOfDirectory(atPath: folder.path)) ?? [])
            .filter { $0 != e.file && !$0.hasPrefix(".") }
            .filter { var d: ObjCBool = false; return fm.fileExists(atPath: folder.appendingPathComponent($0).path, isDirectory: &d) && !d.boolValue }
            .sorted()
        let date = ISO8601DateFormatter.string(from: Date(timeIntervalSince1970: e.updated), timeZone: .current,
                                               formatOptions: [.withFullDate, .withTime, .withColonSeparatorInTime, .withDashSeparatorInDate, .withSpaceBetweenDateAndTime])
        var lines = ["Title: \(e.title)",
                     "Link: \(e.url.isEmpty ? "(never published to claude.ai)" : e.url)",
                     "Published by: \(e.account.isEmpty ? "unknown account" : e.account)"]
        if !e.project.isEmpty { lines.append("Project: \(e.project)") }
        lines.append("Last published: \(date)")
        lines.append("Local copy: \(e.localURL.path)")
        if !others.isEmpty { lines.append("Supporting files (next to it): \(others.joined(separator: ", "))") }
        if !e.source.isEmpty, fm.fileExists(atPath: e.source) { lines.append("Original source file: \(e.source)") }
        return lines.joined(separator: "\n")
    }

    /// `Notchling --page <link | id | words>` and `Notchling --pages`: print what's saved, then exit.
    static func runCLI(_ query: String, list: Bool) -> Never {
        let found = find(query)
        if found.isEmpty {
            FileHandle.standardError.write(Data("No saved page matches \"\(query)\".\n".utf8))
            exit(1)
        }
        if list {
            for e in found {
                print([e.artifactID ?? "-", e.title, e.account.isEmpty ? "-" : e.account].joined(separator: "\t"))
            }
        } else {
            let shown = found.prefix(10)
            print(shown.map(describe).joined(separator: "\n\n"))
            if found.count > shown.count { print("\n…and \(found.count - shown.count) more. Narrow the search, or use --pages.") }
        }
        exit(0)
    }

    /// The saved page an `Artifact read` of a whole page asks for. Reads of one file (`path`/`paths`)
    /// aren't matched, so `path: "index.html"` still fetches the live page.
    private static func savedPage(for event: [String: Any]) -> Entry? {
        let input = event["tool_input"] as? [String: Any] ?? [:]
        guard input["action"] as? String == "read", input["path"] == nil, input["paths"] == nil,
              let link = input["url"] as? String, let id = artifactID(in: link) else { return nil }
        return entries().first(where: { $0.artifactID == id })
    }

    /// The deny reason for a `PreToolUse(Artifact)` read we can answer locally: where the copy is. Nil
    /// (the real read runs) when we don't keep the page or its file is gone.
    static func localRead(_ event: [String: Any]) -> String? {
        guard let page = savedPage(for: event),
              FileManager.default.fileExists(atPath: page.localURL.path) else { return nil }
        return "Notchling answered this read from its local copy of the artifact, as last published from this Mac. "
            + "Read the \"Local copy\" file with the Read tool; this is not an error.\n"
            + describe(page)
            + "\nIt doesn't hold edits made on claude.ai after that publish. To get those, read the live page with "
            + "action \"read\", this url and path \"index.html\" (only the account that published it can)."
    }

    /// Context for a failed `Artifact read`: the link's page is usually another account's (artifacts are
    /// private to their login), and the local copy still reads.
    static func readFallback(_ event: [String: Any]) -> String? {
        guard let page = savedPage(for: event) else { return nil }
        return "Notchling keeps a local copy of this artifact, as last published from this Mac. Read the local file instead:\n"
            + describe(page)
            + "\nOnly the account that published it can republish to this link; from another account, publishing makes a new link."
    }

    // MARK: - Hook side

    /// Saves the page behind a PostToolUse event for the Artifact tool. Silent and best effort: the caller
    /// exits 0 whatever happens here.
    static func handle(_ event: [String: Any]) {
        let input = event["tool_input"] as? [String: Any] ?? [:]
        let action = input["action"] as? String
        guard action == nil || action == "publish",
              input["asset"] as? Bool != true,
              let path = input["file_path"] as? String else { return }
        let ext = (path as NSString).pathExtension.lowercased()
        guard ext == "html" || ext == "md" else { return }

        let response = flatten(event["tool_response"])
        let url = artifactURL(in: response) ?? (input["url"] as? String).flatMap(artifactURL(in:)) ?? ""
        // A refused or failed publish has no page behind it.
        if url.isEmpty, response.range(of: "refused|error|failed|conflict", options: [.regularExpression, .caseInsensitive]) != nil { return }

        let cwd = event["cwd"] as? String ?? ""
        // Only an import of older publishes sets this: their real time, and no account (it isn't known).
        let backfill = event["notchling_backfill"] as? [String: Any]
        let base = cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser : URL(fileURLWithPath: cwd)
        let source = URL(fileURLWithPath: path, relativeTo: base).standardizedFileURL
        guard let data = readFile(source) else { return }

        let fm = FileManager.default
        withLock {
            var list = (HookBridge.readJSON(listURL)?["pages"] as? [[String: Any]] ?? []).compactMap(Entry.init)
            let key = url.isEmpty ? source.path : url
            let existing = list.firstIndex { url.isEmpty ? $0.source == source.path : $0.url == url }
            let folder = existing.map { list[$0].folder } ?? makeFolder(title: pageTitle(data, ext: ext, fallback: source.lastPathComponent), key: key)
            let file = "index." + ext
            let folderURL = dir.appendingPathComponent(folder)
            do { try fm.createDirectory(at: folderURL, withIntermediateDirectories: true) } catch { return }

            guard write(ext == "html" ? wrapped(data) : data, to: folderURL.appendingPathComponent(file)) else { return }
            // A republish may change the kind of page; drop the other index file.
            try? fm.removeItem(at: folderURL.appendingPathComponent(ext == "html" ? "index.md" : "index.html"))
            copySupporting(input, cwd: base, into: folderURL, skipping: file)

            let entry = Entry(folder: folder, file: file,
                              title: pageTitle(data, ext: ext, fallback: source.lastPathComponent),
                              url: url, source: source.path,
                              project: projectName(cwd),
                              // The first publisher owns the page; only they can republish it.
                              account: backfill?["account"] as? String ?? existing.map { list[$0].account }.flatMap { $0.isEmpty ? nil : $0 } ?? activeEmail(),
                              updated: (backfill?["updated"] as? NSNumber)?.doubleValue ?? Date().timeIntervalSince1970,
                              version: publishedVersion(in: response))
            if let i = existing { list[i] = entry } else { list.append(entry) }
            HookBridge.writeJSON(["pages": list.map(\.json)], to: listURL)
            writeIndex(list)
        }
    }

    /// Forgets a page and deletes its local folder. Never touches the page on claude.ai.
    static func remove(_ page: Entry) {
        withLock {
            let list = (HookBridge.readJSON(listURL)?["pages"] as? [[String: Any]] ?? []).compactMap(Entry.init)
                .filter { $0.folder != page.folder }
            try? FileManager.default.removeItem(at: dir.appendingPathComponent(page.folder))
            HookBridge.writeJSON(["pages": list.map(\.json)], to: listURL)
            writeIndex(list)
        }
    }

    /// One writer of the list at a time, across sessions and the app. Another writer may be busy, so wait
    /// a little, then give up rather than hold Claude Code.
    private static func withLock(_ body: () -> Void) {
        do { try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true) } catch { return }
        let lock = open(dir.appendingPathComponent(".lock").path, O_CREAT | O_RDWR, 0o644)
        guard lock >= 0 else { return }
        defer { close(lock) }
        var tries = 0
        while flock(lock, LOCK_EX | LOCK_NB) != 0 {
            tries += 1
            if tries > 40 { return }
            usleep(50_000)
        }
        defer { flock(lock, LOCK_UN) }
        body()
    }

    private static func readFile(_ url: URL) -> Data? {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir), !isDir.boolValue,
              let size = (try? FileManager.default.attributesOfItem(atPath: url.path))?[.size] as? Int,
              size <= maxFileBytes else { return nil }
        return try? Data(contentsOf: url)
    }

    /// Writes beside the target then renames, so a reader never sees half a file.
    private static func write(_ data: Data, to url: URL) -> Bool {
        let tmp = url.deletingLastPathComponent().appendingPathComponent(".\(url.lastPathComponent).\(getpid()).tmp")
        guard (try? data.write(to: tmp)) != nil else { return false }
        if rename(tmp.path, url.path) == 0 { return true }
        try? FileManager.default.removeItem(at: tmp)
        return false
    }

    /// The first claude.ai artifact link in a tool result, whatever shape the result has.
    /// The version id a publish result names: Claude Code's "version id 1793506664-65e3", or Notchling's
    /// own "(version 1793506664-65e3)".
    private static func publishedVersion(in text: String) -> String {
        guard let r = text.range(of: #"version (?:id )?[0-9]{6,}-[0-9a-f]{2,}"#, options: .regularExpression) else { return "" }
        return String(text[r].split(separator: " ").last ?? "")
    }

    private static func artifactURL(in text: String) -> String? {
        guard let r = text.range(of: #"https://claude\.ai/(?:code/)?artifact/[A-Za-z0-9_-]+"#, options: .regularExpression) else { return nil }
        return String(text[r])
    }

    private static func flatten(_ value: Any?) -> String {
        guard let value else { return "" }
        if let s = value as? String { return s }
        guard JSONSerialization.isValidJSONObject(value),
              let data = try? JSONSerialization.data(withJSONObject: value, options: [.withoutEscapingSlashes]) else { return "\(value)" }
        return String(data: data, encoding: .utf8) ?? ""
    }

    /// The project's folder, also from a subagent's `.claude/worktrees/<name>` checkout.
    private static func projectName(_ cwd: String) -> String {
        let root = cwd.range(of: "/.claude/worktrees/").map { String(cwd[..<$0.lowerBound]) } ?? cwd
        return root.isEmpty ? "" : (root as NSString).lastPathComponent
    }

    private static func activeEmail() -> String {
        // A session in a lane publishes as the lane's account; ~/.claude.json only knows the main one.
        if let lane = LaneStore.current() { return lane.email }
        let claude = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
        let account = HookBridge.readJSON(claude)?["oauthAccount"] as? [String: Any]
        return account?["emailAddress"] as? String ?? ""
    }

    // MARK: - Page content

    /// The page's `<title>` (or first Markdown heading), else the file name.
    static func pageTitle(_ data: Data, ext: String, fallback: String) -> String {
        let text = String(decoding: data.prefix(200_000), as: UTF8.self)
        var found: String?
        if ext == "md" {
            found = text.split(separator: "\n", omittingEmptySubsequences: true)
                .first { $0.hasPrefix("# ") }.map { String($0.dropFirst(2)) }
        } else if let r = text.range(of: #"<title[^>]*>[\s\S]*?</title>"#, options: [.regularExpression, .caseInsensitive]) {
            var inner = String(text[r])
            inner = inner.replacingOccurrences(of: #"^<title[^>]*>|</title>$"#, with: "", options: [.regularExpression, .caseInsensitive])
            found = unescape(inner)
        }
        let clean = (found ?? "").components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ")
        return clean.isEmpty ? fallback : String(clean.prefix(200))
    }

    private static func unescape(_ s: String) -> String {
        s.replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&quot;", with: "\"").replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&amp;", with: "&")
    }

    /// Published pages are fragments (the publisher adds a skeleton), so give them one to render from disk.
    static func wrapped(_ data: Data) -> Data {
        let head = String(decoding: data.prefix(2048), as: UTF8.self).lowercased()
        if head.contains("<!doctype") { return data }
        var out = Data("""
        <!doctype html>
        <html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
        <style>body{margin:0;font:14px system-ui,sans-serif}</style></head><body>

        """.utf8)
        out.append(data)
        out.append(Data("\n</body></html>\n".utf8))
        return out
    }

    // MARK: - Folders and supporting files

    private static func makeFolder(title: String, key: String) -> String {
        var slug = title.lowercased().unicodeScalars.map { c -> Character in
            (c.isASCII && (CharacterSet.alphanumerics.contains(c))) ? Character(c) : "-"
        }.reduce(into: "") { $0.append($1) }
        slug = slug.split(separator: "-", omittingEmptySubsequences: true).joined(separator: "-")
        slug = String(slug.prefix(40))
        if slug.isEmpty { slug = "page" }
        var hash: UInt64 = 0xcbf29ce484222325
        for byte in key.utf8 { hash = (hash ^ UInt64(byte)) &* 0x100000001b3 }
        return slug + "-" + String(format: "%08x", UInt32(truncatingIfNeeded: hash))
    }

    /// Copies the files named in `files` next to the page. Published paths must stay inside the folder.
    private static func copySupporting(_ input: [String: Any], cwd: URL, into folder: URL, skipping main: String) {
        var root = cwd
        if let r = input["root"] as? String, !r.isEmpty { root = URL(fileURLWithPath: r, relativeTo: cwd) }
        var pairs: [(published: String, source: String)] = []
        if let map = input["files"] as? [String: Any] {
            for (published, value) in map {
                if let s = value as? String { pairs.append((published, s)) }
                else if let o = value as? [String: Any], o["artifact"] == nil, let from = o["from"] as? String { pairs.append((published, from)) }
            }
        } else if let array = input["files"] as? [[String: Any]] {
            for o in array { if o["artifact"] == nil, let p = o["path"] as? String { pairs.append((p, p)) } }
        }
        let folderPath = folder.standardizedFileURL.path
        var total = 0
        for (published, source) in pairs.sorted(by: { $0.published < $1.published }) {
            let parts = published.split(separator: "/", omittingEmptySubsequences: true)
            guard !published.hasPrefix("/"), !parts.isEmpty, !parts.contains(".."), !parts.contains("."),
                  published != main else { continue }
            let target = folder.appendingPathComponent(parts.joined(separator: "/")).standardizedFileURL
            guard target.path.hasPrefix(folderPath + "/") else { continue }
            let from = URL(fileURLWithPath: source, relativeTo: root).standardizedFileURL
            guard let data = readFile(from), total + data.count <= maxTotalBytes else { continue }
            total += data.count
            try? FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            _ = write(data, to: target)
        }
    }

    // MARK: - The list page

    static func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;").replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "'", with: "&#39;")
    }

    static func writeIndex(_ list: [Entry]) {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        var rows = ""
        for e in list.sorted(by: { $0.updated > $1.updated }) {
            let href = e.folder.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed).map { "\($0)/\(e.file)" } ?? ""
            let online = e.url.hasPrefix("https://claude.ai/") ? "<a href=\"\(escape(e.url))\">claude.ai</a>" : ""
            rows += "<tr><td><a href=\"\(escape(href))\">\(escape(e.title))</a></td><td>\(escape(e.project))</td>"
                + "<td>\(escape(e.account))</td><td>\(formatter.string(from: Date(timeIntervalSince1970: e.updated)))</td><td>\(online)</td></tr>\n"
        }
        let html = """
        <!doctype html>
        <html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
        <title>Notchling Pages</title>
        <style>
        :root{color-scheme:light dark}
        body{margin:24px;font:14px system-ui,sans-serif}
        table{border-collapse:collapse;width:100%}
        th,td{text-align:left;padding:6px 12px 6px 0;border-bottom:1px solid #8884;vertical-align:top}
        </style></head><body>
        <h1>Pages</h1>
        <p>\(list.count) saved, newest first. Private on claude.ai to the account that published them; these copies open whatever account is active.</p>
        <table><tr><th>Title</th><th>Project</th><th>Account</th><th>Saved</th><th>Online</th></tr>
        \(rows)</table>
        </body></html>

        """
        _ = write(Data(html.utf8), to: indexURL)
    }
}
