import PatchwrightCore
import SwiftUI

struct TaskDetailView: View {
    let task: EngineeringTask?

    var body: some View {
        if let task {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    Text(task.title).font(.largeTitle.bold())
                    Label(task.repositoryPath, systemImage: "folder").foregroundStyle(.secondary)
                    GroupBox("Current stage") {
                        HStack { Image(systemName: task.requiresAttention ? "person.crop.circle.badge.exclamationmark" : "gearshape.2"); Text(task.state.rawValue); Spacer() }
                            .padding(.vertical, 8)
                    }
                    GroupBox("Implementation contract") {
                        ContentUnavailableView("Plan pending", systemImage: "list.bullet.clipboard", description: Text("Patchwright will show expected behavior, commands, risks, and rollback here before it changes files."))
                    }
                }.padding(28).frame(maxWidth: 820, alignment: .leading)
            }
        } else {
            ContentUnavailableView("Choose a task", systemImage: "hammer", description: Text("Create a task from a local Git repository to begin an auditable workflow."))
        }
    }
}

