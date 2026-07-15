import Foundation

public enum RelayHealthCheckError: Error, Equatable, LocalizedError, Sendable {
    case launchFailed(String)
    case timedOut
    case unhealthy(Int32)

    public var errorDescription: String? {
        switch self {
        case .launchFailed(let detail):
            "The GitHub relay could not start: \(detail)"
        case .timedOut:
            "GitHub App authentication timed out. Check the relay configuration and try again."
        case .unhealthy:
            "GitHub App authentication failed. Check the App ID and use its unencrypted RSA private key."
        }
    }
}

public enum RelayHealthChecker {
    public static func verify(
        executable: URL,
        configurationURL: URL,
        timeout: TimeInterval = 10
    ) async throws {
        let status = try await Task.detached(priority: .userInitiated) {
            let process = Process()
            process.executableURL = executable
            process.arguments = ["github-app-health", "--config", configurationURL.path]
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.nullDevice
            do {
                try process.run()
            } catch {
                throw RelayHealthCheckError.launchFailed(error.localizedDescription)
            }

            let deadline = Date().addingTimeInterval(max(timeout, 0.01))
            while process.isRunning, Date() < deadline {
                try await Task.sleep(for: .milliseconds(10))
            }
            guard process.isRunning else { return process.terminationStatus }
            process.terminate()
            process.waitUntilExit()
            throw RelayHealthCheckError.timedOut
        }.value
        guard status == 0 else { throw RelayHealthCheckError.unhealthy(status) }
    }
}
