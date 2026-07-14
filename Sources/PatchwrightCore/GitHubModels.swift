import Foundation

public struct GitHubAccount: Codable, Equatable, Sendable {
    public let login: String
    public let avatarUrl: String
    public let htmlUrl: String
    public var avatarURL: String { avatarUrl }
    public var htmlURL: String { htmlUrl }
}

public struct GitHubRepository: Codable, Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let fullName: String
    public let description: String?
    public let `private`: Bool
    public let archived: Bool
    public let defaultBranch: String
    public let htmlUrl: String
    public let updatedAt: Date
    public let pushedAt: Date?
    public let openIssuesCount: UInt64
    public let openPullRequestCount: UInt64?
    public let failingCheckCount: UInt64?
    public let defaultBranchSha: String?
    public let defaultBranchCommittedAt: Date?
    public let installationId: UInt64?
    public let permissions: GitHubRepositoryPermissions?
    public var htmlURL: String { htmlUrl }
    public var defaultBranchSHA: String? { defaultBranchSha }
    public var installationID: UInt64? { installationId }
}

public struct GitHubRepositoryPermissions: Codable, Equatable, Sendable {
    public let admin: Bool
    public let maintain: Bool
    public let push: Bool
    public let triage: Bool
    public let pull: Bool
}

public enum GitHubWorkItemKind: String, Codable, Sendable { case issue, pullRequest }

public struct GitHubWorkItem: Codable, Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let repositoryFullName: String
    public let number: UInt64
    public let kind: GitHubWorkItemKind
    public let title: String
    public let state: String
    public let body: String?
    public let author: String
    public let htmlUrl: String
    public let draft: Bool
    public let commentsCount: UInt64
    public let baseRef: String?
    public let baseSha: String?
    public let headRef: String?
    public let headSha: String?
    public let createdAt: Date?
    public let headCommittedAt: Date?
    public let latestReviewAt: Date?
    public let updatedAt: Date
    public let reviewDecision: String?
    public let ciHealth: String?
    public let mergeable: Bool?
    public let mergeableState: String?
    public let rebaseable: Bool?
    public let hasConflicts: Bool?
    public let headRepositoryFullName: String?
    public let headRepositoryFork: Bool?
    public let maintainerCanModify: Bool?
    public let additions: UInt64?
    public let deletions: UInt64?
    public let changedFiles: UInt64?
    public let labels: [String]
    public let assignees: [String]
    public let milestone: String?
    public var baseSHA: String? { baseSha }
    public var headSHA: String? { headSha }
    public var htmlURL: String { htmlUrl }
}

public struct GitHubDiscussion: Codable, Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let itemNumber: UInt64
    public let kind: String
    public let author: String
    public let body: String?
    public let state: String?
    public let path: String?
    public let line: UInt64?
    public let htmlUrl: String
    public let updatedAt: String?
    public var htmlURL: String { htmlUrl }
}

public struct GitHubCheckRun: Codable, Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let itemNumber: UInt64
    public let name: String
    public let status: String
    public let conclusion: String?
    public let htmlUrl: String?
    public var htmlURL: String? { htmlUrl }
}

public struct GitHubWorkflowRun: Codable, Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let name: String?
    public let status: String?
    public let conclusion: String?
    public let event: String
    public let headSha: String
    public let htmlUrl: String
    public let updatedAt: String
    public var htmlURL: String { htmlUrl }
}

public struct GitHubRepositorySnapshot: Codable, Equatable, Sendable {
    public let repository: GitHubRepository
    public let workItems: [GitHubWorkItem]
    public let discussions: [GitHubDiscussion]
    public let checks: [GitHubCheckRun]
    public let workflowRuns: [GitHubWorkflowRun]
}

public struct GitHubStatus: Codable, Equatable, Sendable {
    public let connected: Bool
    public let account: GitHubAccount?
    public let repositoryCount: Int
    public let lastSyncedAt: String?
}

public struct GitHubSyncSummary: Codable, Equatable, Sendable {
    public let account: GitHubAccount
    public let repositoriesDiscovered: Int
    public let repositoriesSynced: Int
    public let workItems: Int
    public let discussions: Int
    public let checks: Int
    public let workflowRuns: Int
    public let failures: [String]
}

public enum GitHubSyncJobState: String, Codable, Sendable {
    case queued, running, cancelling, cancelled, succeeded, failed, interrupted

    public var isTerminal: Bool {
        switch self {
        case .cancelled, .succeeded, .failed, .interrupted: true
        case .queued, .running, .cancelling: false
        }
    }
}

public struct GitHubSyncJob: Codable, Identifiable, Equatable, Sendable {
    public let id: UUID
    public let kind: String
    public let state: GitHubSyncJobState
    public let cancellation: String
    public let summary: String
    public let createdAt: Date
    public let updatedAt: Date
    public let generation: UInt64
}
