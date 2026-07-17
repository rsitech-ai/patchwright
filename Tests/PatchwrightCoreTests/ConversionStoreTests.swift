import Foundation
import XCTest
@testable import PatchwrightCore

final class ConversionStoreTests: XCTestCase {
    @MainActor
    func testPreviewThenCreateSelectsTheDurableTask() async throws {
        let engine = ConversionEngine()
        let store = WorkspaceStore(engine: engine, healthRetryAttempts: 1)
        let item = try issue()

        await store.previewTask(from: item)
        XCTAssertEqual(store.conversionPreview?.itemNumber, 7)
        XCTAssertTrue(store.conversionPreview?.requiresConfirmation == true)
        XCTAssertTrue(store.tasks.isEmpty)

        await store.createTask(from: item)
        XCTAssertEqual(store.tasks.count, 1)
        XCTAssertEqual(store.selectedTaskID, store.tasks.first?.id)
        XCTAssertEqual(store.tasks.first?.state, .awaitingPreparationApproval)
        XCTAssertNil(store.conversionError)

        let taskID = try XCTUnwrap(store.selectedTaskID)
        await store.previewPreparation(taskID: taskID)
        let preparation = try XCTUnwrap(store.preparationPreviews[taskID])
        XCTAssertEqual(preparation.sourceSha, String(repeating: "a", count: 40))
        await store.approveAndPrepare(preparation)
        XCTAssertEqual(store.tasks.first?.state, .preparing)
        XCTAssertEqual(store.tasks.first?.repositoryPath, "/tmp/worktrees/task")
        let methods = await engine.calledMethods()
        XCTAssertEqual(methods, [
            "task.previewFromGitHub", "task.createFromGitHub", "task.plan", "task.contract",
            "task.timeline",
            "task.preparation.preview", "task.preparation.approve", "task.prepare",
            "task.timeline", "task.worktree",
        ])
    }

    @MainActor
    func testCreateRequiresMatchingPreviewAndSurfacesRemoteFailure() async throws {
        let engine = ConversionEngine(failCreation: true)
        let store = WorkspaceStore(engine: engine, healthRetryAttempts: 1)
        let item = try issue()

        await store.createTask(from: item)
        XCTAssertEqual(store.conversionError, "Preview and confirm this GitHub item before creating a task.")
        let methodsBeforePreview = await engine.calledMethods()
        XCTAssertTrue(methodsBeforePreview.isEmpty)

        await store.previewTask(from: item)
        await store.createTask(from: item)
        XCTAssertEqual(store.conversionError, "The GitHub item changed. Refresh and preview it again.")
        XCTAssertTrue(store.tasks.isEmpty)
    }

    @MainActor
    func testPreviewDiscoversAppInstallationThenBindsRepository() async throws {
        let engine = ConversionEngine(requiresInstallationDiscovery: true)
        let store = WorkspaceStore(engine: engine, healthRetryAttempts: 1)
        let item = try issue()

        await store.refreshGitHub()
        XCTAssertNil(store.repositories.first?.installationID)

        await store.previewTask(from: item)

        XCTAssertEqual(store.conversionPreview?.itemNumber, 7)
        XCTAssertEqual(store.repositories.first?.installationID, 99)
        XCTAssertNil(store.conversionError)
        let methods = await engine.calledMethods()
        XCTAssertEqual(methods, [
            "github.status", "github.repositories", "github.queue", "github.queue.decisions",
            "task.previewFromGitHub", "github.sync.repository", "repository.bind",
            "task.previewFromGitHub",
        ])
    }

    private func issue() throws -> GitHubWorkItem {
        let data = Data(#"{"id":107,"repositoryFullName":"acme/widget","number":7,"kind":"issue","title":"Fix login","state":"open","body":null,"author":"octocat","htmlUrl":"https://github.com/acme/widget/issues/7","draft":false,"commentsCount":0,"headSha":null,"updatedAt":"2026-07-13T12:00:00Z","labels":[],"assignees":[],"milestone":null}"#.utf8)
        return try JSONDecoder.patchwright.decode(GitHubWorkItem.self, from: data)
    }
}

private actor ConversionEngine: EngineServing {
    private var methods: [String] = []
    private let failCreation: Bool
    private let requiresInstallationDiscovery: Bool
    private var installationDiscovered = false
    private var prepared = false

    init(failCreation: Bool = false, requiresInstallationDiscovery: Bool = false) {
        self.failCreation = failCreation
        self.requiresInstallationDiscovery = requiresInstallationDiscovery
    }

    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result {
        methods.append(method)
        if method == "task.createFromGitHub", failCreation {
            throw EngineError.remote(code: -32032, message: "The GitHub item changed. Refresh and preview it again.")
        }
        if method == "task.previewFromGitHub", requiresInstallationDiscovery, !installationDiscovered {
            throw EngineError.remote(code: -32033, message: "repository binding missing")
        }
        let json: String
        switch method {
        case "github.status":
            json = #"{"connected":true,"account":null,"repositoryCount":1,"lastSyncedAt":null}"#
        case "github.repositories":
            let installation = installationDiscovered ? ",\"installationId\":99" : ""
            let repository = "{\"id\":42,\"fullName\":\"acme/widget\",\"description\":null,\"private\":true,\"archived\":false,\"defaultBranch\":\"main\",\"htmlUrl\":\"https://github.com/acme/widget\",\"updatedAt\":\"2026-07-13T12:00:00Z\",\"pushedAt\":null,\"openIssuesCount\":1,\"openPullRequestCount\":0,\"failingCheckCount\":0,\"defaultBranchSha\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"defaultBranchCommittedAt\":null\(installation),\"permissions\":{\"admin\":false,\"maintain\":false,\"push\":true,\"triage\":true,\"pull\":true}}"
            json = "[\(repository)]"
        case "github.queue":
            json = #"[{"id":107,"repositoryFullName":"acme/widget","number":7,"kind":"issue","title":"Fix login","state":"open","body":null,"author":"octocat","htmlUrl":"https://github.com/acme/widget/issues/7","draft":false,"commentsCount":0,"headSha":null,"updatedAt":"2026-07-13T12:00:00Z","labels":[],"assignees":[],"milestone":null}]"#
        case "github.queue.decisions":
            json = "[]"
        case "github.sync.repository":
            installationDiscovered = true
            json = #"{"repository":{"id":42,"fullName":"acme/widget","description":null,"private":true,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/acme/widget","updatedAt":"2026-07-13T12:00:00Z","pushedAt":null,"openIssuesCount":1,"openPullRequestCount":0,"failingCheckCount":0,"defaultBranchSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","defaultBranchCommittedAt":null,"installationId":99,"permissions":{"admin":false,"maintain":false,"push":true,"triage":true,"pull":true}},"workItems":[],"discussions":[],"checks":[],"workflowRuns":[]}"#
        case "repository.bind":
            json = #"{"id":"11111111-1111-1111-1111-111111111111","githubRepositoryId":42,"fullName":"acme/widget","installationId":99,"managedClone":"/tmp/acme-widget","worktreeRoot":"/tmp/worktrees"}"#
        case "task.previewFromGitHub":
            json = #"{"repositoryFullName":"acme/widget","repositoryId":42,"repositoryBindingId":"11111111-1111-1111-1111-111111111111","itemNumber":7,"sourceKind":"issue","title":"Fix login","goal":"Resolve issue","acceptanceCriteria":["Verified"],"repositoryPath":"/tmp/acme-widget","baseSha":null,"headSha":null,"sourceUpdatedAt":"2026-07-13T12:00:00Z","snapshotAt":"2026-07-13T12:01:00Z","requiresConfirmation":true}"#
        case "task.createFromGitHub":
            json = #"{"preview":{"repositoryFullName":"acme/widget","repositoryId":42,"repositoryBindingId":"11111111-1111-1111-1111-111111111111","itemNumber":7,"sourceKind":"issue","title":"Fix login","goal":"Resolve issue","acceptanceCriteria":["Verified"],"repositoryPath":"/tmp/acme-widget","baseSha":null,"headSha":null,"sourceUpdatedAt":"2026-07-13T12:00:00Z","snapshotAt":"2026-07-13T12:01:00Z","requiresConfirmation":true},"task":{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/acme-widget","state":"discovered","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:02:00Z"},"created":true}"#
        case "task.plan":
            json = #"{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/acme-widget","state":"awaitingPreparationApproval","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:03:00Z"}"#
        case "task.contract":
            json = conversionContractJSON
        case "task.preparation.preview":
            json = "{\"taskId\":\"22222222-2222-2222-2222-222222222222\",\"repositoryBindingId\":\"11111111-1111-1111-1111-111111111111\",\"repositoryFullName\":\"acme/widget\",\"repositoryPath\":\"/tmp/acme-widget\",\"sourceSha\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"worktreePath\":\"/tmp/worktrees/task\",\"branch\":\"patchwright/22222222-2222-2222-2222-222222222222\",\"invalidationGeneration\":7,\"policySha256\":\"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\",\"instructionSha256\":\"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\",\"contract\":\(conversionContractJSON),\"fingerprint\":{\"taskId\":\"22222222-2222-2222-2222-222222222222\",\"githubRepositoryId\":42,\"repositoryFullName\":\"acme/widget\",\"actionKind\":\"prepareWorktree\",\"pullRequestNumber\":null,\"branch\":\"patchwright/22222222-2222-2222-2222-222222222222\",\"headSha\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"baseSha\":null,\"payloadSha256\":\"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\",\"policySha256\":\"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\",\"instructionSha256\":\"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\",\"invalidationGeneration\":7}}"
        case "task.preparation.approve":
            json = #"{"id":"33333333-3333-3333-3333-333333333333","class":"preparation","capability":"prepareWorktree","fingerprint":{"taskId":"22222222-2222-2222-2222-222222222222","githubRepositoryId":42,"repositoryFullName":"acme/widget","actionKind":"prepareWorktree","pullRequestNumber":null,"branch":"patchwright/22222222-2222-2222-2222-222222222222","headSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","baseSha":null,"payloadSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","policySha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","instructionSha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","invalidationGeneration":7},"approvedBy":"Patchwright operator","approvedAt":"2026-07-13T12:03:00Z","expiresAt":"2026-07-13T12:13:00Z"}"#
        case "task.prepare":
            guard params["approvalId"] == "33333333-3333-3333-3333-333333333333",
                  params["preview"] != nil else {
                throw EngineError.remote(code: -32602, message: "exact preparation approval required")
            }
            prepared = true
            json = #"{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/worktrees/task","state":"preparing","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:04:00Z"}"#
        case "task.timeline":
            if prepared {
                json = #"[{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/acme-widget","state":"discovered","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:02:00Z"},{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/worktrees/task","state":"preparing","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:04:00Z"}]"#
            } else {
                json = #"[{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/acme-widget","state":"discovered","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:02:00Z"},{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/acme-widget","state":"awaitingPreparationApproval","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:03:00Z"}]"#
            }
        case "task.worktree":
            json = #"{"root":"/tmp/worktrees/task","branch":"patchwright/task","headSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","dirty":false}"#
        default:
            throw EngineError.remote(code: -32601, message: "method not found")
        }
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }

    func calledMethods() -> [String] { methods }
}

private let conversionContractJSON = #"{"version":1,"taskId":"22222222-2222-2222-2222-222222222222","source":{"kind":"localRequest"},"repositoryBindingId":"11111111-1111-1111-1111-111111111111","goal":"Fix login","acceptanceCriteria":["Tests pass"],"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headSha":null,"sourceSha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","repositorySha256":"9999999999999999999999999999999999999999999999999999999999999999","instructionDigests":[{"source":"resolvedInstructions","sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","precedence":0}],"verificationCommands":[{"program":"cargo","args":["test","--workspace"]}],"requiredCapabilities":[],"risk":"moderate","sensitivePaths":[{"path":"Cargo.lock","reason":"Dependency boundary"}],"dependencies":[]}"#
