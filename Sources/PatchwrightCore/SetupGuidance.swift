public struct SetupPermission: Equatable, Sendable {
    public let level: String
    public let capabilities: String

    public init(level: String, capabilities: String) {
        self.level = level
        self.capabilities = capabilities
    }
}

public enum SetupGuidance {
    public static let readOnlyGitHub =
        "Install GitHub CLI (`gh`) and sign in with your own GitHub account. Patchwright reads through `gh` only when you choose Sync GitHub. This path does not write to GitHub."
    public static let readOnlyGitHubSecondary =
        "No GitHub App or private key is required for read-only sync."

    public static let codex =
        "Install the Codex CLI separately, sign in, and make sure `codex` is on PATH before launching Patchwright. Relaunch Patchwright after installation."
    public static let codexSecondary =
        "Codex is required only for coding-agent sessions. GitHub sync and review remain available without it."

    public static let mutations =
        "GitHub mutations require an App that you create, own, and install only on selected repositories. Patchwright ships with no publisher GitHub App credentials or private key."
    public static let privateKey =
        "An imported key is copied to an owner-only file on this Mac. Never reuse or share another publisher's key."
    public static let mutationApproval =
        "Every mutation remains blocked until your App is installed for the target repository, the exact action is previewed and approved, and you choose Execute."

    public static let maximumPermissions: [SetupPermission] = [
        .init(level: "Read", capabilities: "Actions, Metadata"),
        .init(level: "Read & write", capabilities: "Checks, Contents, Issues, Pull requests"),
        .init(level: "Not requested", capabilities: "Administration, Workflows"),
    ]
}
