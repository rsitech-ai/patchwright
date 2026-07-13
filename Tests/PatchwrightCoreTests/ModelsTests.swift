import XCTest
@testable import PatchwrightCore

final class ModelsTests: XCTestCase {
    func testDecodesEngineTask() throws {
        let data = Data(#"{"id":"5A8F17C3-733B-46EE-AE48-015D091A0B91","title":"Fix issue","repositoryPath":"/tmp/repo","state":"awaitingApproval","createdAt":"2026-07-13T10:00:00Z","updatedAt":"2026-07-13T10:01:00Z"}"#.utf8)
        let task = try JSONDecoder.patchwright.decode(EngineeringTask.self, from: data)
        XCTAssertEqual(task.title, "Fix issue")
        XCTAssertEqual(task.state, .awaitingApproval)
        XCTAssertTrue(task.requiresAttention)
    }

    @MainActor
    func testWorkspaceStoreSurfacesEngineFailure() async {
        let store = WorkspaceStore(engine: FailingEngine())
        await store.refreshHealth()
        XCTAssertEqual(store.connectionState, .failed("Engine unavailable"))
    }
}

private struct FailingEngine: EngineServing {
    func call<Result: Decodable & Sendable>(method: String, params: [String: String], as type: Result.Type) async throws -> Result {
        throw EngineError.connectionFailed("Engine unavailable")
    }
}
