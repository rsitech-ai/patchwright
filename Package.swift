// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Patchwright",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "PatchwrightCore", targets: ["PatchwrightCore"]),
        .executable(name: "Patchwright", targets: ["PatchwrightApp"]),
    ],
    dependencies: [
        .package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.9.2"),
    ],
    targets: [
        .target(name: "PatchwrightCore"),
        .executableTarget(
            name: "PatchwrightApp",
            dependencies: [
                "PatchwrightCore",
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            linkerSettings: [
                .unsafeFlags([
                    "-Xlinker", "-rpath",
                    "-Xlinker", "@executable_path/../Frameworks",
                ]),
            ]
        ),
        .testTarget(name: "PatchwrightCoreTests", dependencies: ["PatchwrightCore"]),
    ]
)
