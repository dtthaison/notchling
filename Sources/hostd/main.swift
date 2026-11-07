import ConsoleCore
import Darwin
import Foundation

// agent-console-hostd: owns the ptys and claude processes of Agent Console, so restarting or reinstalling the app
// never ends a session. Started by launchd (local.agentconsole.hostd).
//
//   agent-console-hostd                      run the host
//   agent-console-hostd --status             print {"running":N,"total":M} and exit (asks the running host)
//   agent-console-hostd --socket PATH --state PATH     (tests: other paths)

func argument(_ name: String) -> String? {
    guard let index = CommandLine.arguments.firstIndex(of: name), index + 1 < CommandLine.arguments.count else { return nil }
    return CommandLine.arguments[index + 1]
}

let support = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/AgentConsole")
let socketPath = argument("--socket") ?? TerminalBackends.defaultSocketPath
let statePath = argument("--state") ?? support.appendingPathComponent("hostd-state.json").path

if CommandLine.arguments.contains("--status") {
    do {
        let client = try HostClient.connect(socketPath: socketPath, timeout: 2)
        let reply = try client.request("status")
        print("{\"running\":\(reply["running"]?.int ?? 0),\"total\":\(reply["total"]?.int ?? 0)}")
        exit(0)
    } catch {
        // No host answering: nothing is running.
        print("{\"running\":0,\"total\":0,\"host\":false}")
        exit(0)
    }
}

// Migration: remember a session that is running somewhere else right now, so the next host start resumes it.
//   agent-console-hostd --record-resume SESSION_ID CWD
if let index = CommandLine.arguments.firstIndex(of: "--record-resume"), index + 2 < CommandLine.arguments.count {
    let store = HostStateStore(url: URL(fileURLWithPath: statePath))
    store.upsert(HostRecord(id: UUID().uuidString, cwd: CommandLine.arguments[index + 2], model: nil,
                            claudeSessionId: CommandLine.arguments[index + 1], pid: 0, startedAt: Date()))
    print("recorded \(CommandLine.arguments[index + 1]) for resume")
    exit(0)
}

setvbuf(stdout, nil, _IOLBF, 0)
let paths = ClaudePaths.live
let state = HostStateStore(url: URL(fileURLWithPath: statePath))
let host = SessionHost(spawner: PTYHostSpawner(), state: state,
                       discoverSessionId: { SessionPidMapper.sessionId(forRootPid: $0, sessionsDir: paths.sessionsDir) })
let server = HostServer(host: host, socketPath: socketPath)

do { try server.start() } catch {
    print("hostd: cannot start: \(error.localizedDescription)")
    exit(1)
}
print("hostd: listening on \(socketPath)")

// Bring back sessions that were recorded and whose process is gone (a reboot, a crash, an update).
let resumed = host.resumeRecorded()
if resumed > 0 { print("hostd: resumed \(resumed) session(s)") }

signal(SIGPIPE, SIG_IGN)
// SIGTERM from launchd: stop listening and leave. The ptys end with the host, and the next start resumes them.
let terminate = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
signal(SIGTERM, SIG_IGN)
terminate.setEventHandler { server.stop(); exit(0) }
terminate.resume()

// An update was installed next to the running binary: take it over as soon as nothing is running.
let binary = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().path
func stamp() -> String {
    let attributes = try? FileManager.default.attributesOfItem(atPath: binary)
    return "\((attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0)-\((attributes?[.size] as? NSNumber)?.intValue ?? 0)"
}
let startedStamp = stamp()
let idleCheck = DispatchSource.makeTimerSource(queue: .main)
idleCheck.schedule(deadline: .now() + 30, repeating: 30)
idleCheck.setEventHandler {
    switch HostdUpdatePolicy.decide(binaryChanged: stamp() != startedStamp, runningSessions: host.runningCount) {
    case .restartNow:
        print("hostd: a newer helper is installed and nothing is running; restarting")
        server.stop(); exit(0)   // launchd KeepAlive starts the new binary
    case .deferUntilIdle, .nothing:
        break
    }
}
idleCheck.resume()
dispatchMain()
