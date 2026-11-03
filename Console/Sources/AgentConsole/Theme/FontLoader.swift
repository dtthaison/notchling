import AppKit
import CoreText
import ConsoleCore

/// Geist and Geist Mono ship inside the app (OFL, see Resources/Fonts/OFL.txt). Registered at launch,
/// so nothing depends on the user having them installed.
enum FontLoader {
    static let files = ["Geist-Regular", "Geist-Medium", "Geist-SemiBold", "GeistMono-Regular", "GeistMono-Medium"]

    @discardableResult
    static func registerAll() -> Bool {
        var allRegistered = true
        for file in files {
            guard let url = locate(file) else {
                ConsoleLog.error("FontLoader.registerAll", file, "font file not found in the app bundle")
                allRegistered = false
                continue
            }
            var error: Unmanaged<CFError>?
            if !CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) {
                let reason = error?.takeRetainedValue().localizedDescription ?? "unknown"
                // Registering twice is harmless; anything else is worth a line.
                ConsoleLog.error("FontLoader.registerAll", url.path, "register failed: \(reason)")
                allRegistered = false
            }
        }
        if NSFont(name: "Geist-Regular", size: 13) == nil {
            ConsoleLog.error("FontLoader.registerAll", "Geist-Regular", "font not available after registration; falling back to system")
            allRegistered = false
        }
        return allRegistered
    }

    /// Inside the .app: Contents/Resources/Fonts. From `swift run`: the source folder next to this file.
    private static func locate(_ name: String) -> URL? {
        if let url = Bundle.main.url(forResource: name, withExtension: "ttf", subdirectory: "Fonts") { return url }
        let source = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Resources/Fonts/\(name).ttf")
        return FileManager.default.fileExists(atPath: source.path) ? source : nil
    }
}
