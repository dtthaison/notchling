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
}

/// Runs a `Process`, reading both pipes on background queues so a full pipe never stops the command.
struct ProcessRunner: CommandRunning {
    func run(_ argv: [String], cwd: String? = nil, timeout: TimeInterval) async -> CommandResult {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                continuation.resume(returning: Self.runBlocking(argv, cwd: cwd, timeout: timeout))
            }
        }
    }

    private static func runBlocking(_ argv: [String], cwd: String?, timeout: TimeInterval) -> CommandResult {
        guard let exe = argv.first, FileManager.default.isExecutableFile(atPath: exe) else {
            return CommandResult(code: 127, out: "", err: "\(argv.first ?? "") not found")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: exe)
        process.arguments = Array(argv.dropFirst())
        if let cwd { process.currentDirectoryURL = URL(fileURLWithPath: cwd) }
        let outPipe = Pipe(), errPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = errPipe
        process.standardInput = FileHandle.nullDevice
        let exited = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in exited.signal() }
        do { try process.run() } catch {
            return CommandResult(code: 127, out: "", err: error.localizedDescription)
        }

        var out = Data(), err = Data()
        let readers = DispatchGroup()
        let queue = DispatchQueue.global(qos: .utility)
        queue.async(group: readers) { out = outPipe.fileHandleForReading.readDataToEndOfFile() }
        queue.async(group: readers) { err = errPipe.fileHandleForReading.readDataToEndOfFile() }

        var timedOut = false
        if exited.wait(timeout: .now() + timeout) == .timedOut {
            timedOut = true
            process.terminate()
            if exited.wait(timeout: .now() + 2) == .timedOut {
                kill(process.processIdentifier, SIGKILL)
                exited.wait()
            }
        }
        readers.wait()
        let text = { (d: Data) in String(decoding: d, as: UTF8.self) }
        return CommandResult(code: timedOut ? 124 : process.terminationStatus, out: text(out), err: text(err))
    }
}

/// How Notchling reaches a worker machine over Tailscale SSH.
enum SSH {
    /// `ssh` with no prompts and a short connect timeout, running `remote` on the machine. The remote side
    /// reads one string, so each word is quoted: a path with a space stays one argument.
    static func argv(_ m: Machine, _ remote: [String]) -> [String] {
        ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "\(m.user)@\(m.host)",
         remote.map(LaneStore.quote).joined(separator: " ")]
    }

    /// Where `git` reaches a path on the machine.
    static func gitURL(_ m: Machine, path: String) -> String {
        "ssh://\(m.user)@\(m.host)\(path)"
    }
}
