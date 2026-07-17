import Foundation

public struct PreparationPreview: Codable, Equatable, Identifiable, Sendable {
    public let taskId: UUID
    public let repositoryBindingId: UUID
    public let repositoryFullName: String
    public let repositoryPath: String
    public let sourceSha: String
    public let worktreePath: String
    public let branch: String
    public let invalidationGeneration: UInt64
    public let policySha256: String
    public let instructionSha256: String
    public let contract: TaskContract
    public let fingerprint: DeliveryFingerprint

    public var id: UUID { taskId }
}

public struct PreparationApproval: Codable, Equatable, Sendable {
    public let id: UUID
    public let `class`: String
    public let capability: String
    public let fingerprint: DeliveryFingerprint
    public let approvedBy: String
    public let approvedAt: Date
    public let expiresAt: Date
}
