import Foundation

public enum TaskState: String, Codable, CaseIterable, Sendable {
    case discovered, planned, awaitingApproval, preparing, implementing, verifying, reviewing
    case awaitingDeliveryApproval, delivering, monitoring, completed, failed, cancelled
}

public struct EngineeringTask: Codable, Identifiable, Hashable, Sendable {
    public let id: UUID
    public let title: String
    public let repositoryPath: String
    public let state: TaskState
    public let createdAt: Date
    public let updatedAt: Date

    public var requiresAttention: Bool {
        state == .awaitingApproval || state == .awaitingDeliveryApproval || state == .failed
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

