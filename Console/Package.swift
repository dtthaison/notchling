// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "AgentConsole",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "ConsoleCore", targets: ["ConsoleCore"]),
        .executable(name: "AgentConsole", targets: ["AgentConsole"]),
        .executable(name: "agent-console-hostd", targets: ["hostd"]),
    ],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", from: "1.99.2"),
    ],
    targets: [
        .target(name: "ConsoleCore", swiftSettings: [.swiftLanguageMode(.v5)]),
        .executableTarget(
            name: "AgentConsole",
            dependencies: ["ConsoleCore", .product(name: "SwiftTerm", package: "SwiftTerm")],
            exclude: ["Resources"],
            swiftSettings: [.swiftLanguageMode(.v5)]),
        .executableTarget(
            name: "hostd",
            dependencies: ["ConsoleCore"],
            swiftSettings: [.swiftLanguageMode(.v5)]),
        .testTarget(
            name: "ConsoleCoreTests",
            dependencies: ["ConsoleCore"],
            resources: [.copy("Fixtures")],
            swiftSettings: [.swiftLanguageMode(.v5)]),
    ])
