import Foundation
import XCTest
@testable import PatchwrightCore

final class DeliveryStoreTests: XCTestCase {
    func testClosePullRequestPayloadCarriesTheExactHeadSHA() throws {
        let sha = String(repeating: "b", count: 40)
        let payload = GitHubActionPayload(
            kind: "closePullRequest",
            pullRequestNumber: 7,
            expectedHeadSha: sha
        )
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: JSONEncoder().encode(payload)) as? [String: Any]
        )
        XCTAssertEqual(object["kind"] as? String, "closePullRequest")
        XCTAssertEqual(object["pullRequestNumber"] as? UInt64, 7)
        XCTAssertEqual(object["expectedHeadSha"] as? String, sha)
    }

    func testDeliverySheetIsBoundToTheExactPreviewRequest() throws {
        let repositoryRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let source = try String(
            contentsOf: repositoryRoot.appending(path: "Sources/PatchwrightApp/Views/TaskDetailView.swift"),
            encoding: .utf8
        )

        XCTAssertTrue(source.contains(".sheet(item: $deliveryApprovalRequest)"))
        XCTAssertTrue(source.contains("DeliveryApprovalSheet(store: store, task: task, preview: request.preview)"))
        XCTAssertFalse(source.contains(".sheet(isPresented: $deliveryApprovalPresented)"))
    }

    @MainActor
    func testFailedReplacementPreviewInvalidatesPreviouslyApprovedAction() async throws {
        let engine = DeliveryEngine()
        let store = WorkspaceStore(engine: engine, healthRetryAttempts: 1)
        let task = try fixtureTask()
        await store.refreshHealth()

        await store.previewDelivery(
            task: task,
            action: GitHubActionPayload(commentNumber: 7, body: "Approved action A")
        )
        let approvedPreview = try XCTUnwrap(store.deliveryPreviews[task.id])
        await store.approveDelivery(approvedPreview)
        XCTAssertNotNil(store.deliveryApprovals[task.id])

        await store.previewDelivery(
            task: task,
            action: GitHubActionPayload(kind: "closeIssue", issueNumber: 7)
        )

        XCTAssertNil(store.deliveryPreviews[task.id])
        XCTAssertNil(store.deliveryApprovals[task.id])
        XCTAssertNil(store.deliveryExecutions[task.id])

        await store.executeDelivery(approvedPreview)
        let executionCount = await engine.executionCount
        XCTAssertEqual(executionCount, 0)
    }
}

private actor DeliveryEngine: EngineServing {
    private var previewCount = 0
    private(set) var executionCount = 0

    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result {
        let json: String
        switch method {
        case "system.health":
            json = #"{"status":"ok","version":"0.1.0"}"#
        case "task.list":
            json = "[]"
        case "github.status":
            json = #"{"connected":true,"account":null,"repositoryCount":1,"lastSyncedAt":null}"#
        case "github.repositories":
            json = #"[{"id":42,"fullName":"acme/widget","description":null,"private":true,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/acme/widget","updatedAt":"2026-07-13T12:00:00Z","pushedAt":null,"openIssuesCount":1,"openPullRequestCount":0,"failingCheckCount":0,"defaultBranchSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","defaultBranchCommittedAt":null,"installationId":99,"permissions":null}]"#
        case "github.queue", "github.queue.decisions":
            json = "[]"
        case "delivery.preview":
            previewCount += 1
            guard previewCount == 1 else {
                throw EngineError.remote(code: -32040, message: "Action B preview failed")
            }
            json = deliveryPreviewJSON
        case "delivery.approve":
            json = deliveryApprovalJSON
        case "delivery.execute":
            executionCount += 1
            json = #"{"idempotencyKey":"execute-a","state":"succeeded","result":{"id":1,"number":7,"htmlUrl":"https://github.com/acme/widget/issues/7","sha":null,"merged":null,"nodeId":null,"resolved":null}}"#
        case "task.timeline":
            json = "[]"
        default:
            throw EngineError.remote(code: -32601, message: "method not found: \(method)")
        }
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }
}

private let deliveryPreviewJSON = #"{"taskId":"11111111-1111-1111-1111-111111111111","action":{"remote":{"repositoryId":42,"installationId":99,"repositoryFullName":"acme/widget"},"action":{"kind":"comment","issueNumber":7,"body":"Approved action A","pullRequestNumber":null,"threadId":null,"expectedHeadSha":null,"method":null,"branch":null,"fromSha":null,"headSha":null,"event":null,"inlineComments":null,"name":null,"status":null,"conclusion":null,"title":null,"head":null,"base":null},"precondition":{"expectedHeadSha":null,"expectedBaseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","snapshotGeneration":1783944000},"payloadSha256":"payload-a","idempotencySha256":"idempotency-a","requiredPermissions":["issues:write"]},"fingerprint":{"taskId":"11111111-1111-1111-1111-111111111111","githubRepositoryId":42,"repositoryFullName":"acme/widget","actionKind":"comment","pullRequestNumber":null,"branch":null,"headSha":null,"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","payloadSha256":"payload-a","policySha256":"policy-a","instructionSha256":"instruction-a","invalidationGeneration":1}}"#

private let deliveryApprovalJSON = #"{"id":"22222222-2222-2222-2222-222222222222","class":"githubMutation","capability":"issues:write","fingerprint":{"taskId":"11111111-1111-1111-1111-111111111111","githubRepositoryId":42,"repositoryFullName":"acme/widget","actionKind":"comment","pullRequestNumber":null,"branch":null,"headSha":null,"baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","payloadSha256":"payload-a","policySha256":"policy-a","instructionSha256":"instruction-a","invalidationGeneration":1},"approvedBy":"tester","approvedAt":"2026-07-13T12:00:00Z","expiresAt":"2026-07-13T12:05:00Z"}"#

private func fixtureTask() throws -> EngineeringTask {
    let json = #"{"id":"11111111-1111-1111-1111-111111111111","title":"Fix issue","repositoryPath":"/tmp/acme-widget","state":"awaitingDeliveryApproval","createdAt":"2026-07-13T11:00:00Z","updatedAt":"2026-07-13T12:00:00Z","source":{"kind":"githubIssue","details":{"repositoryId":42,"repositoryFullName":"acme/widget","number":7,"htmlUrl":"https://github.com/acme/widget/issues/7","snapshotAt":"2026-07-13T12:00:00Z"}},"repositoryBindingId":"33333333-3333-3333-3333-333333333333","contractVersion":1}"#
    return try JSONDecoder.patchwright.decode(EngineeringTask.self, from: Data(json.utf8))
}
