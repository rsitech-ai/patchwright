import Foundation
#if canImport(Darwin)
import Darwin
#endif

public enum RelayHealthFailureCategory: String, Equatable, Sendable {
    case configuration
    case permission
    case network
    case unknown
}

public enum RelayHealthCheckError: Error, Equatable, LocalizedError, Sendable {
    case launchFailed(String)
    case timedOut
    case unhealthy(status: Int32, category: RelayHealthFailureCategory, detail: String)

    public var errorDescription: String? {
        switch self {
        case .launchFailed(let detail):
            "The GitHub relay could not start: \(detail)"
        case .timedOut:
            "GitHub App authentication timed out. Check the relay configuration and try again."
        case .unhealthy(let status, let category, let detail):
            "GitHub App \(category.rawValue) check failed (exit \(status)). \(detail)"
        }
    }
}

public enum RelayHealthChecker {
    public static func verify(
        executable: URL,
        configurationURL: URL,
        timeout: TimeInterval = 10
    ) async throws {
        let result = try await Task.detached(priority: .userInitiated) {
            let process = Process()
            let diagnostics = Pipe()
            let capture = BoundedDiagnosticCapture(maximumBytes: 16_384)
            let readerGroup = DispatchGroup()
            process.executableURL = executable
            process.arguments = ["github-app-health", "--config", configurationURL.path]
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            process.standardError = diagnostics
            readerGroup.enter()
            DispatchQueue(label: "ai.patchwright.relay-health-diagnostics").async {
                defer { readerGroup.leave() }
                while let data = try? diagnostics.fileHandleForReading.read(upToCount: 4_096),
                      !data.isEmpty {
                    capture.append(data)
                }
            }
            do {
                try process.run()
                try? diagnostics.fileHandleForWriting.close()
            } catch {
                try? diagnostics.fileHandleForWriting.close()
                await waitForDiagnosticReader(readerGroup, timeout: .milliseconds(100))
                throw RelayHealthCheckError.launchFailed(
                    RelayDiagnostic.sanitize(error.localizedDescription)
                )
            }

            let deadline = Date().addingTimeInterval(max(timeout, 0.01))
            while process.isRunning, Date() < deadline {
                try await Task.sleep(for: .milliseconds(10))
            }
            guard process.isRunning else {
                await waitForDiagnosticReader(readerGroup, timeout: .milliseconds(200))
                try? diagnostics.fileHandleForReading.close()
                return RelayProcessResult(
                    status: process.terminationStatus,
                    diagnostic: capture.value
                )
            }
            process.terminate()
            try await Task.sleep(for: .milliseconds(100))
            if process.isRunning {
                #if canImport(Darwin)
                Darwin.kill(process.processIdentifier, SIGKILL)
                #endif
            }
            process.waitUntilExit()
            try? diagnostics.fileHandleForReading.close()
            throw RelayHealthCheckError.timedOut
        }.value
        guard result.status == 0 else {
            throw RelayHealthCheckError.unhealthy(
                status: result.status,
                category: RelayDiagnostic.category(for: result.diagnostic),
                detail: RelayDiagnostic.sanitize(result.diagnostic)
            )
        }
    }
}

private func waitForDiagnosticReader(_ group: DispatchGroup, timeout: Duration) async {
    await withCheckedContinuation { continuation in
        let signal = OneShotSignal()
        group.notify(queue: .global()) { signal.resume(continuation) }
        Task {
            try? await Task.sleep(for: timeout)
            signal.resume(continuation)
        }
    }
}

private final class OneShotSignal: @unchecked Sendable {
    private let lock = NSLock()
    private var fired = false

    func resume(_ continuation: CheckedContinuation<Void, Never>) {
        lock.lock()
        guard !fired else {
            lock.unlock()
            return
        }
        fired = true
        lock.unlock()
        continuation.resume()
    }
}

private struct RelayProcessResult: Sendable {
    let status: Int32
    let diagnostic: String
}

private final class BoundedDiagnosticCapture: @unchecked Sendable {
    private let lock = NSLock()
    private let maximumBytes: Int
    private var data = Data()

    init(maximumBytes: Int) {
        self.maximumBytes = maximumBytes
    }

    func append(_ chunk: Data) {
        lock.lock()
        defer { lock.unlock() }
        let remaining = maximumBytes - data.count
        guard remaining > 0 else { return }
        data.append(chunk.prefix(remaining))
    }

    var value: String {
        lock.lock()
        defer { lock.unlock() }
        return String(decoding: data, as: UTF8.self)
    }
}

private enum RelayDiagnostic {
    private static let maximumDetailBytes = 1_024

    static func category(for diagnostic: String) -> RelayHealthFailureCategory {
        let value = diagnostic.lowercased()
        if containsAny(value, ["permission", "forbidden", "unauthorized", "denied", "401", "403"]) {
            return .permission
        }
        if containsAny(
            value,
            ["network", "dns", "resolve", "connection", "connect", "tls", "offline", "unreachable"]
        ) {
            return .network
        }
        if containsAny(
            value,
            ["config", "app id", "client id", "invalid", "parse", "missing", "rsa", "pem", "private key"]
        ) {
            return .configuration
        }
        return .unknown
    }

    static func sanitize(_ diagnostic: String) -> String {
        var value = diagnostic
        value = replacing(
            #"-----BEGIN [^-\n]*PRIVATE KEY-----.*?-----END [^-\n]*PRIVATE KEY-----"#,
            in: value,
            with: "[REDACTED PRIVATE KEY]",
            options: [.caseInsensitive, .dotMatchesLineSeparators]
        )
        value = replacing(#"(?i)\bBearer\s+[^\s,;]+"#, in: value, with: "Bearer [REDACTED]")
        value = replacing(#"\bgh[pousr]_[A-Za-z0-9_]+"#, in: value, with: "[REDACTED]")
        value = replacing(
            #"\b[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\b"#,
            in: value,
            with: "[REDACTED]"
        )
        value = replacing(
            #"(?i)\b(token|secret|client[_ -]?secret|private[_ -]?key|authorization|key[_ -]?reference)\s*[:=]\s*[^\s,;]+"#,
            in: value,
            with: "$1=[REDACTED]"
        )
        value = value
            .split(whereSeparator: { $0.isWhitespace })
            .joined(separator: " ")
        if value.isEmpty { value = "No relay diagnostic was emitted." }
        guard value.utf8.count > maximumDetailBytes else { return value }
        return String(decoding: value.utf8.prefix(maximumDetailBytes), as: UTF8.self) + "…"
    }

    private static func containsAny(_ value: String, _ candidates: [String]) -> Bool {
        candidates.contains { value.contains($0) }
    }

    private static func replacing(
        _ pattern: String,
        in value: String,
        with replacement: String,
        options: NSRegularExpression.Options = []
    ) -> String {
        guard let expression = try? NSRegularExpression(pattern: pattern, options: options) else {
            return value
        }
        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        return expression.stringByReplacingMatches(
            in: value,
            options: [],
            range: range,
            withTemplate: replacement
        )
    }
}
