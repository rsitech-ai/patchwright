import Foundation
import XCTest

final class SettingsViewTests: XCTestCase {
    func testSettingsDoNotExposeAnUnwiredReviewProviderPreference() throws {
        let repositoryRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let source = try String(
            contentsOf: repositoryRoot.appending(path: "Sources/PatchwrightApp/Views/SettingsView.swift"),
            encoding: .utf8
        )

        XCTAssertFalse(source.contains("reviewProvider"))
        XCTAssertFalse(source.contains("Review provider"))
        XCTAssertFalse(source.contains("Apple Foundation Models"))
    }
}
