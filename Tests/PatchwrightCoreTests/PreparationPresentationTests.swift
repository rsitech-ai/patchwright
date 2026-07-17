import Foundation
import XCTest

final class PreparationPresentationTests: XCTestCase {
    func testTaskDetailPresentsExactPreparationReviewBeforeApproval() throws {
        let source = try String(
            contentsOf: packageRoot.appending(path: "Sources/PatchwrightApp/Views/TaskDetailView.swift"),
            encoding: .utf8
        )

        XCTAssertTrue(source.contains("Review Worktree Preparation"))
        XCTAssertTrue(source.contains(".sheet(item: $preparationApprovalRequest)"))
        XCTAssertTrue(source.contains("PreparationApprovalSheet(store: store, preview: preview)"))
        XCTAssertFalse(source.contains("store.prepareTask(taskID:"))
    }

    private var packageRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}
