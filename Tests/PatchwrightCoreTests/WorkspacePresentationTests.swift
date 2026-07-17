import Combine
import Foundation
import XCTest
@testable import PatchwrightCore

final class WorkspacePresentationTests: XCTestCase {
    @MainActor
    func testRefreshPreservesLegacyContractWithoutLifecycleError() async throws {
        let taskID = UUID(uuidString: "5A8F17C3-733B-46EE-AE48-015D091A0B91")!
        let store = WorkspaceStore(
            engine: LegacyContractEngine(),
            healthRetryAttempts: 1,
            preferences: MemoryPreferences()
        )

        await store.refreshTaskContract(taskID: taskID)

        let contract = try XCTUnwrap(store.taskContracts[taskID])
        XCTAssertTrue(contract.isLegacyReadOnly)
        XCTAssertEqual(contract.goal, "Historical task outcome")
        XCTAssertNil(store.taskLifecycleError)
    }

    @MainActor
    func testRefreshRestoresTasksQueueAndAttentionSections() async {
        let preferences = MemoryPreferences()
        let store = WorkspaceStore(
            engine: WorkspaceEngine(),
            healthRetryAttempts: 1,
            preferences: preferences
        )
        await store.refreshHealth()

        XCTAssertEqual(store.connectionState, .connected("0.1.0"))
        XCTAssertEqual(store.tasks.count, 3)
        XCTAssertEqual(store.githubWorkItems.count, 1)
        XCTAssertEqual(store.attentionTaskCount, 1)
        XCTAssertEqual(store.tasks(for: .awaitingApproval).map(\.title), ["Needs approval"])
        XCTAssertEqual(store.tasks(for: .monitoring).map(\.title), ["Watching checks"])
        XCTAssertEqual(store.tasks(for: .completed).map(\.title), ["Shipped"])
        XCTAssertEqual(store.contentState(for: .queue), .ready)
    }

    @MainActor
    func testSelectionAndSortPreferencesAreStablePerWorkspace() async throws {
        let preferences = MemoryPreferences()
        let store = WorkspaceStore(
            engine: WorkspaceEngine(),
            healthRetryAttempts: 1,
            preferences: preferences
        )
        await store.refreshHealth()
        let item = try XCTUnwrap(store.githubWorkItems.first)
        await store.selectWorkItem(item)
        XCTAssertEqual(store.selectedWorkItemID, item.id)
        XCTAssertNil(store.selectedTaskID)
        XCTAssertEqual(store.selectedRepository?.repository.fullName, "acme/widget")

        let sort = PullRequestSort(key: .latestHeadCommit, direction: .descending)
        store.setPullRequestSort(sort)
        XCTAssertEqual(preferences.saved["acme/widget"]?.pullRequestSort, sort)
        store.loadPresentationPreferences(workspaceID: "global")
        XCTAssertEqual(store.presentationPreferences, WorkspacePresentationPreferences())
    }

    @MainActor
    func testSortAndFilterChangesPublishObservableUpdates() {
        let store = WorkspaceStore(
            engine: WorkspaceEngine(),
            healthRetryAttempts: 1,
            preferences: MemoryPreferences()
        )
        var updateCount = 0
        let observation = store.objectWillChange.sink { updateCount += 1 }

        store.setPullRequestSort(PullRequestSort(key: .recentlyUpdated, direction: .descending))
        store.setRepositorySort(RepositorySort(key: .recentlyPushed, direction: .descending))
        store.setWorkspaceFilter(WorkspaceFilter(draft: true))

        XCTAssertEqual(updateCount, 3)
        withExtendedLifetime(observation) {}
    }

    @MainActor
    func testChangingWorkspaceSectionClearsStaleDetailSelection() async throws {
        let store = WorkspaceStore(
            engine: WorkspaceEngine(),
            healthRetryAttempts: 1,
            preferences: MemoryPreferences()
        )
        await store.refreshHealth()
        await store.selectWorkItem(try XCTUnwrap(store.githubWorkItems.first))
        XCTAssertNotNil(store.selectedWorkItem)
        XCTAssertNotNil(store.selectedRepository)

        store.selectSection(.issues)

        XCTAssertNil(store.selectedWorkItemID)
        XCTAssertNil(store.selectedRepositoryID)
        XCTAssertNil(store.selectedRepository)
        XCTAssertNil(store.selectedTaskID)
    }

    func testTimestampPresentationProvidesRelativeAndExactValues() {
        let now = Date(timeIntervalSince1970: 1_752_405_600)
        let value = TimestampPresentation(
            date: now.addingTimeInterval(-3_600),
            now: now,
            locale: Locale(identifier: "en_US_POSIX"),
            timeZone: TimeZone(secondsFromGMT: 0)!
        )
        XCTAssertFalse(value.relative.isEmpty)
        XCTAssertTrue(value.exact.contains("2025") || value.exact.contains("2026"))
    }

    func testPullRequestTableUsesCompactColumnsAtTypicalSplitWidth() {
        XCTAssertEqual(PullRequestTableDensity.resolve(availableWidth: 870), .compact)
        XCTAssertEqual(PullRequestTableDensity.resolve(availableWidth: 1_100), .expanded)
    }

    func testExplicitEmptyPartialCancelledAndBlockedStates() throws {
        XCTAssertEqual(WorkspaceContentState.resolve(hasContent: false, loading: false, error: nil), .empty)
        XCTAssertEqual(WorkspaceContentState.resolve(hasContent: true, loading: false, error: "One repo failed"), .partial("One repo failed"))
        XCTAssertEqual(WorkspaceContentState.resolve(hasContent: false, loading: false, error: "Offline"), .blocked("Offline"))
        XCTAssertEqual(TaskSurfaceState.resolve(state: .cancelled, reason: nil), .cancelled)
        XCTAssertEqual(TaskSurfaceState.resolve(state: .blocked, reason: "Binding required"), .blocked("Binding required"))
        XCTAssertEqual(TaskSurfaceState.resolve(state: .completed, reason: nil), .completed)
    }
}

private actor LegacyContractEngine: EngineServing {
    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result {
        guard method == "task.contract" else {
            throw EngineError.remote(code: -32601, message: "method not found")
        }
        let json = #"{"version":1,"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Historical task outcome","acceptanceCriteria":["Preserve the original audit record"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"instructionDigests":[],"verificationCommands":[],"requiredCapabilities":[],"risk":"moderate","sensitivePaths":[],"dependencies":[]}"#
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }
}

@MainActor
private final class MemoryPreferences: WorkspacePreferencesPersisting {
    var saved: [String: WorkspacePresentationPreferences] = [:]

    func load(workspaceID: String) -> WorkspacePresentationPreferences? { saved[workspaceID] }
    func save(_ preferences: WorkspacePresentationPreferences, workspaceID: String) {
        saved[workspaceID] = preferences
    }
}

private actor WorkspaceEngine: EngineServing {
    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result {
        let json: String
        switch method {
        case "system.health":
            json = #"{"status":"ok","version":"0.1.0"}"#
        case "github.status":
            json = #"{"connected":true,"account":null,"repositoryCount":1,"lastSyncedAt":"2026-07-13T12:00:00Z"}"#
        case "github.repositories":
            json = #"[{"id":42,"fullName":"acme/widget","description":null,"private":true,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/acme/widget","updatedAt":"2026-07-13T12:00:00Z","pushedAt":"2026-07-13T11:00:00Z","openIssuesCount":1,"openPullRequestCount":1,"failingCheckCount":0,"defaultBranchSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","defaultBranchCommittedAt":"2026-07-13T10:00:00Z","installationId":99,"permissions":null}]"#
        case "github.queue":
            json = #"[{"id":108,"repositoryFullName":"acme/widget","number":8,"kind":"pullRequest","title":"Repair CI","state":"open","body":null,"author":"octocat","htmlUrl":"https://github.com/acme/widget/pull/8","draft":false,"commentsCount":0,"baseRef":"main","baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headRef":"repair","headSha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","createdAt":"2026-07-13T08:00:00Z","headCommittedAt":"2026-07-13T11:00:00Z","updatedAt":"2026-07-13T12:00:00Z","ciHealth":"passing","reviewDecision":"approved","hasConflicts":false,"labels":[],"assignees":[],"milestone":null}]"#
        case "task.list":
            json = #"[{"id":"11111111-1111-1111-1111-111111111111","title":"Needs approval","repositoryPath":"/tmp/a","state":"awaitingPreparationApproval","createdAt":"2026-07-13T09:00:00Z","updatedAt":"2026-07-13T12:00:00Z"},{"id":"22222222-2222-2222-2222-222222222222","title":"Watching checks","repositoryPath":"/tmp/b","state":"monitoring","createdAt":"2026-07-13T09:00:00Z","updatedAt":"2026-07-13T11:00:00Z"},{"id":"33333333-3333-3333-3333-333333333333","title":"Shipped","repositoryPath":"/tmp/c","state":"completed","createdAt":"2026-07-13T09:00:00Z","updatedAt":"2026-07-13T10:00:00Z"}]"#
        case "github.repository":
            json = #"{"repository":{"id":42,"fullName":"acme/widget","description":null,"private":true,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/acme/widget","updatedAt":"2026-07-13T12:00:00Z","openIssuesCount":1},"workItems":[],"discussions":[],"checks":[],"workflowRuns":[]}"#
        default:
            throw EngineError.remote(code: -32601, message: "method not found")
        }
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }
}
