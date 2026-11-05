import AppKit
import ServiceManagement

// For agents: run a command with Secret Vault values in its environment, or list what's saved. First, so a
// `--hook` or `--page` inside the wrapped command isn't taken for ours.
if CommandLine.arguments.count > 1 {
    if CommandLine.arguments[1] == "--secret" { SecretVault.runCLI(CommandLine.arguments) }
    if CommandLine.arguments[1] == "--secrets" { SecretVault.runList() }
}

// Run by Claude Code as a hook: hand the prompt to the running app and wait for a decision.
if CommandLine.arguments.contains("--hook") { HookBridge.runHook() }

// For agents: find saved Artifact pages by link, id or title words, print where they are, and exit.
if let i = CommandLine.arguments.firstIndex(where: { $0 == "--page" || $0 == "--pages" }) {
    let rest = CommandLine.arguments.dropFirst(i + 1).joined(separator: " ")
    PageLibrary.runCLI(rest, list: CommandLine.arguments[i] == "--pages")
}

// Scriptable equivalents of the "Launch at Login" menu item: register/unregister and exit.
let args = CommandLine.arguments
if args.contains("--enable-login-item") || args.contains("--disable-login-item") {
    do {
        if args.contains("--enable-login-item") {
            try SMAppService.mainApp.register()
        } else {
            try SMAppService.mainApp.unregister()
        }
        print("Launch at Login: \(SMAppService.mainApp.status == .enabled ? "enabled" : "disabled")")
        exit(0)
    } catch {
        FileHandle.standardError.write("Failed: \(error.localizedDescription)\n".data(using: .utf8)!)
        exit(1)
    }
}

AppDelegate.migrateFromAwake()
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
