import PatchwrightCore
import XCTest

final class SetupGuidanceTests: XCTestCase {
    func testReadOnlyGitHubBoundaryIsExplicit() {
        XCTAssertTrue(SetupGuidance.readOnlyGitHub.contains("gh"))
        XCTAssertTrue(SetupGuidance.readOnlyGitHub.contains("does not write to GitHub"))
        XCTAssertTrue(SetupGuidance.readOnlyGitHubSecondary.contains("No GitHub App or private key is required"))
    }

    func testCodexIsAnOptionalSeparatelyManagedIntegration() {
        for phrase in ["separately", "sign in", "PATH", "Relaunch"] {
            XCTAssertTrue(SetupGuidance.codex.contains(phrase), "missing \(phrase)")
        }
        XCTAssertTrue(SetupGuidance.codexSecondary.contains("GitHub sync and review remain available without it"))
    }

    func testMutationBoundaryRequiresUserOwnershipAndExplicitExecution() {
        for phrase in ["create", "own", "selected repositories", "no publisher GitHub App credentials or private key"] {
            XCTAssertTrue(SetupGuidance.mutations.contains(phrase), "missing \(phrase)")
        }
        XCTAssertTrue(SetupGuidance.mutationApproval.contains("previewed and approved"))
        XCTAssertTrue(SetupGuidance.mutationApproval.contains("Execute"))
    }

    func testMaximumPermissionsAreExact() {
        XCTAssertEqual(
            SetupGuidance.maximumPermissions,
            [
                .init(level: "Read", capabilities: "Actions, Metadata"),
                .init(level: "Read & write", capabilities: "Checks, Contents, Issues, Pull requests"),
                .init(level: "Not requested", capabilities: "Administration, Workflows"),
            ]
        )
    }

    func testPrivateKeyGuidanceKeepsPublisherCredentialsOut() {
        XCTAssertTrue(SetupGuidance.privateKey.contains("owner-only"))
        XCTAssertTrue(SetupGuidance.privateKey.contains("Never reuse or share another publisher's key"))
    }
}
