import Foundation
import XCTest
@testable import PatchwrightCore

final class RelayHealthCheckerTests: XCTestCase {
    func testSuccessfulHealthCheckReturnsWithoutBlockingTheCaller() async throws {
        try await RelayHealthChecker.verify(
            executable: URL(fileURLWithPath: "/usr/bin/true"),
            configurationURL: URL(fileURLWithPath: "/tmp/fixture.json"),
            timeout: 1
        )
    }

    func testHealthCheckTerminatesAfterTheBoundedTimeout() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let executable = directory.appending(path: "slow-relay")
        try "#!/bin/sh\ntrap '' TERM\nsleep 5\n".write(to: executable, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: executable.path)

        let started = ContinuousClock.now
        do {
            try await RelayHealthChecker.verify(
                executable: executable,
                configurationURL: directory.appending(path: "fixture.json"),
                timeout: 0.05
            )
            XCTFail("slow relay unexpectedly passed")
        } catch let error as RelayHealthCheckError {
            XCTAssertEqual(error, .timedOut)
            XCTAssertLessThan(ContinuousClock.now - started, .seconds(1))
        }
    }

    func testNonzeroExitReportsRedactedConfigurationDiagnostic() async throws {
        let fixture = try relayFixture(
            body: "printf '%s\\n' 'configuration invalid token=super-secret' >&2\nexit 23\n"
        )
        defer { fixture.cleanup() }

        let message = await failureMessage(executable: fixture.executable)

        XCTAssertTrue(message.localizedCaseInsensitiveContains("configuration"))
        XCTAssertTrue(message.contains("exit 23"))
        XCTAssertTrue(message.contains("[REDACTED]"))
        XCTAssertFalse(message.contains("super-secret"))
    }

    func testDiagnosticsDistinguishPermissionAndNetworkFailures() async throws {
        let permissionFixture = try relayFixture(body: "echo 'permission denied by GitHub (403)' >&2\nexit 4\n")
        let networkFixture = try relayFixture(body: "echo 'network DNS resolution failed' >&2\nexit 5\n")
        defer {
            permissionFixture.cleanup()
            networkFixture.cleanup()
        }

        let permissionMessage = await failureMessage(executable: permissionFixture.executable)
        let networkMessage = await failureMessage(executable: networkFixture.executable)

        XCTAssertTrue(permissionMessage.localizedCaseInsensitiveContains("permission"))
        XCTAssertTrue(networkMessage.localizedCaseInsensitiveContains("network"))
        XCTAssertNotEqual(permissionMessage, networkMessage)
    }

    func testDiagnosticOutputIsBoundedAndPrivateKeyMaterialIsRedacted() async throws {
        let fixture = try relayFixture(
            body: "echo '-----BEGIN PRIVATE KEY-----' >&2\n"
                + "printf 'A%.0s' $(jot 5000 1) >&2\n"
                + "echo '\n-----END PRIVATE KEY-----' >&2\nexit 9\n"
        )
        defer { fixture.cleanup() }

        let message = await failureMessage(executable: fixture.executable)

        XCTAssertLessThanOrEqual(message.utf8.count, 1_500)
        XCTAssertTrue(message.contains("[REDACTED PRIVATE KEY]"))
        XCTAssertFalse(message.contains("BEGIN PRIVATE KEY"))
    }

    private func failureMessage(executable: URL) async -> String {
        do {
            try await RelayHealthChecker.verify(
                executable: executable,
                configurationURL: URL(fileURLWithPath: "/tmp/fixture.json"),
                timeout: 1
            )
            return "unexpected success"
        } catch {
            return error.localizedDescription
        }
    }

    private func relayFixture(body: String) throws -> RelayFixture {
        let directory = FileManager.default.temporaryDirectory
            .appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let executable = directory.appending(path: "relay-fixture")
        try "#!/bin/sh\n\(body)".write(to: executable, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: executable.path)
        return RelayFixture(directory: directory, executable: executable)
    }
}

private struct RelayFixture {
    let directory: URL
    let executable: URL

    func cleanup() {
        try? FileManager.default.removeItem(at: directory)
    }
}
