import Foundation
import Network

public enum EngineError: Error, LocalizedError, Sendable {
    case connectionFailed(String)
    case invalidResponse
    case timedOut
    case remote(code: Int, message: String)

    public var errorDescription: String? {
        switch self {
        case .connectionFailed(let message): message
        case .invalidResponse: "The engine returned an invalid response."
        case .timedOut: "The engine did not respond before the request timed out."
        case .remote(_, let message): message
        }
    }
}

public protocol EngineServing: Sendable {
    func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String],
        as type: Result.Type
    ) async throws -> Result
}

public extension EngineServing {
    func previewDelivery(taskID: UUID, draft: GitHubActionPreviewDraft) async throws -> DeliveryPreview {
        try await call(
            method: "delivery.preview",
            params: ["taskId": taskID.uuidString, "actionPreview": try encodeRPCParameter(draft)],
            as: DeliveryPreview.self
        )
    }

    func approveDelivery(_ preview: DeliveryPreview, approvedBy: String) async throws -> DeliveryApproval {
        try await call(
            method: "delivery.approve",
            params: ["preview": try encodeRPCParameter(preview), "approvedBy": approvedBy],
            as: DeliveryApproval.self
        )
    }

    func executeDelivery(_ preview: DeliveryPreview, approvalID: UUID) async throws -> DeliveryExecution {
        try await call(
            method: "delivery.execute",
            params: ["preview": try encodeRPCParameter(preview), "approvalId": approvalID.uuidString],
            as: DeliveryExecution.self
        )
    }

    func codexStatus(taskID: UUID) async throws -> CodexRuntimeStatus {
        try await call(
            method: "codex.status",
            params: ["taskId": taskID.uuidString],
            as: CodexRuntimeStatus.self
        )
    }

    func startCodex(taskID: UUID) async throws -> CodexRuntimeStatus {
        try await call(
            method: "codex.start",
            params: ["taskId": taskID.uuidString],
            as: CodexRuntimeStatus.self
        )
    }

    func codexEvents(taskID: UUID, after cursor: UInt64, limit: Int = 100) async throws -> [CodexEvent] {
        try await call(
            method: "codex.events",
            params: [
                "taskId": taskID.uuidString,
                "after": String(cursor),
                "limit": String(limit),
            ],
            as: [CodexEvent].self
        )
    }

    func startCodexTurn(
        taskID: UUID,
        clientMessageID: UUID,
        input: String
    ) async throws -> CodexTurnReceipt {
        try await call(
            method: "codex.turn.start",
            params: [
                "taskId": taskID.uuidString,
                "clientMessageId": clientMessageID.uuidString,
                "input": input,
            ],
            as: CodexTurnReceipt.self
        )
    }

    func steerCodexTurn(
        taskID: UUID,
        clientMessageID: UUID,
        input: String
    ) async throws -> CodexTurnReceipt {
        try await call(
            method: "codex.turn.steer",
            params: [
                "taskId": taskID.uuidString,
                "clientMessageId": clientMessageID.uuidString,
                "input": input,
            ],
            as: CodexTurnReceipt.self
        )
    }

    func codexApprovals(taskID: UUID) async throws -> [CodexRuntimeApproval] {
        try await call(method: "codex.approvals", params: ["taskId": taskID.uuidString], as: [CodexRuntimeApproval].self)
    }

    func resolveCodexApproval(taskID: UUID, approvalID: UUID, processGeneration: UUID, approve: Bool) async throws -> CodexRuntimeApproval {
        try await call(
            method: "codex.approval.resolve",
            params: ["taskId": taskID.uuidString, "approvalId": approvalID.uuidString, "processGeneration": processGeneration.uuidString, "approve": String(approve)],
            as: CodexRuntimeApproval.self
        )
    }

    func interruptCodex(taskID: UUID, cancel: Bool) async throws -> CodexRuntimeStatus {
        try await call(method: cancel ? "codex.cancel" : "codex.pause", params: ["taskId": taskID.uuidString], as: CodexRuntimeStatus.self)
    }

    func previewTaskFromGitHub(_ item: GitHubWorkItem) async throws -> ConversionPreview {
        try await call(
            method: "task.previewFromGitHub",
            params: conversionParameters(for: item),
            as: ConversionPreview.self
        )
    }

    func createTaskFromGitHub(_ item: GitHubWorkItem) async throws -> ConversionOutcome {
        try await call(
            method: "task.createFromGitHub",
            params: conversionParameters(for: item),
            as: ConversionOutcome.self
        )
    }

    func planTask(taskID: UUID) async throws -> EngineeringTask {
        try await call(
            method: "task.plan",
            params: ["taskId": taskID.uuidString],
            as: EngineeringTask.self
        )
    }

    func taskContract(taskID: UUID) async throws -> TaskContract {
        try await call(
            method: "task.contract",
            params: ["taskId": taskID.uuidString],
            as: TaskContract.self
        )
    }

    func previewPreparation(taskID: UUID) async throws -> PreparationPreview {
        try await call(
            method: "task.preparation.preview",
            params: ["taskId": taskID.uuidString],
            as: PreparationPreview.self
        )
    }

    func approvePreparation(
        _ preview: PreparationPreview,
        approvedBy: String
    ) async throws -> PreparationApproval {
        try await call(
            method: "task.preparation.approve",
            params: ["preview": try encodeRPCParameter(preview), "approvedBy": approvedBy],
            as: PreparationApproval.self
        )
    }

    func prepareTask(
        preview: PreparationPreview,
        approvalID: UUID
    ) async throws -> EngineeringTask {
        try await call(
            method: "task.prepare",
            params: [
                "taskId": preview.taskId.uuidString,
                "preview": try encodeRPCParameter(preview),
                "approvalId": approvalID.uuidString,
            ],
            as: EngineeringTask.self
        )
    }

    func taskTimeline(taskID: UUID) async throws -> [EngineeringTask] {
        try await call(
            method: "task.timeline",
            params: ["taskId": taskID.uuidString],
            as: [EngineeringTask].self
        )
    }

    func inspectTaskWorktree(taskID: UUID) async throws -> WorktreeInspection {
        try await call(
            method: "task.worktree",
            params: ["taskId": taskID.uuidString],
            as: WorktreeInspection.self
        )
    }

    func readyTaskForDelivery(taskID: UUID) async throws -> EngineeringTask {
        try await call(
            method: "task.readyForDelivery",
            params: ["taskId": taskID.uuidString],
            as: EngineeringTask.self
        )
    }

    func reconcileTaskWithGitHub(taskID: UUID) async throws -> EngineeringTask {
        try await call(
            method: "task.reconcileGitHub",
            params: ["taskId": taskID.uuidString],
            as: EngineeringTask.self
        )
    }

    func bindRepository(_ repository: GitHubRepository) async throws -> RepositoryBindingSummary {
        guard let installationID = repository.installationID else {
            throw EngineError.remote(
                code: -32033,
                message: "Install the Patchwright GitHub App for this repository before creating tasks."
            )
        }
        let root = FileManager.default.homeDirectoryForCurrentUser
            .appending(path: ".patchwright/repositories/\(repository.id)", directoryHint: .isDirectory)
        return try await call(
            method: "repository.bind",
            params: [
                "repositoryFullName": repository.fullName,
                "installationId": String(installationID),
                "managedClone": root.appending(path: "repository", directoryHint: .isDirectory).path,
                "stateRoot": root.appending(path: "state", directoryHint: .isDirectory).path,
                "worktreeRoot": root.appending(path: "worktrees", directoryHint: .isDirectory).path,
            ],
            as: RepositoryBindingSummary.self
        )
    }

    func syncRepositoryWithGitHubApp(_ repository: GitHubRepository) async throws -> GitHubRepositorySnapshot {
        try await call(
            method: "github.sync.repository",
            params: [
                "fullName": repository.fullName,
                "repositoryId": String(repository.id),
                "resourceLimit": "1000",
            ],
            as: GitHubRepositorySnapshot.self
        )
    }

    func githubRepositorySnapshot(fullName: String) async throws -> GitHubRepositorySnapshot? {
        try await call(
            method: "github.repository",
            params: ["fullName": fullName],
            as: GitHubRepositorySnapshot?.self
        )
    }
}

private func encodeRPCParameter<Value: Encodable>(_ value: Value) throws -> String {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    return String(decoding: try encoder.encode(value), as: UTF8.self)
}

private func conversionParameters(for item: GitHubWorkItem) -> [String: String] {
    [
        "repositoryFullName": item.repositoryFullName,
        "itemNumber": String(item.number),
        "expectedUpdatedAt": ISO8601DateFormatter().string(from: item.updatedAt),
    ]
}

private struct RPCRequest: Encodable {
    let jsonrpc = "2.0"
    let id: String
    let method: String
    let params: [String: String]
}

private struct RPCResponse<Result: Decodable>: Decodable {
    struct Failure: Decodable { let code: Int; let message: String }
    let result: Result?
    let error: Failure?
}

struct JSONLineFramer: Sendable {
    private var buffer = Data()
    private let maximumBytes: Int

    init(maximumBytes: Int) {
        self.maximumBytes = maximumBytes
    }

    mutating func append(_ chunk: Data) throws -> Data? {
        guard chunk.count <= maximumBytes, buffer.count <= maximumBytes - chunk.count else {
            throw EngineError.invalidResponse
        }
        buffer.append(chunk)
        guard let newline = buffer.firstIndex(of: 0x0A) else { return nil }
        return Data(buffer[..<newline])
    }
}

public actor UnixEngineClient: EngineServing {
    private let socketPath: String
    private let timeout: Duration
    private let longRunningTimeout: Duration
    private let maximumResponseBytes: Int

    public init(
        socketPath: String,
        timeout: Duration = .seconds(10),
        // Planning emits at most two verification commands; the engine permits five minutes each.
        longRunningTimeout: Duration = .seconds(11 * 60),
        maximumResponseBytes: Int = 64 * 1_024 * 1_024
    ) {
        self.socketPath = socketPath
        self.timeout = max(timeout, .milliseconds(1))
        self.longRunningTimeout = max(longRunningTimeout, .milliseconds(1))
        self.maximumResponseBytes = max(1, maximumResponseBytes)
    }

    public func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String] = [:],
        as type: Result.Type
    ) async throws -> Result {
        var payload = try JSONEncoder().encode(RPCRequest(id: UUID().uuidString, method: method, params: params))
        payload.append(0x0A)
        let requestTimeout = method == "task.readyForDelivery" ? longRunningTimeout : timeout
        let data = try await exchange(payload, timeout: requestTimeout)
        let response: RPCResponse<Result>
        do {
            response = try JSONDecoder.patchwright.decode(RPCResponse<Result>.self, from: data)
        } catch {
            throw EngineError.invalidResponse
        }
        if let result = response.result { return result }
        if let error = response.error { throw EngineError.remote(code: error.code, message: error.message) }
        throw EngineError.invalidResponse
    }

    private func exchange(_ payload: Data, timeout: Duration) async throws -> Data {
        let path = socketPath
        let maximumResponseBytes = maximumResponseBytes
        let operation = EngineExchangeOperation()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                operation.install(continuation)
                guard !Task.isCancelled else {
                    operation.finish(.failure(CancellationError()))
                    return
                }
                let connection = NWConnection(to: .unix(path: path), using: .tcp)
                guard operation.attach(connection) else { return }
                let queue = DispatchQueue(label: "ai.patchwright.engine-client")
                connection.stateUpdateHandler = { state in
                    switch state {
                    case .ready:
                        connection.stateUpdateHandler = nil
                        connection.send(content: payload, completion: .contentProcessed { error in
                            if let error {
                                operation.finish(
                                    .failure(EngineError.connectionFailed(error.localizedDescription))
                                )
                                return
                            }
                            receiveJSONLine(
                                connection: connection,
                                framer: JSONLineFramer(maximumBytes: maximumResponseBytes)
                            ) { result in
                                operation.finish(result)
                            }
                        })
                    case .failed(let error):
                        operation.finish(.failure(EngineError.connectionFailed(error.localizedDescription)))
                    case .cancelled:
                        operation.finish(
                            .failure(EngineError.connectionFailed("The engine connection was cancelled."))
                        )
                    case .setup, .preparing, .waiting:
                        break
                    @unknown default:
                        break
                    }
                }
                operation.attachTimeoutTask(Task {
                    do {
                        try await Task.sleep(for: timeout)
                    } catch {
                        return
                    }
                    operation.finish(.failure(EngineError.timedOut))
                })
                connection.start(queue: queue)
            }
        } onCancel: {
            operation.finish(.failure(CancellationError()))
        }
    }
}

private final class EngineExchangeOperation: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Data, Error>?
    private var connection: NWConnection?
    private var timeoutTask: Task<Void, Never>?
    private var pendingResult: Result<Data, Error>?
    private var finished = false

    func install(_ continuation: CheckedContinuation<Data, Error>) {
        lock.lock()
        if let pendingResult {
            lock.unlock()
            continuation.resume(with: pendingResult)
        } else {
            self.continuation = continuation
            lock.unlock()
        }
    }

    func attach(_ connection: NWConnection) -> Bool {
        lock.lock()
        guard !finished else {
            lock.unlock()
            connection.cancel()
            return false
        }
        self.connection = connection
        lock.unlock()
        return true
    }

    func attachTimeoutTask(_ task: Task<Void, Never>) {
        lock.lock()
        guard !finished else {
            lock.unlock()
            task.cancel()
            return
        }
        timeoutTask = task
        lock.unlock()
    }

    func finish(_ result: Result<Data, Error>) {
        lock.lock()
        guard !finished else {
            lock.unlock()
            return
        }
        finished = true
        let continuation = continuation
        let connection = connection
        let timeoutTask = timeoutTask
        self.continuation = nil
        self.connection = nil
        self.timeoutTask = nil
        if continuation == nil { pendingResult = result }
        lock.unlock()

        timeoutTask?.cancel()
        connection?.stateUpdateHandler = nil
        connection?.cancel()
        continuation?.resume(with: result)
    }
}

private func receiveJSONLine(
    connection: NWConnection,
    framer: JSONLineFramer,
    completion: @escaping @Sendable (Result<Data, Error>) -> Void
) {
    connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { data, _, isComplete, error in
        if let error {
            completion(.failure(EngineError.connectionFailed(error.localizedDescription)))
            return
        }
        var next = framer
        do {
            if let data, let line = try next.append(data) {
                completion(.success(line))
            } else if isComplete {
                completion(.failure(EngineError.invalidResponse))
            } else {
                receiveJSONLine(connection: connection, framer: next, completion: completion)
            }
        } catch {
            completion(.failure(error))
        }
    }
}
