import XCTest
@testable import Notchling

/// Commands that open a menu go to a Terminal; the rest are typed into the hidden attach.
final class MenuCommandTests: XCTestCase {
    func testMenuCommands() {
        XCTAssertTrue(SlashCommands.opensMenu("/plugin"))
        XCTAssertTrue(SlashCommands.opensMenu("  /MCP "))
        XCTAssertTrue(SlashCommands.opensMenu("/plugin install pstack@cursor-plugins"))
        XCTAssertTrue(SlashCommands.opensMenu("/effort"))
    }

    func testCommandsWithOutputOrAValueStayInChat() {
        XCTAssertFalse(SlashCommands.opensMenu("/effort high"))
        XCTAssertFalse(SlashCommands.opensMenu("/model claude-opus-5-5"))
        XCTAssertFalse(SlashCommands.opensMenu("/context"))
        XCTAssertFalse(SlashCommands.opensMenu("/pstack:poteto-mode fix the scroll"))
        XCTAssertFalse(SlashCommands.opensMenu("/plugins"))
        XCTAssertFalse(SlashCommands.opensMenu("please run /plugin"))
        XCTAssertFalse(SlashCommands.opensMenu("/"))
    }
}

/// Which plugins' commands a session's folder offers, from user, project and local settings.
final class EnabledPluginsTests: XCTestCase {
    var home: URL!
    var project: URL!

    override func setUpWithError() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("slash-\(UUID().uuidString)")
        home = root.appendingPathComponent("home")
        project = root.appendingPathComponent("proj")
        try FileManager.default.createDirectory(at: home.appendingPathComponent(".claude/plugins"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: project.appendingPathComponent(".claude"), withIntermediateDirectories: true)
        write(["plugins": [
            "sd@m": [["scope": "user", "installPath": "/cache/sd"]],
            "off@m": [["scope": "user", "installPath": "/cache/off"]],
            "pstack@m": [
                ["scope": "project", "projectPath": "/elsewhere", "installPath": "/cache/pstack-old"],
                ["scope": "project", "projectPath": project.path, "installPath": "/cache/pstack"],
            ],
        ]], to: home.appendingPathComponent(".claude/plugins/installed_plugins.json"))
        write(["enabledPlugins": ["sd@m": true, "off@m": true]], to: home.appendingPathComponent(".claude/settings.json"))
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: home.deletingLastPathComponent())
    }

    func write(_ obj: [String: Any], to url: URL) {
        HookBridge.writeJSON(obj, to: url)
    }

    func names(_ cwd: String) -> [String: String] {
        Dictionary(uniqueKeysWithValues: SlashCommands.enabledPlugins(home: home, cwd: cwd).map { ($0.0, $0.1) })
    }

    func testUserSettingsOnly() {
        XCTAssertEqual(names(""), ["sd": "/cache/sd", "off": "/cache/off"])
    }

    func testProjectScopedPluginShowsInItsFolder() {
        write(["enabledPlugins": ["pstack@m": true]], to: project.appendingPathComponent(".claude/settings.json"))
        XCTAssertEqual(names(project.path)["pstack"], "/cache/pstack", "picks the install made for this folder")
        XCTAssertEqual(names(project.path)["sd"], "/cache/sd", "user plugins stay")
    }

    func testLocalSettingsOverrideProjectAndUser() {
        write(["enabledPlugins": ["pstack@m": true]], to: project.appendingPathComponent(".claude/settings.json"))
        write(["enabledPlugins": ["pstack@m": false, "off@m": false]], to: project.appendingPathComponent(".claude/settings.local.json"))
        let found = names(project.path)
        XCTAssertNil(found["pstack"])
        XCTAssertNil(found["off"])
        XCTAssertEqual(found["sd"], "/cache/sd")
    }
}
