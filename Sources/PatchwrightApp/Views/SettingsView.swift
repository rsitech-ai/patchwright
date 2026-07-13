import SwiftUI

struct SettingsView: View {
    @AppStorage("reviewProvider") private var reviewProvider = "apple"
    var body: some View {
        Form {
            Picker("Local review provider", selection: $reviewProvider) {
                Text("Apple Foundation Models").tag("apple")
                Text("Codex App Server").tag("codex")
            }
            Text("GitHub credentials and approvals are managed by the local engine, never the model.")
                .font(.caption).foregroundStyle(.secondary)
        }.padding().frame(width: 460)
    }
}

