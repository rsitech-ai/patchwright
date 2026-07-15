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
}
