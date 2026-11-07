// swift-tools-version:5.9
// Used for `swift test` and editor support. The app bundle itself is built by build.sh,
// which compiles the same files with swiftc for both architectures. The Swift app lives in legacy/macos/.
import PackageDescription

let package = Package(
    name: "Notchling",
    platforms: [.macOS(.v13)],
    targets: [
        // The phone bridge's engine (sessions, transcripts, pairing, TLS/WebSocket protocol) and the session
        // host client. No dependencies; Notchling and the helper import it.
        .target(name: "ConsoleCore", path: "legacy/macos/Sources/ConsoleCore"),
        .executableTarget(
            name: "Notchling",
            dependencies: ["ConsoleCore"],
            path: "legacy/macos/Sources/Notchling"
        ),
        // The session host: owns the terminals phones and Chat type into, outlives the app.
        .executableTarget(
            name: "hostd",
            dependencies: ["ConsoleCore"],
            path: "legacy/macos/Sources/hostd"
        ),
        .testTarget(
            name: "NotchlingTests",
            dependencies: ["Notchling"],
            path: "legacy/macos/Tests/NotchlingTests",
            resources: [.copy("Fixtures")]
        ),
        .testTarget(
            name: "ConsoleCoreTests",
            dependencies: ["ConsoleCore"],
            path: "legacy/macos/Tests/ConsoleCoreTests",
            resources: [.copy("Fixtures")]
        ),
    ]
)
