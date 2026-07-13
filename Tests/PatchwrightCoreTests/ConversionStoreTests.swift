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
        XCTAssertNil(store.conversionError)
        let methods = await engine.calledMethods()
        XCTAssertEqual(methods, ["task.previewFromGitHub", "task.createFromGitHub"])
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

    private func issue() throws -> GitHubWorkItem {
        let data = Data(#"{"id":107,"repositoryFullName":"acme/widget","number":7,"kind":"issue","title":"Fix login","state":"open","body":null,"author":"octocat","htmlUrl":"https://github.com/acme/widget/issues/7","draft":false,"commentsCount":0,"headSha":null,"updatedAt":"2026-07-13T12:00:00Z","labels":[],"assignees":[],"milestone":null}"#.utf8)
        return try JSONDecoder.patchwright.decode(GitHubWorkItem.self, from: data)
    }
}

private actor ConversionEngine: EngineServing {
    private var methods: [String] = []
    private let failCreation: Bool

    init(failCreation: Bool = false) {
        self.failCreation = failCreation
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
        let json: String
        switch method {
        case "task.previewFromGitHub":
            json = #"{"repositoryFullName":"acme/widget","repositoryId":42,"repositoryBindingId":"11111111-1111-1111-1111-111111111111","itemNumber":7,"sourceKind":"issue","title":"Fix login","goal":"Resolve issue","acceptanceCriteria":["Verified"],"repositoryPath":"/tmp/acme-widget","baseSha":null,"headSha":null,"sourceUpdatedAt":"2026-07-13T12:00:00Z","snapshotAt":"2026-07-13T12:01:00Z","requiresConfirmation":true}"#
        case "task.createFromGitHub":
            json = #"{"preview":{"repositoryFullName":"acme/widget","repositoryId":42,"repositoryBindingId":"11111111-1111-1111-1111-111111111111","itemNumber":7,"sourceKind":"issue","title":"Fix login","goal":"Resolve issue","acceptanceCriteria":["Verified"],"repositoryPath":"/tmp/acme-widget","baseSha":null,"headSha":null,"sourceUpdatedAt":"2026-07-13T12:00:00Z","snapshotAt":"2026-07-13T12:01:00Z","requiresConfirmation":true},"task":{"id":"22222222-2222-2222-2222-222222222222","title":"Fix login","repositoryPath":"/tmp/acme-widget","state":"discovered","createdAt":"2026-07-13T12:02:00Z","updatedAt":"2026-07-13T12:02:00Z"},"created":true}"#
        default:
            throw EngineError.remote(code: -32601, message: "method not found")
        }
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }

    func calledMethods() -> [String] { methods }
}
