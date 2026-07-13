import Foundation

public enum TaskState: String, Codable, CaseIterable, Sendable {
    case discovered, assessing, planned, awaitingPreparationApproval, preparing, implementing
    case verifying, reviewing, awaitingDeliveryApproval, delivering, monitoring
    case awaitingMergeApproval, merging, paused, blocked, completed, failed, cancelled

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let value = try container.decode(String.self)
        if value == "awaitingApproval" {
            self = .awaitingPreparationApproval
        } else if let state = Self(rawValue: value) {
            self = state
        } else {
            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Unknown task state \(value)"
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }

    public var requiresAttention: Bool {
        switch self {
        case .awaitingPreparationApproval, .awaitingDeliveryApproval, .awaitingMergeApproval,
             .blocked, .failed:
            true
        default:
            false
        }
    }
}

public struct TaskInterruption: Codable, Hashable, Sendable {
    public let state: TaskState
    public let resumeState: TaskState
    public let reason: String
}

public struct GitHubIssueTaskSource: Codable, Hashable, Sendable {
    public let repositoryId: UInt64
    public let repositoryFullName: String
    public let number: UInt64
    public let htmlUrl: String
    public let snapshotAt: Date
    public var repositoryID: UInt64 { repositoryId }
    public var htmlURL: String { htmlUrl }
}

public struct GitHubPullRequestTaskSource: Codable, Hashable, Sendable {
    public let repositoryId: UInt64
    public let repositoryFullName: String
    public let number: UInt64
    public let htmlUrl: String
    public let snapshotAt: Date
    public let baseRef: String
    public let baseSha: String
    public let headRef: String
    public let headSha: String
    public var repositoryID: UInt64 { repositoryId }
    public var htmlURL: String { htmlUrl }
    public var baseSHA: String { baseSha }
    public var headSHA: String { headSha }
}

public enum TaskSource: Codable, Hashable, Sendable {
    case localRequest
    case githubIssue(GitHubIssueTaskSource)
    case githubPullRequest(GitHubPullRequestTaskSource)

    private enum CodingKeys: String, CodingKey { case kind, details }
    private enum Kind: String, Codable { case localRequest, githubIssue, githubPullRequest }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .kind) {
        case .localRequest:
            self = .localRequest
        case .githubIssue:
            self = .githubIssue(try container.decode(GitHubIssueTaskSource.self, forKey: .details))
        case .githubPullRequest:
            self = .githubPullRequest(
                try container.decode(GitHubPullRequestTaskSource.self, forKey: .details)
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .localRequest:
            try container.encode(Kind.localRequest, forKey: .kind)
        case .githubIssue(let source):
            try container.encode(Kind.githubIssue, forKey: .kind)
            try container.encode(source, forKey: .details)
        case .githubPullRequest(let source):
            try container.encode(Kind.githubPullRequest, forKey: .kind)
            try container.encode(source, forKey: .details)
        }
    }
}

public struct EngineeringTask: Codable, Identifiable, Hashable, Sendable {
    public let id: UUID
    public let title: String
    public let repositoryPath: String
    public let state: TaskState
    public let createdAt: Date
    public let updatedAt: Date
    public let interruption: TaskInterruption?
    public let source: TaskSource?
    public let repositoryBindingId: UUID?
    public let contractVersion: UInt32?
    public let checkpointId: UUID?
    public var repositoryBindingID: UUID? { repositoryBindingId }
    public var checkpointID: UUID? { checkpointId }

    public var requiresAttention: Bool {
        state.requiresAttention
    }
}

public struct HealthResponse: Codable, Sendable {
    public let status: String
    public let version: String
}

public struct EmptyParameters: Codable, Sendable { public init() {} }

public enum EngineConnectionState: Equatable, Sendable {
    case disconnected, connecting, connected(String), failed(String)
}

public extension JSONDecoder {
    static var patchwright: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }
}
