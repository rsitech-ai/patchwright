import Foundation
import XCTest

final class PreparationPresentationTests: XCTestCase {
    func testTaskPreviewCopySeparatesLocalReviewFromGitHubMutationAccess() throws {
        let source = try String(
            contentsOf: packageRoot.appending(path: "Sources/PatchwrightApp/Views/GitHubRepositoryView.swift"),
            encoding: .utf8
        )

        XCTAssertTrue(source.contains("Preview the local task contract"))
        XCTAssertTrue(source.contains("GitHub App access is required only for remote mutations"))
        XCTAssertFalse(source.contains("Verify GitHub App access and preview the task contract"))
    }

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
