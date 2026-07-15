import AppKit
import PatchwrightCore
import SwiftUI

struct SettingsView: View {
    @AppStorage("reviewProvider") private var reviewProvider = "apple"
    @State private var appID = ""
    @State private var clientID = ""
    @State private var keyReference = ""
    @State private var status = "Optional GitHub App not configured. Read-only gh sync remains available."
    @State private var importing = false

    var body: some View {
        Form {
            Section("Read-only GitHub sync") {
                Text(SetupGuidance.readOnlyGitHub)
                Text(SetupGuidance.readOnlyGitHubSecondary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("Codex coding sessions") {
                Text(SetupGuidance.codex)
                Text(SetupGuidance.codexSecondary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Picker("Review provider", selection: $reviewProvider) {
                    Text("Apple Foundation Models").tag("apple")
                    Text("Codex CLI").tag("codex")
                }
            }
            Section("Your GitHub App — mutations") {
                Text(SetupGuidance.mutations)
                TextField("App ID", text: $appID)
                TextField("Client ID", text: $clientID)
                LabeledContent("Private key", value: privateKeyLocation)
                Text(SetupGuidance.privateKey)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack {
                    Button("Import Private Key…") { importPrivateKey() }
                        .disabled(importing || UInt64(appID) == nil || clientID.isEmpty)
                    Button("Save Metadata") {
                        do {
                            try saveMetadata(keyReference: keyReference)
                            Task { await verifyConnection() }
                        }
                        catch { status = error.localizedDescription }
                    }
                        .disabled(UInt64(appID) == nil || clientID.isEmpty || keyReference.isEmpty)
                }
                Text(status).font(.caption).foregroundStyle(.secondary)
            }
            Section("Maximum GitHub App permissions") {
                ForEach(SetupGuidance.maximumPermissions, id: \.level) { permission in
                    LabeledContent(permission.level, value: permission.capabilities)
                }
                Text(SetupGuidance.mutationApproval)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
        .frame(width: 620, height: 620)
        .task { loadMetadata() }
    }

    private func importPrivateKey() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.data]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.prompt = "Import Securely"
        guard panel.runModal() == .OK, let path = panel.url else { return }
        importing = true
        defer { importing = false }
        do {
            let secretDirectory = configurationURL.deletingLastPathComponent()
                .appending(path: "secrets", directoryHint: .isDirectory)
            try FileManager.default.createDirectory(at: secretDirectory, withIntermediateDirectories: true)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: secretDirectory.path)
            let destination = secretDirectory.appending(path: "github-app-\(appID).pem")
            try Data(contentsOf: path).write(to: destination, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: destination.path)
            keyReference = "file:\(destination.path)"
            try saveMetadata(keyReference: keyReference)
            Task { await verifyConnection() }
        } catch {
            status = error.localizedDescription
        }
    }

    private func loadMetadata() {
        guard let data = try? Data(contentsOf: configurationURL),
              let configuration = try? JSONDecoder().decode(Configuration.self, from: data) else { return }
        appID = String(configuration.appId)
        clientID = configuration.clientId
        keyReference = configuration.keyReference
        status = "Your GitHub App metadata is loaded."
    }

    private func saveMetadata(keyReference: String) throws {
        guard let appID = UInt64(appID), !clientID.isEmpty, !keyReference.isEmpty else {
            throw SetupError("App ID, Client ID, and a private-key reference are required.")
        }
        let directory = configurationURL.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
        let configuration = Configuration(
            appId: appID,
            clientId: clientID,
            keyReference: keyReference,
            apiBaseUrl: "https://api.github.com"
        )
        let data = try JSONEncoder().encode(configuration)
        try data.write(to: configurationURL, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: configurationURL.path)
        status = "Metadata saved. Relaunch Patchwright to apply your GitHub App configuration."
    }

    private func verifyConnection() async {
        let executable = ProcessInfo.processInfo.environment["PATCHWRIGHT_RELAY_BINARY"]
            .map(URL.init(fileURLWithPath:))
            ?? Bundle.main.bundleURL.appending(path: "Contents/Helpers/patchwright-relay")
        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
            status = "The bundled GitHub relay is unavailable. Rebuild Patchwright and try again."
            return
        }
        do {
            try await RelayHealthChecker.verify(
                executable: executable,
                configurationURL: configurationURL
            )
            status = "Your GitHub App authenticated. Install it only on selected repositories, then relaunch Patchwright."
        } catch {
            status = error.localizedDescription
        }
    }

    private var privateKeyLocation: String {
        if keyReference.hasPrefix("keychain:") { return "Stored in Keychain" }
        if keyReference.hasPrefix("file:") { return "Owner-only protected file" }
        return "Not imported"
    }

    private var configurationURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appending(path: ".patchwright", directoryHint: .isDirectory)
            .appending(path: "github-app.json")
    }
}

private struct Configuration: Codable {
    let appId: UInt64
    let clientId: String
    let keyReference: String
    let apiBaseUrl: String
}

private struct SetupError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
