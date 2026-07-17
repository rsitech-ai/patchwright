import Foundation
import XCTest
@testable import PatchwrightCore

final class WorkspaceConcurrencyTests: XCTestCase {
    @MainActor
    func testRepositorySelectionIsLastRequestWins() async throws {
        let responses = RepositoryResponseController()
        let store = WorkspaceStore(engine: SelectionRaceEngine(responses: responses), healthRetryAttempts: 1)
        let repositoryA = try fixtureRepository(id: 1, fullName: "acme/alpha")
        let repositoryB = try fixtureRepository(id: 2, fullName: "acme/beta")

        let first = Task { await store.selectRepository(repositoryA) }
        await responses.waitUntilRequested("acme/alpha")
        let second = Task { await store.selectRepository(repositoryB) }
        await responses.waitUntilRequested("acme/beta")
        await responses.resume(repositoryB)
        await second.value
        await responses.resume(repositoryA)
        await first.value

        XCTAssertEqual(store.selectedRepositoryID, repositoryB.id)
        XCTAssertEqual(store.selectedRepository?.repository.fullName, repositoryB.fullName)
    }

    @MainActor
    func testWorkItemSelectionIsLastRequestWinsAcrossRepositories() async throws {
        let responses = RepositoryResponseController()
        let store = WorkspaceStore(engine: SelectionRaceEngine(responses: responses), healthRetryAttempts: 1)
        await store.refreshHealth()
        let itemA = try fixtureWorkItem(id: 101, repository: "acme/alpha")
        let itemB = try fixtureWorkItem(id: 202, repository: "acme/beta")

        let first = Task { await store.selectWorkItem(itemA) }
        await responses.waitUntilRequested("acme/alpha")
        let second = Task { await store.selectWorkItem(itemB) }
        await responses.waitUntilRequested("acme/beta")
        await responses.resume(try fixtureRepository(id: 2, fullName: "acme/beta"))
        await second.value
        await responses.resume(try fixtureRepository(id: 1, fullName: "acme/alpha"))
        await first.value

        XCTAssertEqual(store.selectedWorkItemID, itemB.id)
        XCTAssertEqual(store.selectedRepositoryID, 2)
        XCTAssertEqual(store.selectedRepository?.repository.fullName, "acme/beta")
    }

    @MainActor
    func testHealthRefreshIsLastRequestWins() async {
        let responses = HealthResponseController()
        let store = WorkspaceStore(engine: HealthRaceEngine(responses: responses), healthRetryAttempts: 1)

        let first = Task { await store.refreshHealth() }
        await responses.waitUntilRequested(1)
        let second = Task { await store.refreshHealth() }
        await responses.waitUntilRequested(2)
        await responses.resume(index: 2, version: "2.0.0")
        await second.value
        await responses.resume(index: 1, version: "1.0.0")
        await first.value

        XCTAssertEqual(store.connectionState, .connected("2.0.0"))
    }
}

private actor RepositoryResponseController {
    private var requests = Set<String>()
    private var requestWaiters: [String: [CheckedContinuation<Void, Never>]] = [:]
    private var responseWaiters: [String: CheckedContinuation<String, Never>] = [:]
    private var pendingResponses: [String: String] = [:]

    func response(for fullName: String) async -> String {
        requests.insert(fullName)
        requestWaiters.removeValue(forKey: fullName)?.forEach { $0.resume() }
        if let response = pendingResponses.removeValue(forKey: fullName) { return response }
        return await withCheckedContinuation { responseWaiters[fullName] = $0 }
    }

    func waitUntilRequested(_ fullName: String) async {
        guard !requests.contains(fullName) else { return }
        await withCheckedContinuation { requestWaiters[fullName, default: []].append($0) }
    }

    func resume(_ repository: GitHubRepository) {
        let response = fixtureSnapshotJSON(repository)
        if let waiter = responseWaiters.removeValue(forKey: repository.fullName) {
            waiter.resume(returning: response)
        } else {
            pendingResponses[repository.fullName] = response
        }
    }
}

private actor HealthResponseController {
    private var requestCount = 0
    private var requests = Set<Int>()
    private var requestWaiters: [Int: [CheckedContinuation<Void, Never>]] = [:]
    private var responseWaiters: [Int: CheckedContinuation<String, Never>] = [:]
    private var pendingResponses: [Int: String] = [:]

    func nextResponse() async -> String {
        requestCount += 1
        let index = requestCount
        requests.insert(index)
        requestWaiters.removeValue(forKey: index)?.forEach { $0.resume() }
        if let response = pendingResponses.removeValue(forKey: index) { return response }
        return await withCheckedContinuation { responseWaiters[index] = $0 }
    }

    func waitUntilRequested(_ index: Int) async {
        guard !requests.contains(index) else { return }
        await withCheckedContinuation { requestWaiters[index, default: []].append($0) }
    }

    func resume(index: Int, version: String) {
        let response = #"{"status":"ok","version":"\#(version)"}"#
        if let waiter = responseWaiters.removeValue(forKey: index) {
            waiter.resume(returning: response)
        } else {
            pendingResponses[index] = response
        }
    }
}

private actor SelectionRaceEngine: EngineServing {
    let responses: RepositoryResponseController

    init(responses: RepositoryResponseController) {
        self.responses = responses
    }

    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result {
        let json: String
        switch method {
        case "github.repository":
            json = await responses.response(for: params["fullName"] ?? "")
        case "system.health":
            json = #"{"status":"ok","version":"0.1.0"}"#
        case "github.status":
            json = #"{"connected":true,"account":null,"repositoryCount":2,"lastSyncedAt":null}"#
        case "github.repositories":
            json = "[\(fixtureRepositoryJSON(id: 1, fullName: "acme/alpha")),\(fixtureRepositoryJSON(id: 2, fullName: "acme/beta"))]"
        case "github.queue", "github.queue.decisions", "task.list":
            json = "[]"
        default:
            throw EngineError.remote(code: -32601, message: "method not found: \(method)")
        }
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }
}

private actor HealthRaceEngine: EngineServing {
    let responses: HealthResponseController

    init(responses: HealthResponseController) {
        self.responses = responses
    }

    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result {
        let json: String
        switch method {
        case "system.health":
            json = await responses.nextResponse()
        case "github.status":
            json = #"{"connected":true,"account":null,"repositoryCount":0,"lastSyncedAt":null}"#
        case "github.repositories", "github.queue", "github.queue.decisions", "task.list":
            json = "[]"
        default:
            throw EngineError.remote(code: -32601, message: "method not found: \(method)")
        }
        return try JSONDecoder.patchwright.decode(Result.self, from: Data(json.utf8))
    }
}

private func fixtureRepository(id: UInt64, fullName: String) throws -> GitHubRepository {
    try JSONDecoder.patchwright.decode(
        GitHubRepository.self,
        from: Data(fixtureRepositoryJSON(id: id, fullName: fullName).utf8)
    )
}

private func fixtureRepositoryJSON(id: UInt64, fullName: String) -> String {
    #"{"id":\#(id),"fullName":"\#(fullName)","description":null,"private":true,"archived":false,"defaultBranch":"main","htmlUrl":"https://github.com/\#(fullName)","updatedAt":"2026-07-13T12:00:00Z","pushedAt":null,"openIssuesCount":1,"openPullRequestCount":1,"failingCheckCount":0,"defaultBranchSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","defaultBranchCommittedAt":null,"installationId":99,"permissions":null}"#
}

private func fixtureSnapshotJSON(_ repository: GitHubRepository) -> String {
    #"{"repository":\#(fixtureRepositoryJSON(id: repository.id, fullName: repository.fullName)),"workItems":[],"discussions":[],"checks":[],"workflowRuns":[]}"#
}

private func fixtureWorkItem(id: UInt64, repository: String) throws -> GitHubWorkItem {
    let json = #"{"id":\#(id),"repositoryFullName":"\#(repository)","number":8,"kind":"pullRequest","title":"Repair CI","state":"open","stateReason":null,"body":null,"author":"octocat","htmlUrl":"https://github.com/\#(repository)/pull/8","draft":false,"commentsCount":0,"baseRef":"main","baseSha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","headRef":"repair","headSha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","merged":false,"mergeCommitSha":null,"createdAt":"2026-07-13T08:00:00Z","headCommittedAt":"2026-07-13T11:00:00Z","latestReviewAt":null,"updatedAt":"2026-07-13T12:00:00Z","reviewDecision":"approved","ciHealth":"passing","mergeable":true,"mergeableState":"clean","rebaseable":true,"hasConflicts":false,"headRepositoryFullName":"\#(repository)","headRepositoryFork":false,"maintainerCanModify":true,"additions":1,"deletions":1,"changedFiles":1,"labels":[],"assignees":[],"milestone":null}"#
    return try JSONDecoder.patchwright.decode(GitHubWorkItem.self, from: Data(json.utf8))
}
