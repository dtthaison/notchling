import Foundation

struct CommandResult: Equatable {
    let code: Int32
    let out: String
    let err: String
}

/// Runs one command and waits for it. Injectable so tests never start ssh. `Sendable`, because a setup step's
/// context carries the runner across tasks.
protocol CommandRunning: Sendable {
    /// `argv[0]` is the executable's path. A timeout returns code 124, a missing executable code 127.
    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult

    /// The same with `env` laid over this app's own environment (a key in `env` wins). A runner that cannot
    /// pass an environment on (a test double) falls back to the plain call.
    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult

    /// The same with `stdin` written to the command's standard input (then closed), and `onOutput` called with
    /// the stdout and stderr text as it arrives. A secret travels only here: never in `argv` or `env`.
    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult
}

extension CommandRunning {
    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, timeout: timeout)
    }

    /// A runner that does not implement stdin or streaming refuses a call that needs them: running the command
    /// without its stdin would send a half-done step on, and dropping a secret silently would hide that.
    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        guard stdin == nil, onOutput == nil else {
            return CommandResult(code: 127, out: "", err: "this runner takes no stdin or stream")
        }
        return await run(argv, cwd: cwd, env: env, timeout: timeout)
    }
}

/// Which pipe a chunk of `onOutput` text came from.
enum OutputChannel: Sendable { case stdout, stderr }

/// `onOutput` is one closure for both pipes, and its callers (`FixContext.log`) keep its signature. While the
/// runner calls it for a chunk, this says which pipe the chunk came from, so a consumer that assembles lines
/// keeps one partial line per pipe: a value cut between two chunks of stdout must not meet stderr text.
/// A runner that does not tell (a test double) reads as `.stdout`.
enum OutputStreamTag {
    private static let key = "notchling.output-channel"

    /// The pipe of the chunk being delivered on this thread.
    static var current: OutputChannel {
        (Thread.current.threadDictionary.object(forKey: key) as? String) == "stderr" ? .stderr : .stdout
    }

    /// Runs `body` with `current` set to `channel`; a runner wraps each `onOutput` call in it.
    static func with<T>(_ channel: OutputChannel, _ body: () -> T) -> T {
        let dictionary = Thread.current.threadDictionary
        let previous = dictionary.object(forKey: key)
        dictionary.setObject(channel == .stderr ? "stderr" : "stdout", forKey: key as NSString)
        defer {
            if let previous { dictionary.setObject(previous, forKey: key as NSString) } else { dictionary.removeObject(forKey: key) }
        }
        return body()
    }
}

extension ProcessRunner {
    /// The most output kept from one pipe; what came before is cut.
    static let outputCap = 2 * 1024 * 1024
    /// The code when the child's exit status could not be read (`waitpid` failed with ECHILD).
    static let lostStatus: Int32 = 125
}

/// Runs a command in a process group of its own, reading both pipes on background queues so a full pipe never
/// stops it. A timeout signals the whole group, so what the command started (a build's compilers, a test's
/// servers) goes with it.
struct ProcessRunner: CommandRunning {
    /// How long a group gets to end after SIGTERM before it is killed.
    private static let termGrace: TimeInterval = 2
    /// How long the output may keep arriving after the command ended. An orphan that kept the pipes open must not
    /// hold the result back.
    private static let drainGrace: TimeInterval = 3
    static let cutNotice = "[earlier output cut]\n"

    func run(_ argv: [String], cwd: String? = nil, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: env, stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                continuation.resume(returning: Self.runBlocking(argv, cwd: cwd, env: env, stdin: stdin,
                                                                timeout: timeout, onOutput: onOutput))
            }
        }
    }

    /// The last `outputCap` bytes read from one pipe, safe to read while the reader is still running. Output
    /// past the cap cuts its start: the end of a log is the part that says why a command failed.
    private final class Sink: @unchecked Sendable {
        private let lock = NSLock()
        private var data = Data()
        private var cut = false
        func append(_ chunk: Data) {
            lock.withLock {
                data.append(chunk)
                // trim in steps, so a long stream does not copy the whole buffer on every read
                if data.count > ProcessRunner.outputCap + 256 * 1024 {
                    data = Data(data.suffix(ProcessRunner.outputCap))
                    cut = true
                }
            }
        }
        var text: String {
            lock.withLock {
                var tail = data
                if tail.count > ProcessRunner.outputCap { tail = Data(tail.suffix(ProcessRunner.outputCap)); cut = true }
                guard cut else { return String(decoding: tail, as: UTF8.self) }
                // the cut may fall inside a character: start at the next one
                let body = tail.drop { $0 & 0xC0 == 0x80 }
                return ProcessRunner.cutNotice + String(decoding: body, as: UTF8.self)
            }
        }
    }

    /// Passes text on to `onOutput` while the run lasts. A reader that outlives the result (an orphan holding
    /// the pipe) delivers nothing after `close()`.
    private final class Relay: @unchecked Sendable {
        private let lock = NSLock()
        private var open = true
        private let sink: (@Sendable (String) -> Void)?
        init(_ sink: (@Sendable (String) -> Void)?) { self.sink = sink }
        func send(_ text: String, from channel: OutputChannel) {
            guard let sink, !text.isEmpty, lock.withLock({ open }) else { return }
            OutputStreamTag.with(channel) { sink(text) }
        }
        func close() { lock.withLock { open = false } }
    }

    /// The number of bytes at the end of `bytes` that begin a UTF-8 character that is not complete yet.
    private static func incompleteTail(_ bytes: Data) -> Int {
        var back = 0
        for byte in bytes.reversed().prefix(3) {
            back += 1
            if byte & 0xC0 == 0x80 { continue }                      // a continuation byte: look further back
            let need = byte >= 0xF0 ? 4 : byte >= 0xE0 ? 3 : byte >= 0xC0 ? 2 : 1
            return need > back ? back : 0
        }
        return 0
    }

    /// The exit status of the child, set by the thread that reaps it.
    private final class Exit: @unchecked Sendable {
        private let lock = NSLock()
        private var value: Int32 = 0
        private var isLost = false
        func set(_ status: Int32) { lock.withLock { value = status } }
        func lose() { lock.withLock { isLost = true } }
        var lost: Bool { lock.withLock { isLost } }
        var code: Int32 {
            lock.withLock {
                // a normal exit gives its code; death by a signal gives the signal number
                value & 0x7f == 0 ? (value >> 8) & 0xff : value & 0x7f
            }
        }
    }

    /// Set when the run is over, so a writer stuck on a pipe nobody reads gives up.
    private final class Flag: @unchecked Sendable {
        private let lock = NSLock()
        private var value = false
        func set() { lock.withLock { value = true } }
        var isSet: Bool { lock.withLock { value } }
    }

    private static func fail(_ message: String) -> CommandResult { CommandResult(code: 127, out: "", err: message) }

    /// SIGTERM to what is left of the group, SIGKILL after `termGrace`.
    private static func endGroup(_ pgid: pid_t) {
        guard killpg(pgid, 0) == 0 else { return }
        killpg(pgid, SIGTERM)
        let deadline = Date().addingTimeInterval(termGrace)
        // the group is gone once nothing in it is left, the leader included
        while Date() < deadline && killpg(pgid, 0) == 0 { usleep(50_000) }
        if killpg(pgid, 0) == 0 { killpg(pgid, SIGKILL) }
    }

    /// Writes `data` to `fd` and closes it. The descriptor does not raise SIGPIPE and is not blocking, so a child
    /// that never reads (or has gone) ends the write instead of hanging it; `stop` ends it too.
    private static func writeAll(_ data: Data, to fd: Int32, stop: Flag) {
        defer { close(fd) }
        var offset = 0
        data.withUnsafeBytes { raw in
            while offset < raw.count && !stop.isSet {
                let n = write(fd, raw.baseAddress! + offset, raw.count - offset)
                if n > 0 { offset += n; continue }
                if n < 0 && errno == EINTR { continue }
                if n < 0 && errno == EAGAIN {
                    var waiting = pollfd(fd: fd, events: Int16(POLLOUT), revents: 0)
                    _ = poll(&waiting, 1, 100)
                    continue
                }
                break                                                  // EPIPE: the child is not reading any more
            }
        }
    }

    private static func runBlocking(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?,
                                    timeout: TimeInterval, onOutput: (@Sendable (String) -> Void)?) -> CommandResult {
        guard let exe = argv.first, FileManager.default.isExecutableFile(atPath: exe) else {
            return fail("\(argv.first ?? "") not found")
        }

        var outFDs: [Int32] = [-1, -1], errFDs: [Int32] = [-1, -1], inFDs: [Int32] = [-1, -1]
        guard pipe(&outFDs) == 0 else { return fail("cannot make a pipe: \(String(cString: strerror(errno)))") }
        guard pipe(&errFDs) == 0 else {
            outFDs.forEach { close($0) }
            return fail("cannot make a pipe: \(String(cString: strerror(errno)))")
        }
        if stdin != nil, pipe(&inFDs) != 0 {
            (outFDs + errFDs).forEach { close($0) }
            return fail("cannot make a pipe: \(String(cString: strerror(errno)))")
        }
        let devNull = stdin == nil ? open("/dev/null", O_RDONLY) : -1
        // Ours only: the child gets its own copies through the file actions below.
        for fd in outFDs + errFDs + inFDs + [devNull] where fd >= 0 { _ = fcntl(fd, F_SETFD, FD_CLOEXEC) }
        if inFDs[1] >= 0 {
            _ = fcntl(inFDs[1], F_SETNOSIGPIPE, 1)
            _ = fcntl(inFDs[1], F_SETFL, fcntl(inFDs[1], F_GETFL) | O_NONBLOCK)
        }

        var actions: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&actions)
        defer { posix_spawn_file_actions_destroy(&actions) }
        posix_spawn_file_actions_adddup2(&actions, stdin == nil ? devNull : inFDs[0], 0)
        posix_spawn_file_actions_adddup2(&actions, outFDs[1], 1)
        posix_spawn_file_actions_adddup2(&actions, errFDs[1], 2)
        if let cwd { posix_spawn_file_actions_addchdir_np(&actions, cwd) }

        var attributes: posix_spawnattr_t?
        posix_spawnattr_init(&attributes)
        defer { posix_spawnattr_destroy(&attributes) }
        // Its own group (the leader's pid is the group id), no descriptor of ours leaks into the child, and it
        // starts with default signals: this thread's mask and any signal the app ignores would else carry over.
        var noSignals = sigset_t(), allSignals = sigset_t()
        sigemptyset(&noSignals)
        sigfillset(&allSignals)
        posix_spawnattr_setsigmask(&attributes, &noSignals)
        posix_spawnattr_setsigdefault(&attributes, &allSignals)
        posix_spawnattr_setflags(&attributes, Int16(POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT
                                                    | POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSIGDEF))
        posix_spawnattr_setpgroup(&attributes, 0)

        var environment = ProcessInfo.processInfo.environment
        for (key, value) in env { environment[key] = value }
        let cArgv: [UnsafeMutablePointer<CChar>?] = argv.map { strdup($0) } + [nil]
        let cEnv: [UnsafeMutablePointer<CChar>?] = environment.map { strdup("\($0.key)=\($0.value)") } + [nil]
        defer {
            cArgv.forEach { free($0) }
            cEnv.forEach { free($0) }
        }

        var pid: pid_t = 0
        let spawned = posix_spawn(&pid, exe, &actions, &attributes, cArgv, cEnv)
        close(outFDs[1]); close(errFDs[1])
        if devNull >= 0 { close(devNull) }
        if inFDs[0] >= 0 { close(inFDs[0]) }
        guard spawned == 0 else {
            close(outFDs[0]); close(errFDs[0])
            if inFDs[1] >= 0 { close(inFDs[1]) }
            return fail(String(cString: strerror(spawned)))
        }

        let finished = Flag()
        let queue = DispatchQueue.global(qos: .utility)
        // The write has a queue of its own: the child may be writing a full stdout pipe while we write its stdin.
        if let stdin {
            let fd = inFDs[1]
            DispatchQueue(label: "notchling.runner.stdin", qos: .utility).async {
                writeAll(stdin, to: fd, stop: finished)
            }
        }

        let out = Sink(), err = Sink()
        let stream = Relay(onOutput)
        let readers = DispatchGroup()
        for (fd, sink, channel) in [(outFDs[0], out, OutputChannel.stdout), (errFDs[0], err, OutputChannel.stderr)] {
            queue.async(group: readers) {
                var buffer = [UInt8](repeating: 0, count: 65536)
                var held = Data()                                    // the start of a character cut by the read
                while true {
                    let n = read(fd, &buffer, buffer.count)
                    if n > 0 {
                        let chunk = Data(buffer[0..<n])
                        sink.append(chunk)
                        held.append(chunk)
                        let keep = incompleteTail(held)
                        stream.send(String(decoding: held.dropLast(keep), as: UTF8.self), from: channel)
                        held = Data(held.suffix(keep))
                    }
                    else if n < 0 && errno == EINTR { continue }
                    else { break }
                }
                stream.send(String(decoding: held, as: UTF8.self), from: channel)
                close(fd)
            }
        }

        let status = Exit()
        let exited = DispatchSemaphore(value: 0)
        queue.async {
            var raw: Int32 = 0
            while true {
                if waitpid(pid, &raw, 0) != -1 { status.set(raw); break }
                if errno == EINTR { continue }
                status.lose()                                        // ECHILD: something else reaped it
                break
            }
            exited.signal()
        }

        var timedOut = false
        if exited.wait(timeout: .now() + timeout) == .timedOut {
            timedOut = true
            endGroup(pid)
            _ = exited.wait(timeout: .now() + 5)
        } else {
            // what the command left running must not outlive it
            endGroup(pid)
        }
        _ = readers.wait(timeout: .now() + drainGrace)
        finished.set()
        stream.close()
        var errText = err.text
        if status.lost && !timedOut {
            errText += (errText.isEmpty || errText.hasSuffix("\n") ? "" : "\n") + "lost the exit status of \(exe)"
        }
        return CommandResult(code: timedOut ? 124 : status.lost ? lostStatus : status.code, out: out.text, err: errText)
    }
}

/// How Notchling reaches a worker machine over Tailscale SSH.
enum SSH {
    /// `ssh` with no prompts and a short connect timeout, running `remote` on the machine. The remote side
    /// reads one string, so each word is quoted: a path with a space stays one argument.
    static func argv(_ m: Machine, _ remote: [String]) -> [String] {
        ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--", "\(m.user)@\(m.host)",
         remote.map(LaneStore.quote).joined(separator: " ")]
    }

    /// A shell script run by `/bin/sh` on the machine (its login shell may be anything) with the service PATH.
    /// The script's own stdin is ssh's stdin: a value to store goes there, never into the script.
    static func script(_ m: Machine, _ script: String) -> [String] {
        ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--", "\(m.user)@\(m.host)",
         "/bin/sh -c " + LaneStore.quote(RemoteShell.prelude + script)]
    }

    /// A script that needs admin rights. Through sudo, the password is the first line of ssh's stdin (`sudo -S`
    /// reads it); the script itself reads no stdin, so a line sudo did not take (NOPASSWD) reaches no command.
    /// As root over ssh there is no sudo at all.
    static func admin(_ m: Machine, _ script: String, asRoot: Bool) -> [String] {
        let user = asRoot ? "root" : m.user
        let body = "exec </dev/null; " + script
        let remote = asRoot
            ? RemoteShell.prelude + body
            : RemoteShell.prelude + "sudo -S -k -p '' /bin/sh -c " + LaneStore.quote(body)
        return ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--", "\(user)@\(m.host)",
                "/bin/sh -c " + LaneStore.quote(remote)]
    }

    /// For a `git fetch` over ssh: no prompts (a prompt would wait for the whole fetch timeout) and a short connect timeout.
    static let gitEnvironment = ["GIT_SSH_COMMAND": "ssh -o BatchMode=yes -o ConnectTimeout=8"]

    static func notAbsolute(_ path: String) -> String { "the machine's worktree path \"\(path)\" is not an absolute path" }

    /// Where `git` reaches a path on the machine, or nil for a path that is not absolute: `.evil.com/x` would
    /// otherwise make the host `m.host.evil.com`.
    static func gitURL(_ m: Machine, path: String) -> String? {
        guard path.hasPrefix("/") else { return nil }
        return "ssh://\(m.user)@\(m.host)\(path)"
    }
}

/// The mode and size of a file on a machine; its content is never read.
struct FileFacts: Equatable {
    let mode: String
    let size: Int
}

/// Strings for the shell on a machine. Everything a machine reports is untrusted: only a value that passed
/// `isSafeMachinePath` may enter a script or a file.
enum RemoteShell {
    static let pathValue = "$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    /// Starts every remote script: a non-interactive ssh command gets a short PATH, without mise or cargo.
    static let prelude = "export PATH=\"\(pathValue)\"; "

    /// The PATH for a service unit, which has no `$HOME` to expand.
    static func servicePath(home: String) -> String {
        pathValue.replacingOccurrences(of: "$HOME", with: home)
    }

    /// `~` and `~/rest` expand on the machine (`$HOME` in double quotes, the rest single-quoted); any other path
    /// is one single-quoted word, so `~other/x` stays literal.
    static func path(_ p: String) -> String {
        if p == "~" { return "\"$HOME\"" }
        if p.hasPrefix("~/") { return "\"$HOME\"/" + LaneStore.quote(String(p.dropFirst(2))) }
        return LaneStore.quote(p)
    }

    /// Whether a path a machine reported may go into a script or a file: absolute, plain characters only, no `..`.
    static func isSafeMachinePath(_ p: String) -> Bool {
        guard p.hasPrefix("/"), p.utf8.count <= 512 else { return false }
        let allowed = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-/+@")
        guard p.allSatisfy({ allowed.contains($0) }) else { return false }
        return !p.split(separator: "/", omittingEmptySubsequences: false).contains("..")
    }

    /// Reads stdin into `p` with mode `mode` (600 unless told), through `p.new` so a half-written file never
    /// sits at `p`. The umask makes the new file private from its first byte.
    static func writeFile(_ p: String, mode: String = "600") -> String {
        let dir = (p as NSString).deletingLastPathComponent
        let temp = path(p + ".new")
        return "umask 077; mkdir -p \(path(dir)) && cat > \(temp) && chmod \(mode) \(temp) && mv -f \(temp) \(path(p))"
    }

    /// Prints one line per path: `mode size`, or `missing`.
    static func stat(_ paths: [String], linux: Bool) -> String {
        let format = linux ? "stat -c '%a %s'" : "stat -f '%Lp %z'"
        return paths.map { "if [ -f \(path($0)) ]; then \(format) \(path($0)); else echo missing; fi" }.joined(separator: "; ")
    }

    /// The facts for each of `count` paths, nil for a missing file or a line that is not `mode size`.
    static func parseStat(_ out: String, count: Int) -> [FileFacts?] {
        let lines = out.split(separator: "\n", omittingEmptySubsequences: false)
        return (0..<count).map { i in
            guard i < lines.count else { return nil }
            let words = lines[i].split(separator: " ", omittingEmptySubsequences: true)
            guard words.count == 2, (1...4).contains(words[0].count), words[0].allSatisfy({ $0 >= "0" && $0 <= "7" }),
                  let size = Int(words[1]), size >= 0 else { return nil }
            return FileFacts(mode: String(words[0]), size: size)
        }
    }
}
