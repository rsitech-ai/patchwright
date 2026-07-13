import XCTest
@testable import PatchwrightCore

final class WorkspaceSortingTests: XCTestCase {
    private let base = Date(timeIntervalSince1970: 1_752_405_600)

    private func repository(id: UInt64, name: String) -> RepositoryQueueRecord {
        RepositoryQueueRecord(id: id, fullName: name, updatedAt: base)
    }

    private func pullRequest(id: UInt64, number: UInt64) -> PullRequestQueueRecord {
        PullRequestQueueRecord(
            id: id,
            number: number,
            updatedAt: base,
            createdAt: base.addingTimeInterval(-14_400),
            author: "alice"
        )
    }

    func testRepositoryModesMatchTheRustContract() {
        var alpha = repository(id: 3, name: "acme/alpha")
        alpha.queuePriority = 2
        alpha.pushedAt = base.addingTimeInterval(-7_200)
        alpha.defaultBranchCommittedAt = base.addingTimeInterval(-10_800)
        alpha.openPullRequestCount = 4
        alpha.failingCheckCount = 1
        var beta = repository(id: 2, name: "acme/beta")
        beta.queuePriority = 1
        beta.updatedAt = base.addingTimeInterval(-3_600)
        beta.pushedAt = base.addingTimeInterval(-3_600)
        beta.defaultBranchCommittedAt = base.addingTimeInterval(-7_200)
        beta.openPullRequestCount = 2
        beta.failingCheckCount = 3

        let cases: [(RepositorySortKey, [UInt64])] = [
            (.queuePriority, [3, 2]),
            (.recentlyUpdated, [3, 2]),
            (.recentlyPushed, [2, 3]),
            (.latestDefaultBranchCommit, [2, 3]),
            (.openPullRequestCount, [3, 2]),
            (.failingCheckCount, [2, 3]),
            (.name, [2, 3]),
        ]
        for (key, expected) in cases {
            XCTAssertEqual(
                sortRepositories([alpha, beta], by: RepositorySort(key: key, direction: .descending)).map(\.id),
                expected,
                "Unexpected order for \(key)"
            )
        }

        XCTAssertEqual(
            sortRepositories(
                [repository(id: 9, name: "acme/same"), repository(id: 4, name: "acme/same")],
                by: RepositorySort(key: .name, direction: .descending)
            ).map(\.id),
            [4, 9]
        )
    }

    func testMissingRepositoryTimestampsStayLastInBothDirections() {
        var known = repository(id: 1, name: "acme/known")
        known.pushedAt = base
        let missing = repository(id: 2, name: "acme/missing")
        for direction in [SortDirection.ascending, .descending] {
            XCTAssertEqual(
                sortRepositories([missing, known], by: RepositorySort(key: .recentlyPushed, direction: direction)).map(\.id),
                [1, 2]
            )
        }

        var newer = repository(id: 3, name: "acme/newer")
        newer.pushedAt = base.addingTimeInterval(3_600)
        XCTAssertEqual(
            sortRepositories([newer, known], by: RepositorySort(key: .recentlyPushed, direction: .ascending)).map(\.id),
            [1, 3]
        )
        XCTAssertEqual(
            sortRepositories([known, newer], by: RepositorySort(key: .recentlyPushed, direction: .descending)).map(\.id),
            [3, 1]
        )
    }

    func testPullRequestModesMatchTheRustContract() {
        var first = pullRequest(id: 10, number: 2)
        first.queuePriority = 1
        first.updatedAt = base.addingTimeInterval(-3_600)
        first.headCommittedAt = base.addingTimeInterval(-7_200)
        first.latestReviewAt = base.addingTimeInterval(-10_800)
        first.ciHealth = .failing
        first.reviewState = .changesRequested
        first.createdAt = base.addingTimeInterval(-18_000)
        first.additions = 30
        first.deletions = 10
        var second = pullRequest(id: 11, number: 7)
        second.queuePriority = 2
        second.headCommittedAt = base.addingTimeInterval(-3_600)
        second.latestReviewAt = base.addingTimeInterval(-7_200)
        second.ciHealth = .passing
        second.reviewState = .approved
        second.additions = 2
        second.deletions = 3

        let cases: [(PullRequestSortKey, [UInt64])] = [
            (.queuePriority, [11, 10]),
            (.recentlyUpdated, [11, 10]),
            (.latestHeadCommit, [11, 10]),
            (.latestReviewActivity, [11, 10]),
            (.ciHealth, [11, 10]),
            (.reviewState, [11, 10]),
            (.createdNewest, [11, 10]),
            (.createdOldest, [10, 11]),
            (.changeSize, [10, 11]),
            (.number, [11, 10]),
        ]
        for (key, expected) in cases {
            XCTAssertEqual(
                sortPullRequests([first, second], by: PullRequestSort(key: key, direction: .descending)).map(\.id),
                expected,
                "Unexpected order for \(key)"
            )
        }
    }

    func testUnknownPullRequestValuesStayLastInBothDirections() {
        var known = pullRequest(id: 1, number: 1)
        known.headCommittedAt = base
        known.ciHealth = .pending
        var unknown = pullRequest(id: 2, number: 2)
        unknown.ciHealth = .unknown
        for direction in [SortDirection.ascending, .descending] {
            XCTAssertEqual(
                sortPullRequests([unknown, known], by: PullRequestSort(key: .latestHeadCommit, direction: direction)).map(\.id),
                [1, 2]
            )
            XCTAssertEqual(
                sortPullRequests([unknown, known], by: PullRequestSort(key: .ciHealth, direction: direction)).map(\.id),
                [1, 2]
            )
        }
    }

    func testFiltersCombineWithAndSemanticsAndPreferencesRoundTrip() throws {
        var candidate = pullRequest(id: 1, number: 1)
        candidate.draft = true
        candidate.author = "octocat"
        candidate.assignees = ["hubot"]
        candidate.labels = ["security"]
        candidate.reviewState = .changesRequested
        candidate.ciHealth = .failing
        candidate.hasConflicts = true
        candidate.queueState = .needsWork
        candidate.activeCodexWork = true

        let filter = WorkspaceFilter(
            open: true,
            draft: true,
            authors: ["octocat"],
            assignees: ["hubot"],
            labels: ["security"],
            reviewStates: [.changesRequested],
            ciResults: [.failing],
            hasConflicts: true,
            maximumAgeDays: 1,
            queueStates: [.needsWork],
            activeCodexWork: true
        )
        XCTAssertTrue(filter.matches(candidate, now: base.addingTimeInterval(3_600)))

        var mismatches: [PullRequestQueueRecord] = []
        var mismatch = candidate
        mismatch.open = false
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.draft = false
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.author = "someone-else"
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.assignees = []
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.labels = []
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.reviewState = .approved
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.ciHealth = .passing
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.hasConflicts = false
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.updatedAt = base.addingTimeInterval(-172_800)
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.queueState = .ready
        mismatches.append(mismatch)
        mismatch = candidate
        mismatch.activeCodexWork = false
        mismatches.append(mismatch)
        XCTAssertTrue(mismatches.allSatisfy { !filter.matches($0, now: base.addingTimeInterval(3_600)) })

        let preferences = WorkspacePresentationPreferences(
            repositorySort: RepositorySort(key: .recentlyPushed, direction: .descending),
            pullRequestSort: PullRequestSort(key: .latestHeadCommit, direction: .descending),
            filter: filter
        )
        let encoded = try JSONEncoder().encode(preferences)
        XCTAssertEqual(try JSONDecoder().decode(WorkspacePresentationPreferences.self, from: encoded), preferences)
    }

    func testEmptyFilterMatchesEveryPullRequest() {
        XCTAssertTrue(WorkspaceFilter().matches(pullRequest(id: 1, number: 1), now: base))
    }
}
