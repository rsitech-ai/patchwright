import Foundation

public enum CodexRuntimeState: String, Codable, Sendable {
    case unavailable, notStarted, ready, staleThreadNeedsConfirmation, failed, exited
}

public enum CodexAccountState: String, Codable, Sendable {
    case signedIn, signedOut, unavailable
}

public struct CodexRuntimeStatus: Codable, Equatable, Sendable {
    public let taskId: UUID
    public let state: CodexRuntimeState
    public let processGeneration: UUID?
    public let accountState: CodexAccountState?
    public let threadId: String?
    public let turnId: String?
    public let lastSequence: UInt64
    public let canStart: Bool
    public let canSend: Bool
    public let canSteer: Bool

    public var taskID: UUID { taskId }
    public var threadID: String? { threadId }
    public var turnID: String? { turnId }
}

public enum CodexEventKind: Equatable, Hashable, Sendable {
    case processStarted, initialized, accountRead, threadReady, threadStale
    case userMessage, userSteer, itemStarted, itemCompleted
    case textDelta, reasoningDelta, commandOutputDelta, fileChangeDelta
    case turnCompleted, error
    case unknown(String)

    public init(rawValue: String) {
        self = switch rawValue {
        case "processStarted": .processStarted
        case "initialized": .initialized
        case "accountRead": .accountRead
        case "threadReady": .threadReady
        case "threadStale": .threadStale
        case "userMessage": .userMessage
        case "userSteer": .userSteer
        case "itemStarted": .itemStarted
        case "itemCompleted": .itemCompleted
        case "textDelta": .textDelta
        case "reasoningDelta": .reasoningDelta
        case "commandOutputDelta": .commandOutputDelta
        case "fileChangeDelta": .fileChangeDelta
        case "turnCompleted": .turnCompleted
        case "error": .error
        default: .unknown(rawValue)
        }
    }

    public var rawValue: String {
        switch self {
        case .processStarted: "processStarted"
        case .initialized: "initialized"
        case .accountRead: "accountRead"
        case .threadReady: "threadReady"
        case .threadStale: "threadStale"
        case .userMessage: "userMessage"
        case .userSteer: "userSteer"
        case .itemStarted: "itemStarted"
        case .itemCompleted: "itemCompleted"
        case .textDelta: "textDelta"
        case .reasoningDelta: "reasoningDelta"
        case .commandOutputDelta: "commandOutputDelta"
        case .fileChangeDelta: "fileChangeDelta"
        case .turnCompleted: "turnCompleted"
        case .error: "error"
        case .unknown(let value): value
        }
    }
}

extension CodexEventKind: Codable {
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        self.init(rawValue: try container.decode(String.self))
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct CodexEvent: Codable, Equatable, Identifiable, Sendable {
    public let taskId: UUID
    public let processGeneration: UUID
    public let sequence: UInt64
    public let kind: CodexEventKind
    public let summary: String
    public let threadId: String?
    public let turnId: String?
    public let itemId: String?
    public let content: String?
    public let occurredAt: Date

    public var id: UInt64 { sequence }
    public var taskID: UUID { taskId }
    public var threadID: String? { threadId }
    public var turnID: String? { turnId }
    public var itemID: String? { itemId }

    public init(
        taskId: UUID,
        processGeneration: UUID,
        sequence: UInt64,
        kind: CodexEventKind,
        summary: String,
        threadId: String? = nil,
        turnId: String? = nil,
        itemId: String? = nil,
        content: String? = nil,
        occurredAt: Date
    ) {
        self.taskId = taskId
        self.processGeneration = processGeneration
        self.sequence = sequence
        self.kind = kind
        self.summary = summary
        self.threadId = threadId
        self.turnId = turnId
        self.itemId = itemId
        self.content = content
        self.occurredAt = occurredAt
    }
}

public struct CodexTurnReceipt: Codable, Equatable, Sendable {
    public let threadId: String
    public let turnId: String
    public let clientMessageId: String
    public var threadID: String { threadId }
    public var turnID: String { turnId }
}

public enum CodexApprovalKind: String, Codable, Sendable { case command, fileChange }
public enum CodexApprovalState: String, Codable, Sendable { case pending, approved, declined, expired, invalidated }

public struct CodexRuntimeApproval: Codable, Equatable, Identifiable, Sendable {
    public let id: UUID
    public let taskId: UUID
    public let `class`: String
    public let requestId: JSONRequestID
    public let processGeneration: UUID
    public let threadId: String
    public let turnId: String
    public let itemId: String
    public let kind: CodexApprovalKind
    public let reason: String?
    public let command: String?
    public let cwd: String?
    public let grantRoot: String?
    public let state: CodexApprovalState
    public let createdAt: Date
    public let expiresAt: Date
    public let decidedAt: Date?
}

public enum JSONRequestID: Codable, Equatable, Sendable {
    case number(Int64), string(String)

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let value = try? container.decode(Int64.self) { self = .number(value) }
        else { self = .string(try container.decode(String.self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self { case .number(let value): try container.encode(value); case .string(let value): try container.encode(value) }
    }
}

public enum CodexTranscriptItemKind: Equatable, Hashable, Sendable {
    case operatorMessage, agentMessage, reasoning, command, fileChange, status
    case unknown(String)
}

public struct CodexTranscriptItem: Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let kind: CodexTranscriptItemKind
    public let itemId: String?
    public let threadId: String?
    public let turnId: String?
    public let occurredAt: Date
    public private(set) var content: String
    public var itemID: String? { itemId }
    public var threadID: String? { threadId }
    public var turnID: String? { turnId }

    fileprivate mutating func append(_ delta: String) {
        content.append(delta)
    }
}

public struct CodexTranscript: Equatable, Sendable {
    public let items: [CodexTranscriptItem]
    public let cursor: UInt64

    public init(events: [CodexEvent]) {
        let ordered = events.sorted { $0.sequence < $1.sequence }
        cursor = ordered.last?.sequence ?? 0
        var items: [CodexTranscriptItem] = []
        var streamingIndex: [StreamingKey: Int] = [:]
        for event in ordered {
            guard let presentationKind = event.kind.presentationKind else { continue }
            let content = event.content ?? event.summary
            if event.kind.isStreaming, let itemId = event.itemId {
                let key = StreamingKey(kind: presentationKind, itemId: itemId)
                if let index = streamingIndex[key] {
                    items[index].append(content)
                    continue
                }
                streamingIndex[key] = items.count
            }
            items.append(
                CodexTranscriptItem(
                    id: event.sequence,
                    kind: presentationKind,
                    itemId: event.itemId,
                    threadId: event.threadId,
                    turnId: event.turnId,
                    occurredAt: event.occurredAt,
                    content: content
                )
            )
        }
        self.items = items
    }
}

private struct StreamingKey: Hashable {
    let kind: CodexTranscriptItemKind
    let itemId: String
}

private extension CodexEventKind {
    var presentationKind: CodexTranscriptItemKind? {
        switch self {
        case .userMessage, .userSteer: .operatorMessage
        case .textDelta: .agentMessage
        case .reasoningDelta: .reasoning
        case .commandOutputDelta: .command
        case .fileChangeDelta: .fileChange
        case .turnCompleted, .error, .threadStale: .status
        case .unknown(let value): .unknown(value)
        case .processStarted, .initialized, .accountRead, .threadReady, .itemStarted, .itemCompleted:
            nil
        }
    }

    var isStreaming: Bool {
        switch self {
        case .textDelta, .reasoningDelta, .commandOutputDelta, .fileChangeDelta: true
        default: false
        }
    }
}
