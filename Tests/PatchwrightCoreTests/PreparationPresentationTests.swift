import Foundation
import XCTest

final class PreparationPresentationTests: XCTestCase {
    func testTaskDetailPresentsExactPreparationReviewBeforeApproval() throws {
        let taskDetailSource = try String(
            contentsOf: packageRoot.appending(path: "Sources/PatchwrightApp/Views/TaskDetailView.swift"),
            encoding: .utf8
        )
        let preparationSource = try String(
            contentsOf: packageRoot.appending(path: "Sources/PatchwrightApp/Views/PreparationApprovalSheet.swift"),
            encoding: .utf8
        )

        XCTAssertTrue(taskDetailSource.contains("Review Worktree Preparation"))
        XCTAssertTrue(taskDetailSource.contains(".sheet(item: $preparationApprovalRequest)"))
        XCTAssertTrue(taskDetailSource.contains("PreparationApprovalSheet(store: store, preview: preview)"))
        XCTAssertTrue(taskDetailSource.contains("contract.goal"))
        XCTAssertTrue(taskDetailSource.contains("contract.acceptanceCriteria"))
        XCTAssertTrue(taskDetailSource.contains("contract.verificationCommands"))
        XCTAssertTrue(taskDetailSource.contains("contract.sensitivePaths"))
        XCTAssertFalse(taskDetailSource.contains("store.prepareTask(taskID:"))

        XCTAssertTrue(preparationSource.contains("Goal"))
        XCTAssertTrue(preparationSource.contains("Acceptance criteria"))
        XCTAssertTrue(preparationSource.contains("Exact verification commands"))
        XCTAssertTrue(preparationSource.contains("Risk"))
        XCTAssertTrue(preparationSource.contains("Sensitive paths"))
    }

    private var packageRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}
