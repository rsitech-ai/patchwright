import PatchwrightCore
import SwiftUI

struct SidebarView: View {
    @ObservedObject var store: WorkspaceStore

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    sectionHeader("Tasks")
                ForEach(store.tasks) { task in
                    Button {
                        store.selectedTaskID = task.id
                    } label: {
                        Label {
                            VStack(alignment: .leading) {
                                Text(task.title).lineLimit(1)
                                Text(task.state.label).font(.caption).foregroundStyle(.secondary)
                            }
                        } icon: {
                            Image(systemName: task.requiresAttention ? "exclamationmark.circle.fill" : "hammer")
                                .foregroundStyle(task.requiresAttention ? .orange : .secondary)
                        }
                    }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 6)
                }
                    sectionHeader("GitHub", trailing: githubLoginLabel)
                if store.repositories.isEmpty {
                    Text(store.githubStatus?.connected == true ? "No repositories ingested" : "Sync to connect your account")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                ForEach(store.repositories) { repository in
                    Button {
                        Task { await store.selectRepository(repository) }
                    } label: {
                        Label {
                            VStack(alignment: .leading) {
                                Text(repository.fullName).lineLimit(1)
                                Text("\(repository.openIssuesCount) open · \(repository.private ? "Private" : "Public")")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        } icon: {
                            Image(systemName: repository.private ? "lock" : "shippingbox")
                        }
                    }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 6)
                }
                if let error = store.githubError {
                    Label(error, systemImage: "exclamationmark.triangle")
                        .font(.caption)
                        .foregroundStyle(.red)
                        .padding(6)
                }
                if let failures = store.githubSyncSummary?.failures, !failures.isEmpty {
                    Text("\(failures.count) repositories could not be refreshed; existing snapshots were preserved.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(6)
                } else if let syncedAt = store.githubStatus?.lastSyncedAt {
                    Text("Last local snapshot: \(syncedAt)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(6)
                }
            }
                .padding(.vertical, 8)
            }
            Divider()
            HStack { Circle().fill(connectionColor).frame(width: 8, height: 8); Text(connectionLabel).font(.caption); Spacer() }
                .padding(10)
        }
        .background(.bar)
    }

    private func sectionHeader(_ title: String, trailing: String? = nil) -> some View {
        HStack {
            Text(title).font(.caption.bold()).foregroundStyle(.secondary)
            Spacer()
            if let trailing { Text(trailing).font(.caption2).foregroundStyle(.tertiary) }
        }
        .textCase(.uppercase)
        .padding(.horizontal, 6)
        .padding(.top, 8)
    }

    private var connectionColor: Color { if case .connected = store.connectionState { .green } else { .orange } }
    private var githubLoginLabel: String? {
        guard let login = store.githubStatus?.account?.login else { return nil }
        return "@\(login)"
    }
    private var connectionLabel: String {
        switch store.connectionState {
        case .disconnected: "Engine disconnected"
        case .connecting: "Connecting…"
        case .connected(let version): "Engine \(version)"
        case .failed(let message): message
        }
    }
}

private extension TaskState {
    var label: String {
        rawValue.replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression).capitalized
    }
}
