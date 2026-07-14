import AppKit
import SwiftUI

struct SettingsView: View {
    @AppStorage("reviewProvider") private var reviewProvider = "apple"
    @State private var appID = ""
    @State private var clientID = ""
    @State private var keyReference = ""
    @State private var status = "GitHub App configuration is incomplete."
    @State private var importing = false

    var body: some View {
        Form {
            Section("Local intelligence") {
                Picker("Review provider", selection: $reviewProvider) {
                    Text("Apple Foundation Models").tag("apple")
                    Text("Codex App Server").tag("codex")
                }
            }
            Section("Patchwright GitHub App") {
                TextField("App ID", text: $appID)
                TextField("Client ID", text: $clientID)
                LabeledContent("Private key", value: privateKeyLocation)
                HStack {
                    Button("Import Private Key…") { importPrivateKey() }
                        .disabled(importing || UInt64(appID) == nil || clientID.isEmpty)
                    Button("Save Metadata") {
                        do {
                            try saveMetadata(keyReference: keyReference)
                            try verifyConnection()
                        }
                        catch { status = error.localizedDescription }
                    }
                        .disabled(UInt64(appID) == nil || clientID.isEmpty || keyReference.isEmpty)
                }
                Text(status).font(.caption).foregroundStyle(.secondary)
            }
            Section("Repository permissions") {
                LabeledContent("Read-only", value: "Actions, Administration, Metadata")
                LabeledContent("Read & write", value: "Checks, Contents, Issues, Pull requests, Workflows")
                Text("Every remote write still requires an exact preview, a short-lived matching approval, and a separate Execute action.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
        .frame(width: 560, height: 470)
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
            try verifyConnection()
            status = "App authenticated. Install it on selected repositories, then relaunch Patchwright."
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
        status = "GitHub App metadata loaded."
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
        status = "Metadata saved. Relaunch Patchwright to apply the configuration."
    }

    private func verifyConnection() throws {
        let executable = ProcessInfo.processInfo.environment["PATCHWRIGHT_RELAY_BINARY"]
            .map(URL.init(fileURLWithPath:))
            ?? Bundle.main.bundleURL.appending(path: "Contents/Helpers/patchwright-relay")
        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
            throw SetupError("The bundled GitHub relay is unavailable. Rebuild Patchwright and try again.")
        }
        let process = Process()
        process.executableURL = executable
        process.arguments = ["github-app-health", "--config", configurationURL.path]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw SetupError("GitHub App authentication failed. Check the App ID and use its unencrypted RSA private key.")
        }
        status = "GitHub App authentication succeeded."
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
