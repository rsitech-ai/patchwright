import Foundation
import Network

public enum EngineError: Error, LocalizedError, Sendable {
    case connectionFailed(String)
    case invalidResponse
    case remote(code: Int, message: String)

    public var errorDescription: String? {
        switch self {
        case .connectionFailed(let message): message
        case .invalidResponse: "The engine returned an invalid response."
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

public actor UnixEngineClient: EngineServing {
    private let socketPath: String

    public init(socketPath: String) { self.socketPath = socketPath }

    public func call<Result: Decodable & Sendable>(
        method: String,
        params: [String: String] = [:],
        as type: Result.Type
    ) async throws -> Result {
        var payload = try JSONEncoder().encode(RPCRequest(id: UUID().uuidString, method: method, params: params))
        payload.append(0x0A)
        let data = try await exchange(payload)
        let response = try JSONDecoder.patchwright.decode(RPCResponse<Result>.self, from: data)
        if let result = response.result { return result }
        if let error = response.error { throw EngineError.remote(code: error.code, message: error.message) }
        throw EngineError.invalidResponse
    }

    private func exchange(_ payload: Data) async throws -> Data {
        let path = socketPath
        return try await withCheckedThrowingContinuation { continuation in
            let connection = NWConnection(to: .unix(path: path), using: .tcp)
            let queue = DispatchQueue(label: "ai.patchwright.engine-client")
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    connection.send(content: payload, completion: .contentProcessed { error in
                        if let error {
                            connection.cancel()
                            continuation.resume(throwing: EngineError.connectionFailed(error.localizedDescription))
                            return
                        }
                        connection.receive(minimumIncompleteLength: 1, maximumLength: 1_048_576) { data, _, _, error in
                            connection.cancel()
                            if let error {
                                continuation.resume(throwing: EngineError.connectionFailed(error.localizedDescription))
                            } else if let data, let newline = data.firstIndex(of: 0x0A) {
                                continuation.resume(returning: data[..<newline])
                            } else if let data, !data.isEmpty {
                                continuation.resume(returning: data)
                            } else {
                                continuation.resume(throwing: EngineError.invalidResponse)
                            }
                        }
                    })
                case .failed(let error):
                    connection.cancel()
                    continuation.resume(throwing: EngineError.connectionFailed(error.localizedDescription))
                default: break
                }
            }
            connection.start(queue: queue)
        }
    }
}

