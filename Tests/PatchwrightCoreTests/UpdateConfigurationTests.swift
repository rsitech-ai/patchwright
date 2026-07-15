import Foundation
import XCTest

final class UpdateConfigurationTests: XCTestCase {
    private let expectedFeedURL = "https://github.com/s1korrrr/patchwright/releases/latest/download/appcast.xml"
    private let expectedSparkleRevision = "6276ba2b404829d139c45ff98427cf90e2efc59b"

    func testSparkleIsPinnedExactlyAndLinkedForAnAppBundle() throws {
        let manifest = try source("Package.swift")

        XCTAssertTrue(manifest.contains(#".package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.9.2")"#))
        XCTAssertTrue(manifest.contains(#".product(name: "Sparkle", package: "Sparkle")"#))
        XCTAssertTrue(manifest.contains(#""@executable_path/../Frameworks""#))

        let resolvedData = try Data(contentsOf: root.appendingPathComponent("Package.resolved"))
        let resolved = try JSONSerialization.jsonObject(with: resolvedData) as? [String: Any]
        let pins = try XCTUnwrap(resolved?["pins"] as? [[String: Any]])
        let sparkle = try XCTUnwrap(pins.first { $0["identity"] as? String == "sparkle" })
        let state = try XCTUnwrap(sparkle["state"] as? [String: Any])
        XCTAssertEqual(state["version"] as? String, "2.9.2")
        XCTAssertEqual(state["revision"] as? String, expectedSparkleRevision)
    }

    func testUpdateFeedRequiresSignedMetadataAndPayloads() throws {
        let plistURL = root.appendingPathComponent("Packaging/Info.plist")
        let plistData = try Data(contentsOf: plistURL)
        let plist = try XCTUnwrap(
            PropertyListSerialization.propertyList(from: plistData, format: nil) as? [String: Any]
        )

        XCTAssertEqual(plist["SUFeedURL"] as? String, expectedFeedURL)
        XCTAssertEqual(plist["SUVerifyUpdateBeforeExtraction"] as? Bool, true)
        XCTAssertEqual(plist["SURequireSignedFeed"] as? Bool, true)

        let encodedKey = try XCTUnwrap(plist["SUPublicEDKey"] as? String)
        let key = try XCTUnwrap(Data(base64Encoded: encodedKey))
        XCTAssertEqual(key.count, 32)
    }

    func testAppOwnsOneInjectableUpdaterAndExposesTheUpdateCommand() throws {
        let controller = try source("Sources/PatchwrightApp/Services/UpdateController.swift")
        let app = try source("Sources/PatchwrightApp/App/PatchwrightApp.swift")
        let commands = try source("Sources/PatchwrightApp/Support/AppCommands.swift")

        XCTAssertTrue(controller.contains("final class UpdateController: ObservableObject"))
        XCTAssertTrue(controller.contains("init(startingUpdater: Bool = true)"))
        XCTAssertTrue(controller.contains("startingUpdater: startingUpdater"))
        XCTAssertEqual(controller.components(separatedBy: "SPUStandardUpdaterController(").count - 1, 1)

        XCTAssertTrue(app.contains("@StateObject private var updateController: UpdateController"))
        XCTAssertEqual(app.components(separatedBy: "UpdateController(").count - 1, 1)
        XCTAssertTrue(app.contains("PatchwrightCommands(store: store, updateController: updateController)"))

        XCTAssertTrue(commands.contains(#"Button("Check for Updates…")"#))
        XCTAssertTrue(commands.contains("updateController.checkForUpdates()"))
        XCTAssertTrue(commands.contains(".disabled(!updateController.canCheckForUpdates)"))
    }

    private var root: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    private func source(_ relativePath: String) throws -> String {
        try String(contentsOf: root.appendingPathComponent(relativePath), encoding: .utf8)
    }
}
