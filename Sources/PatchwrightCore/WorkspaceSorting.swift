import Foundation

public enum SortDirection: String, Codable, Equatable, Sendable {
    case ascending
    case descending
}

public enum RepositorySortKey: String, Codable, Equatable, Sendable {
    case queuePriority
    case recentlyUpdated
    case recentlyPushed
    case latestDefaultBranchCommit
    case openPullRequestCount
    case failingCheckCount
    case name
}

public struct RepositorySort: Codable, Equatable, Sendable {
    public var key: RepositorySortKey
    public var direction: SortDirection

    public init(key: RepositorySortKey, direction: SortDirection) {
        self.key = key
        self.direction = direction
    }
}

public struct RepositoryQueueRecord: Codable, Equatable, Identifiable, Sendable {
    public var id: UInt64
    public var fullName: String
    public var queuePriority: Int64?
    public var updatedAt: Date
    public var pushedAt: Date?
    public var defaultBranchCommittedAt: Date?
    public var openPullRequestCount: UInt64
    public var failingCheckCount: UInt64

    public init(
        id: UInt64,
        fullName: String,
        queuePriority: Int64? = nil,
        updatedAt: Date,
        pushedAt: Date? = nil,
        defaultBranchCommittedAt: Date? = nil,
        openPullRequestCount: UInt64 = 0,
        failingCheckCount: UInt64 = 0
    ) {
        self.id = id
        self.fullName = fullName
        self.queuePriority = queuePriority
        self.updatedAt = updatedAt
        self.pushedAt = pushedAt
        self.defaultBranchCommittedAt = defaultBranchCommittedAt
        self.openPullRequestCount = openPullRequestCount
        self.failingCheckCount = failingCheckCount
    }

    public init(repository: GitHubRepository, queuePriority: Int64? = nil) {
        self.init(
            id: repository.id,
            fullName: repository.fullName,
            queuePriority: queuePriority,
            updatedAt: repository.updatedAt,
            pushedAt: repository.pushedAt,
            defaultBranchCommittedAt: repository.defaultBranchCommittedAt,
            openPullRequestCount: repository.openPullRequestCount ?? 0,
            failingCheckCount: repository.failingCheckCount ?? 0
        )
    }
}

public func sortRepositories(
    _ records: [RepositoryQueueRecord],
    by sort: RepositorySort
) -> [RepositoryQueueRecord] {
    records.sorted { left, right in
        let primary: ComparisonResult
        switch sort.key {
        case .queuePriority:
            primary = compareOptional(left.queuePriority, right.queuePriority, direction: sort.direction)
        case .recentlyUpdated:
            primary = compare(left.updatedAt, right.updatedAt, direction: sort.direction)
        case .recentlyPushed:
            primary = compareOptional(left.pushedAt, right.pushedAt, direction: sort.direction)
        case .latestDefaultBranchCommit:
            primary = compareOptional(
                left.defaultBranchCommittedAt,
                right.defaultBranchCommittedAt,
                direction: sort.direction
            )
        case .openPullRequestCount:
            primary = compare(left.openPullRequestCount, right.openPullRequestCount, direction: sort.direction)
        case .failingCheckCount:
            primary = compare(left.failingCheckCount, right.failingCheckCount, direction: sort.direction)
        case .name:
            primary = compare(left.fullName, right.fullName, direction: sort.direction)
        }
        return orderedBefore(
            primary,
            fallback: compare(left.fullName, right.fullName, direction: .ascending),
            final: compare(left.id, right.id, direction: .ascending)
        )
    }
}

public enum CiHealth: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case failing
    case pending
    case passing
    case unknown

    fileprivate var sortRank: Int? {
        switch self {
        case .failing: 1
        case .pending: 2
        case .passing: 3
        case .unknown: nil
        }
    }
}

public enum ReviewState: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case changesRequested
    case reviewRequired
    case dismissed
    case approved
    case unknown

    fileprivate var sortRank: Int? {
        switch self {
        case .changesRequested: 1
        case .reviewRequired: 2
        case .dismissed: 3
        case .approved: 4
        case .unknown: nil
        }
    }
}

public enum PullRequestQueueState: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case inbox
    case assessed
    case ready
    case needsWork
    case blocked
    case active
    case awaitingWriteApproval
    case monitoring
    case mergeReady
    case awaitingMergeApproval
    case merged
    case failed
}

public enum PullRequestSortKey: String, Codable, Equatable, Sendable {
    case queuePriority
    case recentlyUpdated
    case latestHeadCommit
    case latestReviewActivity
    case ciHealth
    case reviewState
    case createdNewest
    case createdOldest
    case changeSize
    case number
}

public struct PullRequestSort: Codable, Equatable, Sendable {
    public var key: PullRequestSortKey
    public var direction: SortDirection

    public init(key: PullRequestSortKey, direction: SortDirection) {
        self.key = key
        self.direction = direction
    }
}

public struct PullRequestQueueRecord: Codable, Equatable, Identifiable, Sendable {
    public var id: UInt64
    public var number: UInt64
    public var queuePriority: Int64?
    public var updatedAt: Date
    public var headCommittedAt: Date?
    public var latestReviewAt: Date?
    public var ciHealth: CiHealth?
    public var reviewState: ReviewState?
    public var createdAt: Date
    public var additions: UInt64
    public var deletions: UInt64
    public var open: Bool
    public var draft: Bool
    public var author: String
    public var assignees: Set<String>
    public var labels: Set<String>
    public var hasConflicts: Bool?
    public var queueState: PullRequestQueueState?
    public var activeCodexWork: Bool

    public init(
        id: UInt64,
        number: UInt64,
        queuePriority: Int64? = nil,
        updatedAt: Date,
        headCommittedAt: Date? = nil,
        latestReviewAt: Date? = nil,
        ciHealth: CiHealth? = nil,
        reviewState: ReviewState? = nil,
        createdAt: Date,
        additions: UInt64 = 0,
        deletions: UInt64 = 0,
        open: Bool = true,
        draft: Bool = false,
        author: String,
        assignees: Set<String> = [],
        labels: Set<String> = [],
        hasConflicts: Bool? = nil,
        queueState: PullRequestQueueState? = nil,
        activeCodexWork: Bool = false
    ) {
        self.id = id
        self.number = number
        self.queuePriority = queuePriority
        self.updatedAt = updatedAt
        self.headCommittedAt = headCommittedAt
        self.latestReviewAt = latestReviewAt
        self.ciHealth = ciHealth
        self.reviewState = reviewState
        self.createdAt = createdAt
        self.additions = additions
        self.deletions = deletions
        self.open = open
        self.draft = draft
        self.author = author
        self.assignees = assignees
        self.labels = labels
        self.hasConflicts = hasConflicts
        self.queueState = queueState
        self.activeCodexWork = activeCodexWork
    }

    public init(
        workItem: GitHubWorkItem,
        queuePriority: Int64? = nil,
        queueState: PullRequestQueueState? = nil,
        activeCodexWork: Bool = false
    ) {
        self.init(
            id: workItem.id,
            number: workItem.number,
            queuePriority: queuePriority,
            updatedAt: workItem.updatedAt,
            headCommittedAt: workItem.headCommittedAt,
            latestReviewAt: workItem.latestReviewAt,
            ciHealth: workItem.ciHealth.flatMap(CiHealth.init(rawValue:)),
            reviewState: workItem.reviewDecision.flatMap(ReviewState.init(rawValue:)),
            createdAt: workItem.createdAt ?? workItem.updatedAt,
            additions: workItem.additions ?? 0,
            deletions: workItem.deletions ?? 0,
            open: workItem.state.caseInsensitiveCompare("open") == .orderedSame,
            draft: workItem.draft,
            author: workItem.author,
            assignees: Set(workItem.assignees),
            labels: Set(workItem.labels),
            hasConflicts: workItem.hasConflicts,
            queueState: queueState,
            activeCodexWork: activeCodexWork
        )
    }
}

public func sortPullRequests(
    _ records: [PullRequestQueueRecord],
    by sort: PullRequestSort
) -> [PullRequestQueueRecord] {
    records.sorted { left, right in
        let primary: ComparisonResult
        switch sort.key {
        case .queuePriority:
            primary = compareOptional(left.queuePriority, right.queuePriority, direction: sort.direction)
        case .recentlyUpdated:
            primary = compare(left.updatedAt, right.updatedAt, direction: sort.direction)
        case .latestHeadCommit:
            primary = compareOptional(left.headCommittedAt, right.headCommittedAt, direction: sort.direction)
        case .latestReviewActivity:
            primary = compareOptional(left.latestReviewAt, right.latestReviewAt, direction: sort.direction)
        case .ciHealth:
            primary = compareOptional(left.ciHealth?.sortRank, right.ciHealth?.sortRank, direction: sort.direction)
        case .reviewState:
            primary = compareOptional(left.reviewState?.sortRank, right.reviewState?.sortRank, direction: sort.direction)
        case .createdNewest:
            primary = compare(left.createdAt, right.createdAt, direction: sort.direction)
        case .createdOldest:
            primary = compare(right.createdAt, left.createdAt, direction: sort.direction)
        case .changeSize:
            primary = compare(
                saturatingSum(left.additions, left.deletions),
                saturatingSum(right.additions, right.deletions),
                direction: sort.direction
            )
        case .number:
            primary = compare(left.number, right.number, direction: sort.direction)
        }
        return orderedBefore(
            primary,
            fallback: compare(left.number, right.number, direction: .ascending),
            final: compare(left.id, right.id, direction: .ascending)
        )
    }
}

public struct WorkspaceFilter: Codable, Equatable, Sendable {
    public var open: Bool?
    public var draft: Bool?
    public var authors: Set<String>
    public var assignees: Set<String>
    public var labels: Set<String>
    public var reviewStates: Set<ReviewState>
    public var ciResults: Set<CiHealth>
    public var hasConflicts: Bool?
    public var maximumAgeDays: UInt32?
    public var queueStates: Set<PullRequestQueueState>
    public var activeCodexWork: Bool?

    public init(
        open: Bool? = nil,
        draft: Bool? = nil,
        authors: Set<String> = [],
        assignees: Set<String> = [],
        labels: Set<String> = [],
        reviewStates: Set<ReviewState> = [],
        ciResults: Set<CiHealth> = [],
        hasConflicts: Bool? = nil,
        maximumAgeDays: UInt32? = nil,
        queueStates: Set<PullRequestQueueState> = [],
        activeCodexWork: Bool? = nil
    ) {
        self.open = open
        self.draft = draft
        self.authors = authors
        self.assignees = assignees
        self.labels = labels
        self.reviewStates = reviewStates
        self.ciResults = ciResults
        self.hasConflicts = hasConflicts
        self.maximumAgeDays = maximumAgeDays
        self.queueStates = queueStates
        self.activeCodexWork = activeCodexWork
    }

    public func matches(_ record: PullRequestQueueRecord, now: Date) -> Bool {
        (open == nil || record.open == open)
            && (draft == nil || record.draft == draft)
            && (authors.isEmpty || authors.contains(record.author))
            && (assignees.isEmpty || !assignees.isDisjoint(with: record.assignees))
            && (labels.isEmpty || !labels.isDisjoint(with: record.labels))
            && (reviewStates.isEmpty || record.reviewState.map(reviewStates.contains) == true)
            && (ciResults.isEmpty || record.ciHealth.map(ciResults.contains) == true)
            && (hasConflicts == nil || record.hasConflicts == hasConflicts)
            && maximumAgeDays.map {
                record.updatedAt >= now.addingTimeInterval(-Double($0) * 86_400)
            } != false
            && (queueStates.isEmpty || record.queueState.map(queueStates.contains) == true)
            && (activeCodexWork == nil || record.activeCodexWork == activeCodexWork)
    }
}

public struct WorkspacePresentationPreferences: Codable, Equatable, Sendable {
    public var repositorySort: RepositorySort
    public var pullRequestSort: PullRequestSort
    public var filter: WorkspaceFilter

    public init(
        repositorySort: RepositorySort = RepositorySort(key: .queuePriority, direction: .ascending),
        pullRequestSort: PullRequestSort = PullRequestSort(key: .queuePriority, direction: .ascending),
        filter: WorkspaceFilter = WorkspaceFilter(open: true)
    ) {
        self.repositorySort = repositorySort
        self.pullRequestSort = pullRequestSort
        self.filter = filter
    }
}

private func compare<Value: Comparable>(
    _ left: Value,
    _ right: Value,
    direction: SortDirection
) -> ComparisonResult {
    let result: ComparisonResult = if left < right {
        .orderedAscending
    } else if left > right {
        .orderedDescending
    } else {
        .orderedSame
    }
    return direction == .ascending ? result : result.reversed
}

private func saturatingSum(_ left: UInt64, _ right: UInt64) -> UInt64 {
    let result = left.addingReportingOverflow(right)
    return result.overflow ? .max : result.partialValue
}

private func compareOptional<Value: Comparable>(
    _ left: Value?,
    _ right: Value?,
    direction: SortDirection
) -> ComparisonResult {
    switch (left, right) {
    case let (.some(left), .some(right)):
        compare(left, right, direction: direction)
    case (.some, .none):
        .orderedAscending
    case (.none, .some):
        .orderedDescending
    case (.none, .none):
        .orderedSame
    }
}

private func orderedBefore(
    _ primary: ComparisonResult,
    fallback: @autoclosure () -> ComparisonResult,
    final: @autoclosure () -> ComparisonResult
) -> Bool {
    if primary != .orderedSame { return primary == .orderedAscending }
    let fallbackResult = fallback()
    if fallbackResult != .orderedSame { return fallbackResult == .orderedAscending }
    return final() == .orderedAscending
}

private extension ComparisonResult {
    var reversed: ComparisonResult {
        switch self {
        case .orderedAscending: .orderedDescending
        case .orderedDescending: .orderedAscending
        case .orderedSame: .orderedSame
        }
    }
}
