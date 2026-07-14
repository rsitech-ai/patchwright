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
    public let expectedHeadSha: String?
    public let method: String?
    public init(commentNumber: UInt64, body: String) {
        kind = "comment"
        issueNumber = commentNumber
        self.body = body
        pullRequestNumber = nil
        expectedHeadSha = nil
        method = nil
    }
    public init(pullRequestNumber: UInt64, expectedHeadSha: String, method: GitHubMergeMethod) {
        kind = "mergePullRequest"
        issueNumber = nil
        body = nil
        self.pullRequestNumber = pullRequestNumber
        self.expectedHeadSha = expectedHeadSha
        self.method = method.rawValue
    }
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
}

public struct DeliveryExecution: Codable, Equatable, Sendable {
    public let idempotencyKey: String
    public let state: String
    public let result: GitHubMutationResult
}
