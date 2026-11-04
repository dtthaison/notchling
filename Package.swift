// swift-tools-version:5.9
// Used for `swift test` and editor support. The app bundle itself is built by build.sh,
// which compiles the same files with swiftc for both architectures.
import PackageDescription

let package = Package(
    name: "Notchling",
    platforms: [.macOS(.v13)],
    targets: [
        // The phone bridge's engine (sessions, transcripts, pairing, TLS/WebSocket protocol) and the session
        // host client. No dependencies; Notchling and the helper import it.
        .target(name: "ConsoleCore", path: "Sources/ConsoleCore"),
        .executableTarget(
            name: "Notchling",
            dependencies: ["ConsoleCore"],
            path: "Sources/Notchling"
        ),
        // The session host: owns the terminals phones and Chat type into, outlives the app.
        .executableTarget(
            name: "hostd",
            dependencies: ["ConsoleCore"],
            path: "Sources/hostd"
        ),
        .testTarget(
            name: "NotchlingTests",
            dependencies: ["Notchling"],
            path: "Tests/NotchlingTests",
            resources: [.copy("Fixtures")]
        ),
        .testTarget(
            name: "ConsoleCoreTests",
            dependencies: ["ConsoleCore"],
            path: "Tests/ConsoleCoreTests",
            resources: [.copy("Fixtures")]
        ),
    ]
)
