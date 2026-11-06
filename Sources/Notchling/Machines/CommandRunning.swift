import Foundation

struct CommandResult: Equatable {
    let code: Int32
    let out: String
    let err: String
}

/// Runs one command and waits for it. Injectable so tests never start ssh.
protocol CommandRunning {
    /// `argv[0]` is the executable's path. A timeout returns code 124, a missing executable code 127.
    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult

    /// The same with `env` laid over this app's own environment (a key in `env` wins). A runner that cannot
    /// pass an environment on (a test double) falls back to the plain call.
    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult
}

extension CommandRunning {
    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, timeout: timeout)
    }
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

    func run(_ argv: [String], cwd: String? = nil, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], timeout: timeout)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                continuation.resume(returning: Self.runBlocking(argv, cwd: cwd, env: env, timeout: timeout))
            }
        }
    }

    /// Bytes read from one pipe, safe to read while the reader is still running.
    private final class Sink: @unchecked Sendable {
        private let lock = NSLock()
        private var data = Data()
        func append(_ chunk: Data) { lock.withLock { data.append(chunk) } }
        var text: String { lock.withLock { String(decoding: data, as: UTF8.self) } }
    }

    /// The exit status of the child, set by the thread that reaps it.
    private final class Exit: @unchecked Sendable {
        private let lock = NSLock()
        private var value: Int32 = 0
        func set(_ status: Int32) { lock.withLock { value = status } }
        var code: Int32 {
            lock.withLock {
                // a normal exit gives its code; death by a signal gives the signal number
                value & 0x7f == 0 ? (value >> 8) & 0xff : value & 0x7f
            }
        }
    }

    private static func fail(_ message: String) -> CommandResult { CommandResult(code: 127, out: "", err: message) }

    private static func runBlocking(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) -> CommandResult {
        guard let exe = argv.first, FileManager.default.isExecutableFile(atPath: exe) else {
            return fail("\(argv.first ?? "") not found")
        }

        var outFDs: [Int32] = [-1, -1], errFDs: [Int32] = [-1, -1]
        guard pipe(&outFDs) == 0 else { return fail("cannot make a pipe: \(String(cString: strerror(errno)))") }
        guard pipe(&errFDs) == 0 else {
            outFDs.forEach { close($0) }
            return fail("cannot make a pipe: \(String(cString: strerror(errno)))")
        }
        let devNull = open("/dev/null", O_RDONLY)
        // Ours only: the child gets its own copies through the file actions below.
        for fd in outFDs + errFDs + [devNull] where fd >= 0 { _ = fcntl(fd, F_SETFD, FD_CLOEXEC) }

        var actions: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&actions)
        defer { posix_spawn_file_actions_destroy(&actions) }
        posix_spawn_file_actions_adddup2(&actions, devNull, 0)
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
        close(outFDs[1]); close(errFDs[1]); close(devNull)
        guard spawned == 0 else {
            close(outFDs[0]); close(errFDs[0])
            return fail(String(cString: strerror(spawned)))
        }

        let out = Sink(), err = Sink()
        let readers = DispatchGroup()
        let queue = DispatchQueue.global(qos: .utility)
        for (fd, sink) in [(outFDs[0], out), (errFDs[0], err)] {
            queue.async(group: readers) {
                var buffer = [UInt8](repeating: 0, count: 65536)
                while true {
                    let n = read(fd, &buffer, buffer.count)
                    if n > 0 { sink.append(Data(buffer[0..<n])) }
                    else if n < 0 && errno == EINTR { continue }
                    else { break }
                }
                close(fd)
            }
        }

        let status = Exit()
        let exited = DispatchSemaphore(value: 0)
        queue.async {
            var raw: Int32 = 0
            while waitpid(pid, &raw, 0) == -1 && errno == EINTR {}
            status.set(raw)
            exited.signal()
        }

        var timedOut = false
        if exited.wait(timeout: .now() + timeout) == .timedOut {
            timedOut = true
            killpg(pid, SIGTERM)
            let deadline = Date().addingTimeInterval(termGrace)
            // the group is gone once nothing in it is left, the leader included
            while Date() < deadline && killpg(pid, 0) == 0 { usleep(50_000) }
            if killpg(pid, 0) == 0 { killpg(pid, SIGKILL) }
            _ = exited.wait(timeout: .now() + 5)
        }
        _ = readers.wait(timeout: .now() + drainGrace)
        return CommandResult(code: timedOut ? 124 : status.code, out: out.text, err: err.text)
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
