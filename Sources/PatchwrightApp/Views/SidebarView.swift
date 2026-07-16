import PatchwrightCore
import SwiftUI

struct SidebarView: View {
    @ObservedObject var store: WorkspaceStore

    var body: some View {
        List(selection: $store.primarySelection) {
            Section("Workspace") {
                ForEach(WorkspaceSection.allCases) { section in
                    HStack(spacing: 8) {
                        Label(section.title, systemImage: section.systemImage)
                        Spacer()
                        if let count = count(for: section), count > 0 {
                            Text(count.formatted())
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                    .tag(section)
                    .accessibilityLabel(section.accessibilityLabel(count: count(for: section)))
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            VStack(spacing: 0) {
                Divider()
                HStack(spacing: 8) {
                    Circle()
                        .fill(connectionColor)
                        .frame(width: 8, height: 8)
                        .accessibilityHidden(true)
                    Text(connectionLabel)
                        .font(.caption)
                        .lineLimit(1)
                    Spacer()
                }
                .padding(10)
                .background(.bar)
            }
        }
        .navigationTitle("Patchwright")
    }

    private func count(for section: WorkspaceSection) -> Int? {
        switch section {
        case .queue: store.visiblePullRequests.count
        case .issues: store.visibleIssues.count
        case .repositories: store.repositories.count
        case .activeTasks, .awaitingApproval, .monitoring, .completed: store.tasks(for: section).count
        }
    }

    private var connectionColor: Color {
        if case .connected = store.connectionState { .green } else { .orange }
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

extension WorkspaceSection {
    var title: String {
        switch self {
        case .queue: "Pull Request Queue"
        case .issues: "Issues"
        case .repositories: "Repositories"
        case .activeTasks: "Active Tasks"
        case .awaitingApproval: "Awaiting Approval"
        case .monitoring: "Monitoring"
        case .completed: "Completed"
        }
    }

    var systemImage: String {
        switch self {
        case .queue: "list.number"
        case .issues: "record.circle"
        case .repositories: "shippingbox"
        case .activeTasks: "hammer"
        case .awaitingApproval: "person.crop.circle.badge.exclamationmark"
        case .monitoring: "waveform.path.ecg"
        case .completed: "checkmark.circle"
        }
    }

    func accessibilityLabel(count: Int?) -> String {
        guard let count, count > 0 else { return title }
        return "\(title), \(count) items"
    }
}
