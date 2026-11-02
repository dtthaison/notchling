import AppKit
import CryptoKit

/// A lane is a Claude Code config folder of its own beside ~/.claude, on another account. Sessions started
/// with `CLAUDE_CONFIG_DIR=<path>` keep their login in the Keychain item
/// `Claude Code-credentials-<first 8 hex of sha256(path)>`, their profile in `<path>/.claude.json`, and
/// get their own background host (`claude --bg` sessions run on the lane's account too). What should be
/// the same everywhere (settings, hooks, skills, plugins, history, memory) is a symlink into ~/.claude.
/// One account is live in one place at a time, since renewing a login retires its old refresh token and a
/// second copy would be signed out.
struct Lane: Codable, Equatable {
    var name: String
    /// The exact value of the variable; Claude Code hashes this string.
    var path: String
    var accountID: String
    var email: String
    /// Where the last terminal for this lane opened.
    var folder: String?

    var service: String { LaneStore.service(for: path) }
    var startCommand: String { "\(LaneStore.variable)=\(LaneStore.quote(path)) claude" }
}

/// Lanes on disk (`lanes.json`, read by the hook too) and their logins in the Keychain.
enum LaneStore {
    static let variable = "CLAUDE_CONFIG_DIR"
    static let fileURL = HookBridge.dir.appendingPathComponent("lanes.json")
    static let home = FileManager.default.homeDirectoryForCurrentUser
    static let mainDir = home.appendingPathComponent(".claude")
    /// Outside ~/.claude, so nothing that walks ~/.claude meets the lanes' links to it.
    static let lanesDir = home.appendingPathComponent(".claude-lanes")
    /// Shared with ~/.claude through symlinks. Everything else stays the lane's own: `.claude.json`
    /// (profile), the background host (`daemon*`), `sessions`, `jobs`, caches, and the account's policy files.
    static let shared = ["settings.json", "CLAUDE.md", "keybindings.json", "statusline.sh", "history.jsonl",
                         "skills", "plugins", "agents", "commands", "output-styles", "projects", "file-history",
                         "paste-cache", "tasks", "todos", "plans"]

    static func service(for path: String) -> String {
        let digest = SHA256.hash(data: Data(path.precomposedStringWithCanonicalMapping.utf8))
        return "Claude Code-credentials-" + digest.map { String(format: "%02x", $0) }.joined().prefix(8)
    }

    static func all() -> [Lane] {
        guard let data = try? Data(contentsOf: fileURL) else { return [] }
        return (try? JSONDecoder().decode([Lane].self, from: data)) ?? []
    }

    static func save(_ lanes: [Lane]) {
        try? FileManager.default.createDirectory(at: HookBridge.dir, withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(lanes) { try? data.write(to: fileURL, options: .atomic) }
    }

    static func lane(forAccount id: String) -> Lane? { all().first { $0.accountID == id } }

    /// The lane this process runs in: a hook inherits its session's environment. Nil for the main login.
    static func current() -> Lane? {
        guard let path = ProcessInfo.processInfo.environment[variable], !path.isEmpty else { return nil }
        return lane(atPath: path)
    }

    static func lane(atPath path: String) -> Lane? {
        let wanted = URL(fileURLWithPath: path).standardizedFileURL.path
        return all().first { URL(fileURLWithPath: $0.path).standardizedFileURL.path == wanted }
    }

    /// Makes the lane's folder: links to the shared parts of ~/.claude, and a `.claude.json` copied from the
    /// main one (projects, trust, MCP servers) with the lane's account as `oauthAccount`.
    static func prepare(_ lane: Lane, profile: Any) throws {
        let fm = FileManager.default
        let dir = URL(fileURLWithPath: lane.path)
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        relink(lane)
        let config = dir.appendingPathComponent(".claude.json")
        if !fm.fileExists(atPath: config.path) {
            let main = home.appendingPathComponent(".claude.json")
            var obj = (try? JSONSerialization.jsonObject(with: Data(contentsOf: main))) as? [String: Any] ?? [:]
            obj["oauthAccount"] = profile
            try writeConfig(obj, to: config)
        } else {
            try setProfile(profile, in: lane)
        }
    }

    /// Points the lane's shared entries at ~/.claude again. Claude Code saves some files by writing a new
    /// file and renaming it over the old one, which replaces a link with a plain file; the newer of the two
    /// copies wins, and the main one is kept as `<name>.lane-backup` when the lane's is taken.
    static func relink(_ lane: Lane) {
        let fm = FileManager.default
        let dir = URL(fileURLWithPath: lane.path)
        for name in shared {
            let link = dir.appendingPathComponent(name)
            let target = mainDir.appendingPathComponent(name)
            if let dest = try? fm.destinationOfSymbolicLink(atPath: link.path) {
                if dest == target.path { continue }
                try? fm.removeItem(at: link)
            } else if fm.fileExists(atPath: link.path) {
                var isDir: ObjCBool = false
                fm.fileExists(atPath: link.path, isDirectory: &isDir)
                let mine = (try? fm.attributesOfItem(atPath: link.path))?[.modificationDate] as? Date ?? .distantPast
                let theirs = (try? fm.attributesOfItem(atPath: target.path))?[.modificationDate] as? Date ?? .distantPast
                if !isDir.boolValue, mine > theirs {
                    let backup = mainDir.appendingPathComponent(name + ".lane-backup")
                    try? fm.removeItem(at: backup)
                    try? fm.copyItem(at: target, to: backup)
                    try? fm.removeItem(at: target)
                    try? fm.moveItem(at: link, to: target)
                } else {
                    // A folder made before the link existed, or an older file: keep it aside, never delete it.
                    try? fm.moveItem(at: link, to: dir.appendingPathComponent(name + ".lane-old-\(Int(Date().timeIntervalSince1970))"))
                }
            }
            try? fm.createSymbolicLink(at: link, withDestinationURL: target)
        }
    }

    /// Sets the lane's account in its `.claude.json`, so `/status` and the rest of Claude Code name it.
    static func setProfile(_ profile: Any, in lane: Lane) throws {
        let config = URL(fileURLWithPath: lane.path).appendingPathComponent(".claude.json")
        var obj = (try? JSONSerialization.jsonObject(with: Data(contentsOf: config))) as? [String: Any] ?? [:]
        obj["oauthAccount"] = profile
        try writeConfig(obj, to: config)
    }

    private static func writeConfig(_ obj: [String: Any], to url: URL) throws {
        let data = try JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .withoutEscapingSlashes])
        try data.write(to: url, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    /// Removes the lane's own folder. Links are removed as links; what they point to stays.
    static func removeFolder(_ lane: Lane) {
        let dir = URL(fileURLWithPath: lane.path).standardizedFileURL
        guard dir.path.hasPrefix(lanesDir.standardizedFileURL.path + "/") else { return }
        try? FileManager.default.removeItem(at: dir)
    }

    /// The lane's `claudeAiOauth`, as Claude Code last wrote it (it renews the login in place).
    static func login(_ lane: Lane) -> [String: Any]? {
        item(lane)?["claudeAiOauth"] as? [String: Any]
    }

    /// Puts a login in the lane, keeping anything else Claude Code stored in the item (MCP logins).
    static func setLogin(_ oauth: [String: Any], in lane: Lane) -> Bool {
        var value = item(lane) ?? [:]
        value["claudeAiOauth"] = oauth
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.withoutEscapingSlashes]) else { return false }
        return Keychain.write(service: lane.service, account: NSUserName(), value: String(decoding: data, as: UTF8.self))
    }

    static func removeLogin(_ lane: Lane) {
        Keychain.delete(service: lane.service, account: NSUserName())
    }

    private static func item(_ lane: Lane) -> [String: Any]? {
        guard let text = Keychain.read(service: lane.service, account: NSUserName()) else { return nil }
        // `security -w` prints hex when the value isn't plain text.
        let data = Data(text.utf8)
        if let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] { return obj }
        var bytes = [UInt8]()
        var hex = text[...]
        while hex.count >= 2, let b = UInt8(hex.prefix(2), radix: 16) { bytes.append(b); hex = hex.dropFirst(2) }
        return (try? JSONSerialization.jsonObject(with: Data(bytes))) as? [String: Any]
    }

    /// A short name from the email's first part, unique among the lanes.
    static func newName(for email: String, taken: [String]) -> String {
        let base = String(email.lowercased().split(separator: "@").first ?? "lane")
            .map { $0.isLetter || $0.isNumber ? String($0) : "-" }.joined()
            .trimmingCharacters(in: CharacterSet(charactersIn: "-"))
        let stem = base.isEmpty ? "lane" : String(base.prefix(24))
        var name = stem
        var n = 2
        while taken.contains(name) { name = "\(stem)-\(n)"; n += 1 }
        return name
    }

    static func quote(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    /// Opens Terminal in `folder` with a Claude Code session on the lane's login.
    static func openTerminal(_ lane: Lane, in folder: String) throws {
        let script = HookBridge.dir.appendingPathComponent("lane-\(lane.name).command")
        let text = "#!/bin/zsh -l\n"
            + "cd \(quote(folder)) && export \(variable)=\(quote(lane.path)) && exec \(quote(ReplySender.claudePath))\n"
        try text.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
        NSWorkspace.shared.open(script)
    }
}

// MARK: - Which lane a session runs in

extension LaneStore {
    /// The lane of a session, by session id: its `sessions/<pid>.json` is in the lane's own folder (background
    /// sessions too: each config folder has its own background host). Nil means the main login.
    static func lane(forSession id: String) -> Lane? {
        for lane in all() where sessionIDs(in: URL(fileURLWithPath: lane.path)).contains(id) { return lane }
        return nil
    }

    /// Every config folder sessions can live in: ~/.claude, then each lane's. Each has its own `sessions`
    /// and `jobs`; `projects` (transcripts) is shared. To reach a lane's background session, run `claude`
    /// (attach, stop, logs) with `CLAUDE_CONFIG_DIR` set to the lane's path.
    static func configDirs() -> [(dir: URL, lane: Lane?)] {
        [(mainDir, nil)] + all().map { (URL(fileURLWithPath: $0.path), $0) }
    }

    private static func sessionIDs(in dir: URL) -> Set<String> {
        let sessions = dir.appendingPathComponent("sessions")
        var ids = Set<String>()
        for file in (try? FileManager.default.contentsOfDirectory(at: sessions, includingPropertiesForKeys: nil)) ?? []
        where file.pathExtension == "json" {
            if let data = try? Data(contentsOf: file),
               let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
               let id = obj["sessionId"] as? String { ids.insert(id) }
        }
        return ids
    }

    /// The lane a Claude Code process is in, from its environment; nil for the main login.
    static func lane(forPID pid: pid_t) -> Lane? {
        guard let path = environment(of: pid)[variable], !path.isEmpty else { return nil }
        return lane(atPath: path)
    }

    /// Another process's environment (same user), through `KERN_PROCARGS2`: argc, the executable path,
    /// padding, argv, then the environment.
    static func environment(of pid: pid_t) -> [String: String] {
        var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
        var size = 0
        guard sysctl(&mib, 3, nil, &size, nil, 0) == 0, size > MemoryLayout<Int32>.size else { return [:] }
        var buffer = [UInt8](repeating: 0, count: size)
        guard sysctl(&mib, 3, &buffer, &size, nil, 0) == 0 else { return [:] }
        let argc = Int(buffer.withUnsafeBytes { $0.load(as: Int32.self) })
        var i = MemoryLayout<Int32>.size
        while i < size, buffer[i] != 0 { i += 1 }
        while i < size, buffer[i] == 0 { i += 1 }
        var strings: [String] = []
        while i < size {
            let start = i
            while i < size, buffer[i] != 0 { i += 1 }
            if i > start { strings.append(String(decoding: buffer[start..<i], as: UTF8.self)) } else if strings.count > argc { break }
            i += 1
        }
        var env: [String: String] = [:]
        for entry in strings.dropFirst(argc) {
            guard let eq = entry.firstIndex(of: "=") else { continue }
            env[String(entry[..<eq])] = String(entry[entry.index(after: eq)...])
        }
        return env
    }
}

/// The "Lanes" submenu: the main login and each lane with its account, and New Lane for free accounts.
final class LanesMenu: NSObject {
    private let accounts: AccountStore

    init(accounts: AccountStore) {
        self.accounts = accounts
    }

    func item() -> NSMenuItem {
        let item = NSMenuItem(title: "Lanes", action: nil, keyEquivalent: "")
        item.submenu = menu()
        return item
    }

    private func menu() -> NSMenu {
        let menu = NSMenu()
        let lanes = LaneStore.all()
        let info = NSMenuItem(title: "Main: \(accounts.active?.email ?? "not signed in")", action: nil, keyEquivalent: "")
        info.isEnabled = false
        menu.addItem(info)

        for lane in lanes {
            let row = NSMenuItem(title: "\(lane.name): \(lane.email)\(usageText(lane.accountID))", action: nil, keyEquivalent: "")
            let sub = NSMenu()
            sub.addItem(action("Open Terminal…", #selector(openTerminal(_:)), lane.name))
            let copy = action("Copy Start Command", #selector(copyCommand(_:)), lane.name)
            copy.toolTip = "For VS Code's terminal or a script. Add --bg to start a background session on this account."
            sub.addItem(copy)
            let switchItem = NSMenuItem(title: "Switch To", action: nil, keyEquivalent: "")
            let targets = NSMenu()
            for account in freeAccounts() {
                targets.addItem(action(account.email + usageText(account.id), #selector(switchLane(_:)), [lane.name, account.id]))
            }
            if targets.items.isEmpty { targets.addItem(disabled("No free account")) }
            switchItem.submenu = targets
            sub.addItem(switchItem)
            sub.addItem(.separator())
            sub.addItem(action("Close Lane…", #selector(closeLane(_:)), lane.name))
            row.submenu = sub
            menu.addItem(row)
        }

        menu.addItem(.separator())
        let newItem = NSMenuItem(title: "New Lane", action: nil, keyEquivalent: "")
        let choices = NSMenu()
        for account in freeAccounts() {
            choices.addItem(action(account.email + usageText(account.id), #selector(newLane(_:)), account.id))
        }
        if choices.items.isEmpty { choices.addItem(disabled("No free account: each account runs in one place")) }
        newItem.submenu = choices
        newItem.toolTip = "Run Claude Code on another account beside the main one. Lanes share settings and history; each has its own login."
        menu.addItem(newItem)
        return menu
    }

    /// Saved, signed in, and not live in the main login or another lane.
    private func freeAccounts() -> [ClaudeAccount] {
        let live = Set(LaneStore.all().map(\.accountID) + [accounts.activeID].compactMap { $0 })
        return accounts.accounts.filter { !live.contains($0.id) && !$0.needsLogin }
    }

    private func usageText(_ id: String) -> String {
        guard let u = accounts.usage[id] else { return "" }
        let parts = [u.session.map { "5h \(Int($0.percent.rounded()))%" }, u.week.map { "week \(Int($0.percent.rounded()))%" }].compactMap { $0 }
        return parts.isEmpty ? "" : "  (" + parts.joined(separator: ", ") + ")"
    }

    private func action(_ title: String, _ selector: Selector, _ value: Any) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
        item.target = self
        item.representedObject = value
        return item
    }

    private func disabled(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }

    // MARK: Actions

    @objc private func newLane(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String, let folder = pickFolder(nil) else { return }
        accounts.openLane(for: id) { [weak self] result in
            switch result {
            case .success(let lane): self?.open(lane, in: folder)
            case .failure(let error): Self.alert("Couldn't start the lane", error.localizedDescription)
            }
        }
    }

    @objc private func openTerminal(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String, let lane = LaneStore.all().first(where: { $0.name == name }),
              let folder = pickFolder(lane.folder) else { return }
        open(lane, in: folder)
    }

    @objc private func copyCommand(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String, let lane = LaneStore.all().first(where: { $0.name == name }) else { return }
        Self.copy(lane.startCommand)
    }

    private static func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    @objc private func switchLane(_ sender: NSMenuItem) {
        guard let pair = sender.representedObject as? [String], pair.count == 2 else { return }
        accounts.switchLane(pair[0], to: pair[1]) { result in
            if case .failure(let error) = result { Self.alert("Couldn't switch the lane", error.localizedDescription) }
        }
    }

    @objc private func closeLane(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String, let lane = LaneStore.all().first(where: { $0.name == name }) else { return }
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = "Close the \(lane.name) lane?"
        alert.informativeText = "Claude Code sessions still running in it are signed out. \(lane.email) goes back to the saved accounts."
        alert.addButton(withTitle: "Close Lane")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        accounts.closeLane(name)
    }

    private func open(_ lane: Lane, in folder: String) {
        do {
            try LaneStore.openTerminal(lane, in: folder)
            var lanes = LaneStore.all()
            if let i = lanes.firstIndex(where: { $0.name == lane.name }) { lanes[i].folder = folder; LaneStore.save(lanes) }
        } catch {
            Self.alert("Couldn't open Terminal", error.localizedDescription)
        }
    }

    private func pickFolder(_ start: String?) -> String? {
        NSApp.activate(ignoringOtherApps: true)
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.prompt = "Open Here"
        panel.message = "The project folder to start Claude Code in."
        if let start { panel.directoryURL = URL(fileURLWithPath: start) }
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    private static func alert(_ title: String, _ text: String) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        alert.runModal()
    }
}
