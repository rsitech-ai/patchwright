import XCTest
@testable import PatchwrightCore

final class ModelsTests: XCTestCase {
    func testDecodesEngineTask() throws {
        let data = Data(#"{"id":"5A8F17C3-733B-46EE-AE48-015D091A0B91","title":"Fix issue","repositoryPath":"/tmp/repo","state":"awaitingPreparationApproval","createdAt":"2026-07-13T10:00:00Z","updatedAt":"2026-07-13T10:01:00Z"}"#.utf8)
        let task = try JSONDecoder.patchwright.decode(EngineeringTask.self, from: data)
        XCTAssertEqual(task.title, "Fix issue")
        XCTAssertEqual(task.state, .awaitingPreparationApproval)
        XCTAssertTrue(task.requiresAttention)
    }

    func testDecodesLegacyPreparationApprovalState() throws {
        let state = try JSONDecoder.patchwright.decode(TaskState.self, from: Data(#""awaitingApproval""#.utf8))
        XCTAssertEqual(state, .awaitingPreparationApproval)
        let encoded = try JSONEncoder().encode(state)
        XCTAssertEqual(String(decoding: encoded, as: UTF8.self), #""awaitingPreparationApproval""#)
    }

    func testDecodesRecoverableTaskInterruption() throws {
        let data = Data(#"{"id":"5A8F17C3-733B-46EE-AE48-015D091A0B91","title":"Fix issue","repositoryPath":"/tmp/repo","state":"blocked","createdAt":"2026-07-13T10:00:00Z","updatedAt":"2026-07-13T10:01:00Z","interruption":{"state":"blocked","resumeState":"assessing","reason":"Repository binding required"}}"#.utf8)
        let task = try JSONDecoder.patchwright.decode(EngineeringTask.self, from: data)
        XCTAssertEqual(task.state, .blocked)
        XCTAssertEqual(task.interruption?.resumeState, .assessing)
        XCTAssertEqual(task.interruption?.reason, "Repository binding required")
        XCTAssertTrue(task.requiresAttention)
    }

    func testTaskAttentionStatesCoverEveryOperatorGate() {
        for state in [
            TaskState.awaitingPreparationApproval,
            .awaitingDeliveryApproval,
            .awaitingMergeApproval,
            .blocked,
            .failed,
        ] {
            XCTAssertTrue(state.requiresAttention)
        }
        XCTAssertFalse(TaskState.implementing.requiresAttention)
        XCTAssertFalse(TaskState.paused.requiresAttention)
    }

    func testDecodesGitHubRepositorySnapshot() throws {
        let data = Data(#"{"repository":{"id":1,"fullName":"octocat/hello","description":null,"private":false,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/octocat/hello","updatedAt":"2026-07-13T10:00:00Z","openIssuesCount":1},"workItems":[{"id":10,"repositoryFullName":"octocat/hello","number":1,"kind":"pullRequest","title":"Ship it","state":"open","body":"Body","author":"octocat","htmlUrl":"https://github.com/octocat/hello/pull/1","draft":true,"commentsCount":2,"headSha":"abc","updatedAt":"2026-07-13T10:00:00Z","labels":["bug"],"assignees":["hubot"],"milestone":"v1"}],"discussions":[],"checks":[],"workflowRuns":[]}"#.utf8)
        let snapshot = try JSONDecoder.patchwright.decode(GitHubRepositorySnapshot.self, from: data)
        XCTAssertEqual(snapshot.repository.fullName, "octocat/hello")
        XCTAssertEqual(snapshot.workItems.first?.kind, .pullRequest)
        XCTAssertEqual(snapshot.workItems.first?.headSHA, "abc")
        XCTAssertEqual(snapshot.workItems.first?.labels, ["bug"])
        XCTAssertEqual(snapshot.workItems.first?.assignees, ["hubot"])
        XCTAssertEqual(snapshot.workItems.first?.milestone, "v1")
    }

    func testJSONLineFramerWaitsForACompleteFragmentedResponse() throws {
        var framer = JSONLineFramer(maximumBytes: 128)
        XCTAssertNil(try framer.append(Data(#"{"result":{"repositories":51"#.utf8)))
        let line = try XCTUnwrap(framer.append(Data("}}\nignored".utf8)))
        XCTAssertEqual(String(decoding: line, as: UTF8.self), #"{"result":{"repositories":51}}"#)
    }

    @MainActor
    func testWorkspaceStoreSurfacesEngineFailure() async {
        let store = WorkspaceStore(engine: FailingEngine(), healthRetryAttempts: 1)
        await store.refreshHealth()
        XCTAssertEqual(store.connectionState, .failed("Engine unavailable"))
    }
}

private struct FailingEngine: EngineServing {
    func call<Result: Decodable & Sendable>(method: String, params: [String: String], as type: Result.Type) async throws -> Result {
        throw EngineError.connectionFailed("Engine unavailable")
    }
}
