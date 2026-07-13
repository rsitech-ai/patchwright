import PatchwrightCore
import SwiftUI

struct SidebarView: View {
    @ObservedObject var store: WorkspaceStore

    var body: some View {
        List(selection: $store.selectedTaskID) {
            Section("Tasks") {
                ForEach(store.tasks) { task in
                    Label {
                        VStack(alignment: .leading) {
                            Text(task.title).lineLimit(1)
                            Text(task.state.label).font(.caption).foregroundStyle(.secondary)
                        }
                    } icon: {
                        Image(systemName: task.requiresAttention ? "exclamationmark.circle.fill" : "hammer")
                            .foregroundStyle(task.requiresAttention ? .orange : .secondary)
                    }
                    .tag(task.id)
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            HStack { Circle().fill(connectionColor).frame(width: 8, height: 8); Text(connectionLabel).font(.caption); Spacer() }
                .padding(10).background(.bar)
        }
    }

    private var connectionColor: Color { if case .connected = store.connectionState { .green } else { .orange } }
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

