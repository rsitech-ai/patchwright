import Darwin
import Foundation
import XCTest
@testable import PatchwrightCore

final class UnixEngineClientTests: XCTestCase {
    func testSilentEngineTimesOutWithinConfiguredBound() async throws {
        let server = try UnixSocketServer(response: nil)
        let client = UnixEngineClient(socketPath: server.socketPath, timeout: .milliseconds(50))
        let started = ContinuousClock.now

        do {
            let _: HealthResponse = try await client.call(method: "system.health", params: [:], as: HealthResponse.self)
            XCTFail("silent engine unexpectedly returned")
        } catch EngineError.timedOut {
            XCTAssertLessThan(ContinuousClock.now - started, .seconds(1))
        } catch {
            XCTFail("unexpected error: \(error)")
        }
    }

    func testReadyForDeliveryUsesLongRunningTimeout() async throws {
        let response = Data(
            #"{"jsonrpc":"2.0","id":"test","result":{"status":"ok","version":"0.1.0"}}"#.utf8
        ) + Data([0x0A])
        let server = try UnixSocketServer(response: response, responseDelay: 0.1)
        let client = UnixEngineClient(
            socketPath: server.socketPath,
            timeout: .milliseconds(50),
            longRunningTimeout: .seconds(1)
        )

        let health: HealthResponse = try await client.call(
            method: "task.readyForDelivery",
            params: [:],
            as: HealthResponse.self
        )

        XCTAssertEqual(health.status, "ok")
    }

    func testCancellingCallCancelsThePendingSocketExchange() async throws {
        let server = try UnixSocketServer(response: nil)
        let client = UnixEngineClient(socketPath: server.socketPath, timeout: .seconds(5))
        let call = Task {
            let _: HealthResponse = try await client.call(
                method: "system.health",
                params: [:],
                as: HealthResponse.self
            )
        }
        await server.waitUntilRequestReceived()
        call.cancel()

        do {
            try await call.value
            XCTFail("cancelled engine call unexpectedly returned")
        } catch is CancellationError {
        } catch {
            XCTFail("unexpected error: \(error)")
        }
    }

    func testOversizedResponseIsRejected() async throws {
        let server = try UnixSocketServer(response: Data(repeating: 0x41, count: 129))
        let client = UnixEngineClient(
            socketPath: server.socketPath,
            timeout: .seconds(1),
            maximumResponseBytes: 128
        )

        do {
            let _: HealthResponse = try await client.call(method: "system.health", params: [:], as: HealthResponse.self)
            XCTFail("oversized engine response unexpectedly decoded")
        } catch EngineError.invalidResponse {
        } catch {
            XCTFail("unexpected error: \(error)")
        }
    }

    func testMalformedResponseEnvelopeIsReportedAsInvalidResponse() async throws {
        let server = try UnixSocketServer(response: Data("not-json\n".utf8))
        let client = UnixEngineClient(socketPath: server.socketPath, timeout: .seconds(1))

        do {
            let _: HealthResponse = try await client.call(method: "system.health", params: [:], as: HealthResponse.self)
            XCTFail("malformed engine response unexpectedly decoded")
        } catch EngineError.invalidResponse {
        } catch {
            XCTFail("unexpected error: \(error)")
        }
    }
}

private final class UnixSocketServer: @unchecked Sendable {
    let socketPath: String
    private let descriptor: Int32
    private let queue = DispatchQueue(label: "ai.patchwright.tests.unix-server")
    private let requestSignal = SocketRequestSignal()

    init(response: Data?, responseDelay: TimeInterval = 0) throws {
        socketPath = FileManager.default.temporaryDirectory
            .appending(path: "patchwright-\(UUID().uuidString).sock")
            .path
        descriptor = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw POSIXError(.ENOTSOCK) }

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(socketPath.utf8)
        guard pathBytes.count < MemoryLayout.size(ofValue: address.sun_path) else {
            Darwin.close(descriptor)
            throw POSIXError(.ENAMETOOLONG)
        }
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.copyBytes(from: pathBytes)
            buffer[pathBytes.count] = 0
        }
        let addressLength = socklen_t(MemoryLayout<sa_family_t>.size + pathBytes.count + 1)
        let bindResult = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.bind(descriptor, $0, addressLength)
            }
        }
        guard bindResult == 0, Darwin.listen(descriptor, 1) == 0 else {
            Darwin.close(descriptor)
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }

        let listeningDescriptor = descriptor
        let requestSignal = requestSignal
        queue.async {
            let client = Darwin.accept(listeningDescriptor, nil, nil)
            guard client >= 0 else { return }
            defer { Darwin.close(client) }
            var request = [UInt8](repeating: 0, count: 4_096)
            guard Darwin.recv(client, &request, request.count, 0) > 0 else { return }
            Task { await requestSignal.markReceived() }
            if let response {
                if responseDelay > 0 {
                    Thread.sleep(forTimeInterval: responseDelay)
                }
                response.withUnsafeBytes { buffer in
                    guard let baseAddress = buffer.baseAddress else { return }
                    _ = Darwin.send(client, baseAddress, buffer.count, 0)
                }
            } else {
                while Darwin.recv(client, &request, request.count, 0) > 0 {}
            }
        }
    }

    func waitUntilRequestReceived() async {
        await requestSignal.wait()
    }

    deinit {
        Darwin.close(descriptor)
        Darwin.unlink(socketPath)
    }
}

private actor SocketRequestSignal {
    private var received = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func wait() async {
        guard !received else { return }
        await withCheckedContinuation { waiters.append($0) }
    }

    func markReceived() {
        received = true
        let waiters = waiters
        self.waiters.removeAll()
        waiters.forEach { $0.resume() }
    }
}
