import AppKit

/// Notchling lives in the menu bar (`.accessory`: no Dock icon, not in ⌘Tab). While one of its
/// windows (Chat, Artifacts, Accounts) is open it becomes a regular app, so you can switch away and
/// back with ⌘Tab or the Dock. When the last one closes it goes back to the menu bar only.
enum DockPresence {
    private static var observers: [NSObjectProtocol] = []

    static func start() {
        let center = NotificationCenter.default
        let names: [Notification.Name] = [NSWindow.didBecomeKeyNotification, NSWindow.didBecomeMainNotification,
                                          NSWindow.willCloseNotification, NSWindow.didMiniaturizeNotification]
        for name in names {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { note in
                // A closing window is still visible here; look again once it's gone.
                let closing = name == NSWindow.willCloseNotification ? note.object as? NSWindow : nil
                DispatchQueue.main.async { update(ignoring: closing) }
            })
        }
    }

    /// Our own windows that should keep a Dock icon: titled ones, not the notch panel or alerts.
    private static func appWindows(ignoring closing: NSWindow?) -> [NSWindow] {
        NSApp.windows.filter { window in
            window !== closing && !(window is NSPanel) && window.styleMask.contains(.titled)
                && (window.isVisible || window.isMiniaturized)
        }
    }

    static func update(ignoring closing: NSWindow? = nil) {
        let wantsDock = !appWindows(ignoring: closing).isEmpty
        let policy: NSApplication.ActivationPolicy = wantsDock ? .regular : .accessory
        guard NSApp.activationPolicy() != policy else { return }
        NSApp.setActivationPolicy(policy)
        if wantsDock {
            NSApp.mainMenu = mainMenu()
            NSApp.activate(ignoringOtherApps: true)
        } else {
            NSApp.mainMenu = nil
        }
    }

    /// The menu bar while a window is open: Quit, the edit commands and the window list. No Hide:
    /// it would hide the notch too.
    private static func mainMenu() -> NSMenu {
        let main = NSMenu()

        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit Notchling", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        main.addItem(submenu(appMenu, title: "Notchling"))

        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        let redo = edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        main.addItem(submenu(edit, title: "Edit"))

        let window = NSMenu(title: "Window")
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        main.addItem(submenu(window, title: "Window"))
        NSApp.windowsMenu = window
        return main
    }

    private static func submenu(_ menu: NSMenu, title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = menu
        return item
    }
}
