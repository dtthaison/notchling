import ConsoleCore
import SwiftUI

/// Paths and commands in a message: paths that exist become links that show them in Finder, and
/// shell code blocks can be run. Relative paths ("src/timeline.ts") are read against the folders
/// the same message names ("The project is in ~/Downloads/reel/edit/"), then the session's folder.
enum ChatPaths {
    typealias Exists = (String) -> Bool
    static let onDisk: Exists = { FileManager.default.fileExists(atPath: $0) }
    static let isFolder: Exists = { path in
        var dir: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &dir) && dir.boolValue
    }

    /// Folders to read relative paths against: folders the text names (absolute, then relative to the session's
    /// folder: "Desktop/Voices/" followed by a bare "line.mp3"), then the session's.
    static func bases(in text: String, cwd: String?, isFolder: Exists = isFolder) -> [String] {
        var out: [String] = []
        for (_, token) in tokens(in: text) where token.hasPrefix("/") || token.hasPrefix("~/") {
            let path = expand(token)
            if isFolder(path), !out.contains(path) { out.append(path) }
        }
        if let cwd, !cwd.isEmpty {
            for (_, token) in tokens(in: text) where !token.hasPrefix("/") && !token.hasPrefix("~") && !token.contains("://") && !ChatAudio.isAudioName(token) {
                let path = ((cwd as NSString).appendingPathComponent(token) as NSString).standardizingPath
                if isFolder(path), !out.contains(path) { out.append(path) }
            }
            if !out.contains(cwd) { out.append(cwd) }
        }
        return out
    }

    /// The existing file or folder a token names, or nil.
    static func resolve(_ token: String, bases: [String], exists: Exists = onDisk) -> String? {
        // A bare audio name ("line.mp3") counts: agents list voice files under a folder they named once.
        guard token.contains("/") || ChatAudio.isAudioName(token), !token.contains("://"), token != "/", token != "~/" else { return nil }
        if token.hasPrefix("/") || token.hasPrefix("~") {
            let path = expand(token)
            return exists(path) ? path : nil
        }
        for base in bases {
            let path = (base as NSString).appendingPathComponent(token)
            if exists(path) { return (path as NSString).standardizingPath }
        }
        return nil
    }

    /// Markdown destinations such as `/Users/me/image.png` have no scheme. Convert them to file
    /// URLs so they reach the file panel instead of Launch Services as an application URL.
    static func fileURL(for url: URL, bases: [String], exists: Exists = onDisk) -> URL? {
        if url.isFileURL { return url.standardizedFileURL }
        guard url.scheme == nil, url.host == nil else { return nil }
        let path = url.path
        if path.hasPrefix("/") || path.hasPrefix("~/") {
            return URL(fileURLWithPath: expand(path), isDirectory: url.hasDirectoryPath)
        }
        guard !path.isEmpty else { return nil }
        // Explicit markdown destinations can be bare filenames, unlike tokens detected in prose.
        for base in bases {
            let resolved = ((base as NSString).appendingPathComponent(path) as NSString).standardizingPath
            if exists(resolved) { return URL(fileURLWithPath: resolved, isDirectory: url.hasDirectoryPath) }
        }
        return nil
    }

    /// Ranges (UTF-16, for NSAttributedString) of tokens that name existing paths, with the path.
    static func links(in text: String, bases: [String], exists: Exists = onDisk) -> [(NSRange, String)] {
        tokens(in: text).compactMap { range, token in
            resolve(token, bases: bases, exists: exists).map { (range, $0) }
        }
    }

    // MARK: Commands

    private static let shellLanguages: Set<String> = ["bash", "sh", "shell", "zsh", "console", "terminal", "command"]

    /// A fenced block that is a shell command, without "$ " prompts. Nil for code in other languages,
    /// and for unlabeled blocks unless they're a few lines that start with a program on this Mac.
    static func command(language: String, text: String, isProgram: (String) -> Bool = isProgram) -> String? {
        let lang = language.lowercased()
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false).map { line -> String in
            let s = String(line)
            return s.hasPrefix("$ ") ? String(s.dropFirst(2)) : s
        }
        let script = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !script.isEmpty else { return nil }
        if shellLanguages.contains(lang) {
            // Output pasted in a console block (no prompt on any line) isn't a command.
            if lang == "console", !text.contains("$ ") { return nil }
            return script
        }
        guard lang.isEmpty, lines.count <= 3,
              let first = script.split(whereSeparator: \.isWhitespace).first, isProgram(String(first)) else { return nil }
        return script
    }

    /// `npx remotion render …` written as inline code: a program on this Mac and at least one argument.
    static func inlineCommand(_ code: String, isProgram: (String) -> Bool = isProgram) -> String? {
        let words = code.split(whereSeparator: \.isWhitespace)
        guard words.count >= 2, !code.contains("\n"), isProgram(String(words[0])) else { return nil }
        return code.trimmingCharacters(in: .whitespaces)
    }

    static let runScheme = "notchling-run"

    /// A link that runs `command` in `folder` when clicked (`ChatModel.open` asks first).
    static func runLink(_ command: String, folder: String?) -> URL? {
        var parts = URLComponents()
        parts.scheme = runScheme
        parts.host = "run"
        parts.queryItems = [URLQueryItem(name: "c", value: command)] + (folder.map { [URLQueryItem(name: "d", value: $0)] } ?? [])
        return parts.url
    }

    static func parseRunLink(_ url: URL) -> (command: String, folder: String?)? {
        guard url.scheme == runScheme, let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
              let command = items.first(where: { $0.name == "c" })?.value else { return nil }
        return (command, items.first(where: { $0.name == "d" })?.value)
    }

    /// The folder a command should run in: the first base under which a relative path it names
    /// exists (`npx remotion render src/index.ts` runs where src/index.ts is), else the last base
    /// (the session's folder). A command that changes folder itself runs in the session's folder.
    static func directory(for command: String, bases: [String], exists: Exists = onDisk) -> String? {
        guard !command.hasPrefix("cd ") else { return bases.last }
        let relative = command.split(whereSeparator: { $0.isWhitespace || $0 == "=" }).map(String.init).filter {
            $0.contains("/") && !$0.hasPrefix("-") && !$0.hasPrefix("/") && !$0.hasPrefix("~") && !$0.contains("://")
        }
        for base in bases where relative.contains(where: { exists((base as NSString).appendingPathComponent(trim($0))) }) {
            return base
        }
        return bases.last
    }

    /// A program on PATH or in the usual install folders (a login shell's PATH isn't ours).
    static let isProgram: (String) -> Bool = { name in
        guard !name.isEmpty, !name.contains("/") else { return false }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let path = (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map(String.init)
        let dirs = path + ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", home + "/.local/bin", home + "/.bun/bin"]
        return dirs.contains { FileManager.default.isExecutableFile(atPath: ($0 as NSString).appendingPathComponent(name)) }
    }

    // MARK: Tokens

    private static let tokenPattern = try! NSRegularExpression(pattern: "[^\\s()\\[\\]<>\"'`,;|]+")

    /// Words that could be paths, without trailing punctuation, with their UTF-16 ranges.
    static func tokens(in text: String) -> [(NSRange, String)] {
        let ns = text as NSString
        return tokenPattern.matches(in: text, range: NSRange(location: 0, length: ns.length)).compactMap { match in
            let raw = ns.substring(with: match.range)
            let token = trim(raw)
            guard token.contains("/") || ChatAudio.isAudioName(token) else { return nil }
            return (NSRange(location: match.range.location, length: (token as NSString).length), token)
        }
    }

    private static func trim(_ token: String) -> String {
        var t = token
        while let last = t.last, ".:!?".contains(last) { t.removeLast() }
        return t
    }

    private static func expand(_ token: String) -> String {
        ((token as NSString).expandingTildeInPath as NSString).standardizingPath
    }
}

private struct ChatFolderKey: EnvironmentKey { static let defaultValue: String? = nil }
private struct RunCommandKey: EnvironmentKey { static let defaultValue: ((String, String?) -> Void)? = nil }

extension EnvironmentValues {
    /// The session's folder, for paths and commands in its messages.
    var chatFolder: String? {
        get { self[ChatFolderKey.self] }
        set { self[ChatFolderKey.self] = newValue }
    }

    /// Runs a command from a message in a folder; nil where commands can't run (the notch).
    var runCommand: ((String, String?) -> Void)? {
        get { self[RunCommandKey.self] }
        set { self[RunCommandKey.self] = newValue }
    }
}
