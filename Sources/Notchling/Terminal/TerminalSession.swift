import ConsoleCore
import Foundation

/// A live `claude attach <job>` in a pseudo-terminal, drawn by `TerminalScreen`. Closing it detaches;
/// the background session keeps running.
final class TerminalSession: ObservableObject, Identifiable {
    enum Failure: Error, LocalizedError {
        case launch(Int32)
        var errorDescription: String? {
            switch self {
            case .launch(let code): return "Couldn't run claude attach (\(code))."
            }
        }
    }

    let id = UUID()
    let jobID: String
    let title: String
    let screen: TerminalScreen
    /// Called on the main thread after the screen changed.
    var onChange: (() -> Void)?
    @Published private(set) var ended = false

    private var master: Int32 = -1
    private var pid: pid_t = 0
    private var reader: DispatchSourceRead?
    /// Typed once the session has drawn and gone quiet, like a command picked in Chat.
    private var pendingInput: String?
    private var lastOutput: Date?
    private var quietTimer: Timer?

    init(jobID: String, title: String, rows: Int = 30, cols: Int = 100) {
        self.jobID = jobID
        self.title = title
        screen = TerminalScreen(rows: rows, cols: cols)
    }

    @Published private(set) var failure: String?
    private var begun = false
    /// Typed into the session once it has drawn, like "/plugin".
    private var pendingCommand: String?

    convenience init(jobID: String, title: String, command: String?) {
        self.init(jobID: jobID, title: title)
        pendingCommand = command
    }

    /// A shell command from a message, run in `folder` by the user's interactive zsh (so its PATH,
    /// nvm and the like are there), instead of attaching to the session.
    private(set) var script: (text: String, folder: String)?
    /// The command's exit status once it ends.
    @Published private(set) var exitStatus: Int32?

    convenience init(run text: String, in folder: String, jobID: String) {
        self.init(jobID: jobID, title: (folder as NSString).abbreviatingWithTildeInPath)
        script = (text, folder)
    }

    /// Typed key by key instead of pasted: an interactive program that isn't Claude Code (Codex) gets
    /// a slash command the way its command popup expects one, into an input box that starts empty.
    private var typeKeys = false

    /// An interactive program (`codex resume <id>`) run in `folder`, with `typing` entered once it has drawn.
    convenience init(program text: String, in folder: String, title: String, typing: String?) {
        self.init(jobID: "", title: title)
        script = (text, folder)
        pendingCommand = typing
        typeKeys = true
    }

    deinit { close() }

    /// Starts on the view's first layout, at the size it measured.
    func begin(rows: Int, cols: Int) {
        guard !begun else { return resize(rows: rows, cols: cols) }
        begun = true
        screen.resize(rows: rows, cols: cols)
        // zsh -c doesn't echo what it runs; show it like a prompt line.
        if let script {
            let shown = script.text.replacingOccurrences(of: "\n", with: "\r\n  ")
            screen.feed(Array("\u{1b}[2m$ \(shown)\u{1b}[0m\r\n".utf8))
        }
        do { try start(typing: pendingCommand) } catch { failure = error.localizedDescription; ended = true }
    }

    /// Starts the attach. `typing` is sent (after clearing the input box) once the session has drawn.
    func start(typing: String? = nil) throws {
        var env = script == nil ? AgentMonitor.environment(forJob: jobID) : ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        // This view draws color; an inherited NO_COLOR left Claude Code's menus all one gray.
        env["NO_COLOR"] = nil
        let program = script == nil ? ReplySender.claudePath : "/bin/zsh"
        let args = script.map { ["/bin/zsh", "-ilc", $0.text] } ?? [ReplySender.claudePath, "attach", jobID]
        let path = strdup(program)
        let folder = strdup(script?.folder ?? FileManager.default.homeDirectoryForCurrentUser.path)
        let cArgs = args.map { strdup($0) } + [nil]
        let cEnv = env.map { strdup("\($0.key)=\($0.value)") } + [nil]
        defer { ([path, folder] + cArgs + cEnv).forEach { free($0) } }
        var noSignals = sigset_t()
        sigemptyset(&noSignals)
        let maxFD = min(getdtablesize(), 4096)

        // forkpty, not posix_spawn: it makes the pty the child's controlling terminal the way a terminal
        // app does. Started with posix_spawn (SETSID plus opening the pty), `claude attach` got SIGWINCH
        // but never repainted for the new size. The child only resets its signal mask, closes what it
        // inherited and execs, all prepared above, since nothing else is safe after fork.
        var size = winsize(ws_row: UInt16(screen.rows), ws_col: UInt16(screen.cols), ws_xpixel: 0, ws_ypixel: 0)
        var opened: Int32 = -1
        let forked = forkpty(&opened, nil, nil, &size)
        if forked == 0 {
            sigprocmask(SIG_SETMASK, &noSignals, nil)
            var other: Int32 = 3
            while other < maxFD { Darwin.close(other); other += 1 }
            if let folder { _ = chdir(folder) }
            execve(path, cArgs, cEnv)
            _exit(127)
        }
        guard forked > 0 else { throw Failure.launch(errno) }
        pid = forked
        master = opened
        _ = fcntl(master, F_SETFD, FD_CLOEXEC)

        pendingInput = typing
        let source = DispatchSource.makeReadSource(fileDescriptor: master, queue: .global(qos: .userInitiated))
        let fd = master
        source.setCancelHandler { Darwin.close(fd) }
        source.setEventHandler { [weak self] in
            var buffer = [UInt8](repeating: 0, count: 65536)
            let n = read(fd, &buffer, buffer.count)
            guard n > 0 else {
                source.cancel()
                DispatchQueue.main.async { self?.finish() }
                return
            }
            let chunk = Array(buffer[0..<n])
            DispatchQueue.main.async { self?.receive(chunk) }
        }
        reader = source
        source.resume()

        let child = pid
        DispatchQueue.global(qos: .utility).async { [weak self] in
            var status: Int32 = 0
            waitpid(child, &status, 0)
            // WEXITSTATUS / WTERMSIG by hand: Swift doesn't import the macros.
            let code = status & 0x7f == 0 ? (status >> 8) & 0xff : 128 + (status & 0x7f)
            DispatchQueue.main.async { self?.exitStatus = code; self?.finish() }
        }
        quietTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in self?.typeWhenQuiet() }
    }

    private func receive(_ chunk: [UInt8]) {
        screen.feed(chunk)
        let replies = screen.takeReplies()
        if !replies.isEmpty { write(replies) }
        lastOutput = Date()
        onChange?()
    }

    private func typeWhenQuiet() {
        guard let input = pendingInput else { quietTimer?.invalidate(); return }
        guard let last = lastOutput, Date().timeIntervalSince(last) >= 0.5 else { return }
        pendingInput = nil
        quietTimer?.invalidate()
        if typeKeys {
            let keys = Array(input).map(String.init) + ["\r"]
            for (i, key) in keys.enumerated() {
                // A longer pause before Enter, so the popup has filtered to the command.
                let at = Double(i) * 0.04 + (key == "\r" ? 0.4 : 0)
                DispatchQueue.main.asyncAfter(deadline: .now() + at) { [weak self] in self?.send(key) }
            }
            return
        }
        // Same keys as a reply, with the same pauses: sent in one burst, the clear keys read as part of
        // the paste ("Removed 22 invisible characters") and Enter didn't run it.
        send(ReplySender.clearInput)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            self?.send(ConsoleCore.TerminalKeys.bracketed(input))
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { self?.send("\r") }
        }
    }

    func send(_ text: String) { write(Array(text.utf8)) }

    func write(_ bytes: [UInt8]) {
        guard master >= 0, !bytes.isEmpty else { return }
        var offset = 0
        while offset < bytes.count {
            let n = bytes[offset...].withUnsafeBufferPointer { Darwin.write(master, $0.baseAddress, $0.count) }
            if n <= 0 { return }
            offset += n
        }
    }

    /// Text from the clipboard, wrapped as a paste when the program asked for that.
    func paste(_ text: String) {
        send(screen.bracketedPaste ? ConsoleCore.TerminalKeys.bracketed(text) : text)
    }

    func resize(rows: Int, cols: Int) {
        guard rows != screen.rows || cols != screen.cols, rows > 1, cols > 1 else { return }
        screen.resize(rows: rows, cols: cols)
        setWindowSize()
        onChange?()
    }

    private func setWindowSize() {
        guard master >= 0 else { return }
        var size = winsize(ws_row: UInt16(screen.rows), ws_col: UInt16(screen.cols), ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)
    }

    private func finish() {
        guard !ended else { return }
        ended = true
        onChange?()
    }

    /// Detaches: the attach client exits, the session goes on.
    func close() {
        quietTimer?.invalidate()
        quietTimer = nil
        // A command's whole process group (zsh and what it started, like node); an attach just detaches.
        if pid > 0, !ended { kill(script == nil ? pid : -pid, SIGTERM) }
        // The reader's cancel handler closes the pty once no read is running.
        if let reader { reader.cancel() } else if master >= 0 { Darwin.close(master) }
        reader = nil
        master = -1
    }
}
