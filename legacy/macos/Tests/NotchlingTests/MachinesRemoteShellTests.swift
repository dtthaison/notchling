import XCTest
@testable import Notchling

/// The ssh script and admin argv, the remote path helpers and the stat parser. Pure strings: no ssh starts.
final class MachinesRemoteShellTests: XCTestCase {
    private let machine = Machine(name: "vps", host: "vps.tail", user: "agent", sdPath: "/opt/sd/sd.py", fileVault: false, repos: [])
    private let path = "$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

    func testScriptArgvRunsPosixShWithTheServicePath() {
        XCTAssertEqual(SSH.script(machine, "echo hi"),
                       ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--", "agent@vps.tail",
                        "/bin/sh -c 'export PATH=\"\(path)\"; echo hi'"])
    }

    /// The sudo binary and the PATH of the script it runs are fixed: nothing the worker account owns decides
    /// either (C1). A shell file, a `~/.local/bin/sudo` or a planted `pmset` is never reached.
    private func assertNothingOfTheWorkersOwn(_ argv: [String], file: StaticString = #filePath, line: UInt = #line) {
        let text = argv.joined(separator: "\n")
        for needle in [RemoteShell.prelude, "export PATH", "$HOME", ".local/bin", ".cargo", "mise", "/opt/homebrew", "/usr/local"] {
            XCTAssertFalse(text.contains(needle), "an admin command carries \(needle): \(text)", file: file, line: line)
        }
    }

    func testAdminThroughSudoCallsTheAbsoluteSudoWithAFixedEnvironment() {
        var mac = machine
        mac.os = .macOS
        let argv = SSH.admin(mac, "pmset -a sleep 0", asRoot: false)
        XCTAssertEqual(argv[argv.count - 2], "agent@vps.tail")
        let inner = "/usr/bin/sudo -S -k -p '' /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/var/root /bin/sh -c "
            + LaneStore.quote("exec </dev/null; pmset -a sleep 0")
        XCTAssertEqual(argv.last, "/bin/sh -c " + LaneStore.quote(inner))
        assertNothingOfTheWorkersOwn(argv)
    }

    func testAdminThroughSudoOnLinuxUsesRootsHome() {
        var linux = machine
        linux.os = .linux
        let argv = SSH.admin(linux, "apt-get install -y git", asRoot: false)
        XCTAssertTrue(argv.last!.contains("/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root /bin/sh -c"), argv.last!)
        XCTAssertTrue(argv.last!.contains("/usr/bin/sudo -S -k -p "), argv.last!)
        assertNothingOfTheWorkersOwn(argv)
    }

    func testAdminAsRootOverSSHHasNoSudoAndAFixedEnvironment() {
        var linux = machine
        linux.os = .linux
        let argv = SSH.admin(linux, "apt-get update", asRoot: true)
        XCTAssertEqual(argv[argv.count - 2], "root@vps.tail")
        XCTAssertFalse(argv.last!.contains("sudo"))
        let inner = "/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root /bin/sh -c " + LaneStore.quote("exec </dev/null; apt-get update")
        XCTAssertEqual(argv.last, "/bin/sh -c " + LaneStore.quote(inner))
        assertNothingOfTheWorkersOwn(argv)
    }

    func testRemotePathQuoting() {
        XCTAssertEqual(RemoteShell.path("~/src/my app"), "\"$HOME\"/'src/my app'")
        XCTAssertEqual(RemoteShell.path("~"), "\"$HOME\"")
        XCTAssertEqual(RemoteShell.path("/opt/x y"), "'/opt/x y'")
        XCTAssertEqual(RemoteShell.path("~other/x"), "'~other/x'")
    }

    func testUnsafeMachinePathsAreRefused() {
        XCTAssertTrue(RemoteShell.isSafeMachinePath("/home/agent"))
        XCTAssertTrue(RemoteShell.isSafeMachinePath("/Users/sondoan/.local/bin/python3"))
        for bad in ["/Users/so doan", "/home/a;rm -rf ~", "relative", "/x/../y", "/x\ny", "", "/home/$USER", "/a/b'c"] {
            XCTAssertFalse(RemoteShell.isSafeMachinePath(bad), bad.debugDescription)
        }
    }

    func testWriteFileScript() {
        XCTAssertEqual(RemoteShell.writeFile("~/.config/agents/github-token"),
                       "umask 077; mkdir -p \"$HOME\"/'.config/agents' && cat > \"$HOME\"/'.config/agents/github-token.new' && chmod 600 \"$HOME\"/'.config/agents/github-token.new' && mv -f \"$HOME\"/'.config/agents/github-token.new' \"$HOME\"/'.config/agents/github-token'")
        XCTAssertTrue(RemoteShell.writeFile("/etc/x/y", mode: "644").contains("chmod 644 '/etc/x/y.new'"))
    }

    func testStatScriptAndParse() {
        XCTAssertTrue(RemoteShell.stat(["~/a", "/b"], linux: true).contains("stat -c '%a %s'"))
        XCTAssertTrue(RemoteShell.stat(["~/a"], linux: false).contains("stat -f '%Lp %z'"))
        XCTAssertEqual(RemoteShell.stat(["/b"], linux: true), "if [ -f '/b' ]; then stat -c '%a %s' '/b'; else echo missing; fi")
        XCTAssertEqual(RemoteShell.parseStat("600 41\nmissing\n", count: 2), [FileFacts(mode: "600", size: 41), nil])
        XCTAssertEqual(RemoteShell.parseStat("x", count: 1), [nil])
        XCTAssertEqual(RemoteShell.parseStat("", count: 2), [nil, nil])
    }

    func testServicePathExpandsHome() {
        let p = RemoteShell.servicePath(home: "/home/agent")
        XCTAssertTrue(p.hasPrefix("/home/agent/.local/bin:/home/agent/.cargo/bin:/home/agent/.local/share/mise/shims:/opt/homebrew/bin"))
        XCTAssertFalse(p.contains("$"))
    }
}
