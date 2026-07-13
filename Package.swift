// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Patchwright",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "PatchwrightCore", targets: ["PatchwrightCore"]),
        .executable(name: "Patchwright", targets: ["PatchwrightApp"]),
    ],
    targets: [
        .target(name: "PatchwrightCore"),
        .executableTarget(name: "PatchwrightApp", dependencies: ["PatchwrightCore"]),
        .testTarget(name: "PatchwrightCoreTests", dependencies: ["PatchwrightCore"]),
    ]
)

