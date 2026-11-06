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

    func testAdminThroughSudoReadsThePasswordFromStdin() {
        let argv = SSH.admin(machine, "apt-get install -y git", asRoot: false)
        XCTAssertEqual(argv[argv.count - 2], "agent@vps.tail")
        // the brief's quoting turns the empty prompt '' into '\'''\'' in the outer word, so the exact string below is the check
        XCTAssertTrue(argv.last!.contains("sudo -S -k -p "), argv.last!)
        XCTAssertTrue(argv.last!.contains("exec </dev/null; apt-get"), argv.last!)
        XCTAssertEqual(argv.last, "/bin/sh -c " + LaneStore.quote(RemoteShell.prelude + "sudo -S -k -p '' /bin/sh -c "
                       + LaneStore.quote("exec </dev/null; apt-get install -y git")))
    }

    func testAdminAsRootOverSSH() {
        let argv = SSH.admin(machine, "apt-get update", asRoot: true)
        XCTAssertEqual(argv[argv.count - 2], "root@vps.tail")
        XCTAssertFalse(argv.last!.contains("sudo"))
        XCTAssertEqual(argv.last, "/bin/sh -c " + LaneStore.quote(RemoteShell.prelude + "exec </dev/null; apt-get update"))
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
