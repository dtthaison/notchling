// swift-tools-version:5.9
// Used for `swift test` and editor support. The app bundle itself is built by build.sh,
// which compiles the same files with swiftc for both architectures.
import PackageDescription

let package = Package(
    name: "Notchling",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "Notchling",
            path: "Sources/Notchling"
        ),
        .testTarget(
            name: "NotchlingTests",
            dependencies: ["Notchling"],
            path: "Tests/NotchlingTests",
            resources: [.copy("Fixtures")]
        ),
    ]
)
