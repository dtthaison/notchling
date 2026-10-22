import AppKit
import ServiceManagement

// Run by Claude Code as a hook: hand the prompt to the running app and wait for a decision.
if CommandLine.arguments.contains("--hook") { HookBridge.runHook() }

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
