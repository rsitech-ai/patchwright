import PatchwrightCore
import SwiftUI

struct GitHubInspector: View {
    let snapshot: GitHubRepositorySnapshot
    let status: GitHubStatus?
    let summary: GitHubSyncSummary?

    var body: some View {
        Form {
            Section("Local Snapshot") {
                LabeledContent("Repository", value: snapshot.repository.fullName)
                LabeledContent("Account", value: status?.account?.login ?? "Unavailable")
                LabeledContent("Synced", value: status?.lastSyncedAt ?? "Not recorded")
            }
            Section("Ingested Records") {
                LabeledContent("Issues and PRs", value: snapshot.workItems.count.formatted())
                LabeledContent("Discussion", value: snapshot.discussions.count.formatted())
                LabeledContent("Checks", value: snapshot.checks.count.formatted())
                LabeledContent("Workflow runs", value: snapshot.workflowRuns.count.formatted())
            }
            if let summary {
                Section("Latest Sync") {
                    LabeledContent("Repositories", value: "\(summary.repositoriesSynced) / \(summary.repositoriesDiscovered)")
                    if summary.failures.isEmpty {
                        Label("Completed without repository failures", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                    } else {
                        ForEach(summary.failures, id: \.self) { failure in
                            Text(failure).font(.caption).foregroundStyle(.red)
                        }
                    }
                }
            }
            Section("Credential Handling") {
                Text("Patchwright asks the authenticated GitHub CLI for a token at sync time. The token stays in engine memory and is not written to SQLite.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}
