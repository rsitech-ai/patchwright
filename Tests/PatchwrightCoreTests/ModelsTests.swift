import XCTest
@testable import PatchwrightCore

final class ModelsTests: XCTestCase {
    func testDecodesLegacyContractSnapshotForReadOnlyAudit() throws {
        let data = Data(#"{"version":1,"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Historical task outcome","acceptanceCriteria":["Preserve the original audit record"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"instructionDigests":[],"verificationCommands":[],"requiredCapabilities":[],"risk":"moderate","sensitivePaths":[],"dependencies":[]}"#.utf8)

        let snapshot = try JSONDecoder.patchwright.decode(TaskContractSnapshot.self, from: data)

        XCTAssertTrue(snapshot.isLegacyReadOnly)
        XCTAssertEqual(snapshot.version, 1)
        XCTAssertEqual(snapshot.goal, "Historical task outcome")
        XCTAssertTrue(snapshot.verificationCommands.isEmpty)
    }

    func testLegacyContractSnapshotRejectsMalformedPartialIntegrityEvidence() throws {
        let data = Data(#"{"version":1,"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Historical task outcome","acceptanceCriteria":["Preserve the original audit record"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"sourceSha256":"not-a-digest","instructionDigests":[],"verificationCommands":[],"requiredCapabilities":[],"risk":"moderate","sensitivePaths":[],"dependencies":[]}"#.utf8)

        XCTAssertThrowsError(
            try JSONDecoder.patchwright.decode(TaskContractSnapshot.self, from: data)
        )
    }

    func testLegacyContractSnapshotRejectsSingleValidIntegrityHash() throws {
        let data = Data(#"{"version":1,"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Historical task outcome","acceptanceCriteria":["Preserve the original audit record"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"sourceSha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","instructionDigests":[],"verificationCommands":[],"requiredCapabilities":[],"risk":"moderate","sensitivePaths":[],"dependencies":[]}"#.utf8)

        XCTAssertThrowsError(
            try JSONDecoder.patchwright.decode(TaskContractSnapshot.self, from: data)
        )
    }

    func testDecodesExactPreparationPreviewAndApprovalBoundary() throws {
        let previewData = Data(#"{"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","repositoryBindingId":"11111111-1111-1111-1111-111111111111","repositoryFullName":"acme/widget","repositoryPath":"/tmp/repository","sourceSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","worktreePath":"/tmp/worktrees/task","branch":"patchwright/task","invalidationGeneration":7,"policySha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","instructionSha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","contract":{"version":1,"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Fix login","acceptanceCriteria":["Tests pass"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"sourceSha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","repositorySha256":"9999999999999999999999999999999999999999999999999999999999999999","instructionDigests":[{"source":"resolvedInstructions","sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","precedence":0}],"verificationCommands":[{"program":"cargo","args":["test","--workspace"]}],"requiredCapabilities":["prepareWorktree"],"risk":"moderate","sensitivePaths":[{"path":"Cargo.lock","reason":"Dependency boundary"}],"dependencies":[]},"fingerprint":{"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","githubRepositoryId":42,"repositoryFullName":"acme/widget","actionKind":"prepareWorktree","pullRequestNumber":null,"branch":"patchwright/task","headSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","baseSha":null,"payloadSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","policySha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","instructionSha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","invalidationGeneration":7}}"#.utf8)
        let preview = try JSONDecoder.patchwright.decode(PreparationPreview.self, from: previewData)

        XCTAssertEqual(preview.repositoryFullName, "acme/widget")
        XCTAssertEqual(preview.sourceSha, String(repeating: "a", count: 40))
        XCTAssertEqual(preview.fingerprint.actionKind, "prepareWorktree")
        XCTAssertEqual(preview.invalidationGeneration, 7)
        XCTAssertEqual(preview.contract.goal, "Fix login")
        XCTAssertEqual(preview.contract.verificationCommands.first?.argvDisplay, #"["cargo","test","--workspace"]"#)
    }

    func testContractDecodingRejectsAnEmptyOrMalformedVerificationBoundary() throws {
        let valid = try XCTUnwrap(
            JSONSerialization.jsonObject(with: validContractData) as? [String: Any]
        )
        var empty = valid
        empty["verificationCommands"] = []
        XCTAssertThrowsError(
            try JSONDecoder.patchwright.decode(
                TaskContract.self,
                from: JSONSerialization.data(withJSONObject: empty)
            )
        )

        var malformed = valid
        malformed["verificationCommands"] = [["program": "cargo\nsh", "args": ["test"]]]
        XCTAssertThrowsError(
            try JSONDecoder.patchwright.decode(
                TaskContract.self,
                from: JSONSerialization.data(withJSONObject: malformed)
            )
        )
    }

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

    func testDecodesTypedGitHubPullRequestTaskSource() throws {
        let data = Data(#"{"id":"5A8F17C3-733B-46EE-AE48-015D091A0B91","title":"Repair CI","repositoryPath":"/tmp/repo","state":"discovered","createdAt":"2026-07-13T10:00:00Z","updatedAt":"2026-07-13T10:01:00Z","source":{"kind":"githubPullRequest","details":{"repositoryId":42,"repositoryFullName":"acme/widget","number":8,"htmlUrl":"https://github.com/acme/widget/pull/8","snapshotAt":"2026-07-13T10:00:00Z","baseRef":"main","baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headRef":"repair","headSha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","contractVersion":1}"#.utf8)
        let task = try JSONDecoder.patchwright.decode(EngineeringTask.self, from: data)
        guard case .githubPullRequest(let source) = task.source else {
            return XCTFail("Expected GitHub pull request source")
        }
        XCTAssertEqual(source.repositoryID, 42)
        XCTAssertEqual(source.baseSHA, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        XCTAssertEqual(source.headSHA, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        XCTAssertEqual(task.contractVersion, 1)
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
        let data = Data(#"{"repository":{"id":1,"fullName":"octocat/hello","description":null,"private":false,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/octocat/hello","updatedAt":"2026-07-13T10:00:00Z","pushedAt":"2026-07-13T09:30:00Z","openIssuesCount":1,"openPullRequestCount":1,"failingCheckCount":2,"defaultBranchSha":"def","defaultBranchCommittedAt":"2026-07-13T09:00:00Z","installationId":99,"permissions":{"admin":false,"maintain":true,"push":true,"triage":true,"pull":true}},"workItems":[{"id":10,"repositoryFullName":"octocat/hello","number":1,"kind":"pullRequest","title":"Ship it","state":"open","body":"Body","author":"octocat","htmlUrl":"https://github.com/octocat/hello/pull/1","draft":true,"commentsCount":2,"headSha":"abc","baseSha":"def","headRef":"feature","baseRef":"main","createdAt":"2026-07-12T08:00:00Z","headCommittedAt":"2026-07-13T08:30:00Z","latestReviewAt":"2026-07-13T09:45:00Z","updatedAt":"2026-07-13T10:00:00Z","reviewDecision":"changesRequested","ciHealth":"failing","mergeable":false,"mergeableState":"dirty","headRepositoryFullName":"fork/hello","headRepositoryFork":true,"maintainerCanModify":true,"additions":12,"deletions":3,"changedFiles":2,"labels":["bug"],"assignees":["hubot"],"milestone":"v1"}],"discussions":[],"checks":[],"workflowRuns":[]}"#.utf8)
        let snapshot = try JSONDecoder.patchwright.decode(GitHubRepositorySnapshot.self, from: data)
        XCTAssertEqual(snapshot.repository.fullName, "octocat/hello")
        XCTAssertEqual(snapshot.repository.openPullRequestCount, 1)
        XCTAssertEqual(snapshot.repository.failingCheckCount, 2)
        XCTAssertEqual(snapshot.repository.defaultBranchSHA, "def")
        XCTAssertEqual(snapshot.repository.installationID, 99)
        XCTAssertEqual(snapshot.repository.updatedAt, ISO8601DateFormatter().date(from: "2026-07-13T10:00:00Z"))
        XCTAssertEqual(snapshot.workItems.first?.kind, .pullRequest)
        XCTAssertEqual(snapshot.workItems.first?.headSHA, "abc")
        XCTAssertEqual(snapshot.workItems.first?.baseSHA, "def")
        XCTAssertEqual(snapshot.workItems.first?.reviewDecision, "changesRequested")
        XCTAssertEqual(snapshot.workItems.first?.ciHealth, "failing")
        XCTAssertEqual(snapshot.workItems.first?.changedFiles, 2)
        XCTAssertEqual(snapshot.workItems.first?.headCommittedAt, ISO8601DateFormatter().date(from: "2026-07-13T08:30:00Z"))
        XCTAssertEqual(snapshot.workItems.first?.labels, ["bug"])
        XCTAssertEqual(snapshot.workItems.first?.assignees, ["hubot"])
        XCTAssertEqual(snapshot.workItems.first?.milestone, "v1")
    }

    func testDecodesLegacyGitHubRepositorySnapshotWithNewMetadataAbsent() throws {
        let data = Data(#"{"repository":{"id":1,"fullName":"octocat/hello","description":null,"private":false,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/octocat/hello","updatedAt":"2026-07-13T10:00:00Z","openIssuesCount":1},"workItems":[{"id":10,"repositoryFullName":"octocat/hello","number":1,"kind":"issue","title":"Legacy","state":"open","body":null,"author":"octocat","htmlUrl":"https://github.com/octocat/hello/issues/1","draft":false,"commentsCount":0,"headSha":null,"updatedAt":"2026-07-13T10:00:00Z","labels":[],"assignees":[],"milestone":null}],"discussions":[],"checks":[],"workflowRuns":[]}"#.utf8)
        let snapshot = try JSONDecoder.patchwright.decode(GitHubRepositorySnapshot.self, from: data)
        XCTAssertNil(snapshot.repository.pushedAt)
        XCTAssertNil(snapshot.repository.defaultBranchSHA)
        XCTAssertNil(snapshot.workItems.first?.headCommittedAt)
        XCTAssertEqual(snapshot.workItems.first?.updatedAt, ISO8601DateFormatter().date(from: "2026-07-13T10:00:00Z"))
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

private let validContractData = Data(#"{"version":1,"taskId":"5A8F17C3-733B-46EE-AE48-015D091A0B91","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Fix login","acceptanceCriteria":["Tests pass"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"sourceSha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","repositorySha256":"9999999999999999999999999999999999999999999999999999999999999999","instructionDigests":[{"source":"resolvedInstructions","sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","precedence":0}],"verificationCommands":[{"program":"cargo","args":["test","--workspace"]}],"requiredCapabilities":[],"risk":"moderate","sensitivePaths":[{"path":"Cargo.lock","reason":"Dependency boundary"}],"dependencies":[]}"#.utf8)

private struct FailingEngine: EngineServing {
    func call<Result: Decodable & Sendable>(method: String, params: [String: String], as type: Result.Type) async throws -> Result {
        throw EngineError.connectionFailed("Engine unavailable")
    }
}
