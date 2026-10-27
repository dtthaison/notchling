import AppKit

/// Opening an agent where it lives: a Terminal running `claude attach`, or the window that hosts it.
enum AgentActions {
    static func open(_ target: AgentTarget, cwd: String = "") {
        switch target {
        case .job(let id):
            runInTerminal("\(shellQuote(ReplySender.claudePath)) attach \(id)")
        case .resume(let sessionID, let cwd):
            runInTerminal("cd \(shellQuote(cwd)) && \(shellQuote(ReplySender.claudePath)) --resume \(sessionID)")
        case .process(let pid):
            if !jump(to: pid, cwd: cwd) { NSSound.beep() }
        }
    }

    /// Brings forward the exact place the session runs: its Terminal or iTerm tab, or the editor
    /// window that has its folder open. Falls back to just activating the app.
    private static func jump(to pid: pid_t, cwd: String) -> Bool {
        guard let app = hostApp(of: pid) else { return false }

        // The Claude app hosts many Code sessions in one window; activating it shows whatever was
        // last open (often Chat), so ask it for this session by its own id.
        if app.bundleIdentifier == "com.anthropic.claudefordesktop",
           let id = desktopSessionID(of: pid),
           let url = URL(string: "claude://code/continue?session=\(id)") {
            // We're a background app, so macOS won't bring Claude forward for the link on its own.
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.activates = true
            NSWorkspace.shared.open(url, configuration: configuration)
            if #available(macOS 14, *) { app.activate() } else { app.activate(options: []) }
            return true
        }

        switch (app.bundleIdentifier, ttyPath(of: pid)) {
        case ("com.apple.Terminal", let tty?):
            runAppleScript("""
                tell application "Terminal"
                    repeat with w in windows
                        repeat with t in tabs of w
                            if tty of t is "\(tty)" then
                                set selected of t to true
                                set index of w to 1
                            end if
                        end repeat
                    end repeat
                end tell
                """)
        case ("com.googlecode.iterm2", let tty?):
            runAppleScript("""
                tell application "iTerm2"
                    repeat with w in windows
                        repeat with t in tabs of w
                            repeat with s in sessions of t
                                if tty of s is "\(tty)" then
                                    select w
                                    select t
                                    select s
                                end if
                            end repeat
                        end repeat
                    end repeat
                end tell
                """)
        default:
            // VS Code, Cursor and friends focus the window that already has the folder open.
            if !cwd.isEmpty, let bundle = app.bundleURL {
                NSWorkspace.shared.open([URL(fileURLWithPath: cwd)], withApplicationAt: bundle,
                                        configuration: NSWorkspace.OpenConfiguration())
            }
        }
        if #available(macOS 14, *) { app.activate() } else { app.activate(options: []) }
        return true
    }

    /// The Claude app's own id for a session it hosts ("local_…"), from Claude Code's per-process
    /// file. Only ids the app's deep link accepts come back.
    private static func desktopSessionID(of pid: pid_t) -> String? {
        let file = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".claude/sessions/\(pid).json")
        guard let data = try? Data(contentsOf: file),
              let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let id = json["hostSessionId"] as? String,
              id.range(of: #"^local_[A-Za-z0-9-]{1,64}$"#, options: .regularExpression) != nil
        else { return nil }
        return id
    }

    private static func runAppleScript(_ source: String) {
        DispatchQueue.global(qos: .userInitiated).async {
            var error: NSDictionary?
            NSAppleScript(source: source)?.executeAndReturnError(&error)
        }
    }

    /// The session's terminal device, like /dev/ttys012.
    private static func ttyPath(of pid: pid_t) -> String? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        let dev = info.kp_eproc.e_tdev
        guard dev != -1, let name = devname(dev, S_IFCHR) else { return nil }
        return "/dev/" + String(cString: name)
    }

    private static var hostNames: [pid_t: String] = [:]
    private static let lock = NSLock()

    /// Name of the app hosting the session's terminal, cached per process.
    static func hostAppName(of pid: pid_t) -> String? {
        lock.lock(); defer { lock.unlock() }
        if let name = hostNames[pid] { return name }
        guard let name = hostApp(of: pid)?.localizedName else { return nil }
        hostNames[pid] = name == "Code" ? "VS Code" : name
        return hostNames[pid]
    }

    private static func hostApp(of pid: pid_t) -> NSRunningApplication? {
        var current = pid
        for _ in 0..<32 {
            if let app = NSRunningApplication(processIdentifier: current), app.activationPolicy == .regular {
                return app
            }
            guard let parent = parentPID(of: current) else { return nil }
            current = parent
        }
        return nil
    }

    private static func parentPID(of pid: pid_t) -> pid_t? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        let parent = info.kp_eproc.e_ppid
        return parent > 1 ? parent : nil
    }

    /// Opens the command in the default terminal app via a throwaway .command file.
    private static func runInTerminal(_ command: String) {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("awake-\(UUID().uuidString).command")
        let script = "#!/bin/zsh -l\nrm -f \(shellQuote(url.path))\n\(command)\n"
        do {
            try script.write(to: url, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
            NSWorkspace.shared.open(url)
        } catch {
            NSSound.beep()
        }
    }

    private static func shellQuote(_ s: String) -> String {
        "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}
