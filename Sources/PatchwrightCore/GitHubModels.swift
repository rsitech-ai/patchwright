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
    public let updatedAt: String
    public let openIssuesCount: UInt64
    public var htmlURL: String { htmlUrl }
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
    public let headSha: String?
    public let updatedAt: String
    public let labels: [String]
    public let assignees: [String]
    public let milestone: String?
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
