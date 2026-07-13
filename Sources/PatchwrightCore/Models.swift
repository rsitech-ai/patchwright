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

public struct EngineeringTask: Codable, Identifiable, Hashable, Sendable {
    public let id: UUID
    public let title: String
    public let repositoryPath: String
    public let state: TaskState
    public let createdAt: Date
    public let updatedAt: Date
    public let interruption: TaskInterruption?

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
