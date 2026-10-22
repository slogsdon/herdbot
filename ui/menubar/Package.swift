// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "HerdbotMenu",
    platforms: [.macOS(.v15)],
    targets: [
        .target(name: "HerdbotKit"),
        .executableTarget(name: "HerdbotMenu", dependencies: ["HerdbotKit"]),
        .testTarget(name: "HerdbotKitTests", dependencies: ["HerdbotKit"]),
    ]
)
