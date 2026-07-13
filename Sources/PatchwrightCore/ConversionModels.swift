import Foundation

public struct ConversionPreview: Codable, Equatable, Sendable {
    public let repositoryFullName: String
    public let repositoryId: UInt64
    public let repositoryBindingId: UUID
    public let itemNumber: UInt64
    public let sourceKind: GitHubWorkItemKind
    public let title: String
    public let goal: String
    public let acceptanceCriteria: [String]
    public let repositoryPath: String
    public let baseSha: String?
    public let headSha: String?
    public let sourceUpdatedAt: Date
    public let snapshotAt: Date
    public let requiresConfirmation: Bool
    public var repositoryID: UInt64 { repositoryId }
    public var repositoryBindingID: UUID { repositoryBindingId }
    public var baseSHA: String? { baseSha }
    public var headSHA: String? { headSha }
}

public struct ConversionOutcome: Codable, Equatable, Sendable {
    public let preview: ConversionPreview
    public let task: EngineeringTask
    public let created: Bool
}
