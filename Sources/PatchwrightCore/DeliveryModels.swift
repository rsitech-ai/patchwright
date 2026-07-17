import Foundation

public struct GitHubRemoteIdentity: Codable, Equatable, Sendable {
    public let repositoryId: UInt64
    public let installationId: UInt64
    public let repositoryFullName: String
    public init(repositoryId: UInt64, installationId: UInt64, repositoryFullName: String) {
        self.repositoryId = repositoryId
        self.installationId = installationId
        self.repositoryFullName = repositoryFullName
    }
}

public struct GitHubActionPayload: Codable, Equatable, Sendable {
    public let kind: String
    public let issueNumber: UInt64?
    public let body: String?
    public let pullRequestNumber: UInt64?
    public let threadId: String?
    public let expectedHeadSha: String?
    public let method: String?
    public let branch: String?
    public let fromSha: String?
    public let headSha: String?
    public let event: String?
    public let inlineComments: [GitHubInlineReviewComment]?
    public let name: String?
    public let status: String?
    public let conclusion: String?
    public let title: String?
    public let head: String?
    public let base: String?
    public let expectedBaseSha: String?

    public init(
        kind: String,
        issueNumber: UInt64? = nil,
        body: String? = nil,
        pullRequestNumber: UInt64? = nil,
        threadId: String? = nil,
        expectedHeadSha: String? = nil,
        method: String? = nil,
        branch: String? = nil,
        fromSha: String? = nil,
        headSha: String? = nil,
        event: String? = nil,
        inlineComments: [GitHubInlineReviewComment]? = nil,
        name: String? = nil,
        status: String? = nil,
        conclusion: String? = nil,
        title: String? = nil,
        head: String? = nil,
        base: String? = nil,
        expectedBaseSha: String? = nil
    ) {
        self.kind = kind
        self.issueNumber = issueNumber
        self.body = body
        self.pullRequestNumber = pullRequestNumber
        self.threadId = threadId
        self.expectedHeadSha = expectedHeadSha
        self.method = method
        self.branch = branch
        self.fromSha = fromSha
        self.headSha = headSha
        self.event = event
        self.inlineComments = inlineComments
        self.name = name
        self.status = status
        self.conclusion = conclusion
        self.title = title
        self.head = head
        self.base = base
        self.expectedBaseSha = expectedBaseSha
    }
    public init(commentNumber: UInt64, body: String) {
        self.init(kind: "comment", issueNumber: commentNumber, body: body)
    }
    public init(pullRequestNumber: UInt64, expectedHeadSha: String, method: GitHubMergeMethod) {
        self.init(
            kind: "mergePullRequest",
            pullRequestNumber: pullRequestNumber,
            expectedHeadSha: expectedHeadSha,
            method: method.rawValue
        )
    }
}

public struct GitHubInlineReviewComment: Codable, Equatable, Sendable {
    public let path: String
    public let line: UInt64
    public let body: String
}

public enum GitHubReviewEvent: String, Codable, CaseIterable, Identifiable, Sendable {
    case approve, requestChanges, comment
    public var id: String { rawValue }
    public var label: String {
        switch self {
        case .approve: "Approve"
        case .requestChanges: "Request changes"
        case .comment: "Comment"
        }
    }
}

public struct WorktreeInspection: Codable, Equatable, Sendable {
    public let root: String
    public let branch: String
    public let headSha: String
    public let dirty: Bool
    public var headSHA: String { headSha }
}

public enum GitHubMergeMethod: String, Codable, CaseIterable, Identifiable, Sendable {
    case merge, squash, rebase
    public var id: String { rawValue }
    public var label: String { rawValue.capitalized }
}

public struct GitHubActionPreviewDraft: Codable, Equatable, Sendable {
    public let remote: GitHubRemoteIdentity
    public let action: GitHubActionPayload
    public let expectedHeadSha: String?
    public let expectedBaseSha: String?
    public let snapshotGeneration: UInt64
}

public struct GitHubRemotePrecondition: Codable, Equatable, Sendable {
    public let expectedHeadSha: String?
    public let expectedBaseSha: String?
    public let snapshotGeneration: UInt64
}

public struct GitHubActionPreview: Codable, Equatable, Sendable {
    public let remote: GitHubRemoteIdentity
    public let action: GitHubActionPayload
    public let precondition: GitHubRemotePrecondition
    public let payloadSha256: String
    public let idempotencySha256: String
    public let requiredPermissions: [String]
}

public struct DeliveryFingerprint: Codable, Equatable, Sendable {
    public let taskId: UUID
    public let githubRepositoryId: UInt64
    public let repositoryFullName: String
    public let actionKind: String
    public let pullRequestNumber: UInt64?
    public let branch: String?
    public let headSha: String?
    public let baseSha: String?
    public let payloadSha256: String
    public let policySha256: String
    public let instructionSha256: String
    public let invalidationGeneration: UInt64
}

public struct DeliveryPreview: Codable, Equatable, Sendable {
    public let taskId: UUID
    public let action: GitHubActionPreview
    public let fingerprint: DeliveryFingerprint
}

public struct DeliveryApproval: Codable, Equatable, Sendable {
    public let id: UUID
    public let `class`: String
    public let capability: String
    public let fingerprint: DeliveryFingerprint
    public let approvedBy: String
    public let approvedAt: Date
    public let expiresAt: Date
}

public struct GitHubMutationResult: Codable, Equatable, Sendable {
    public let id: UInt64?
    public let number: UInt64?
    public let htmlUrl: String?
    public let sha: String?
    public let merged: Bool?
    public let nodeId: String?
    public let resolved: Bool?
}

public struct DeliveryExecution: Codable, Equatable, Sendable {
    public let idempotencyKey: String
    public let state: String
    public let result: GitHubMutationResult
}
