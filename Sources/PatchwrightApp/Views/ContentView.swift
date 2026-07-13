import PatchwrightCore
import SwiftUI

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var inspectorPresented = true
    @State private var taskDraft: TaskDraft?

    var body: some View {
        NavigationSplitView {
            SidebarView(store: store)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260)
        } detail: {
            TaskDetailView(task: store.tasks.first { $0.id == store.selectedTaskID })
        }
        .inspector(isPresented: $inspectorPresented) {
            EvidenceInspector(task: store.tasks.first { $0.id == store.selectedTaskID })
                .inspectorColumnWidth(min: 250, ideal: 300, max: 420)
        }
        .toolbar {
            ToolbarItemGroup {
                Button("New Task", systemImage: "plus") { chooseRepository() }
                    .keyboardShortcut("n", modifiers: .command)
                Button("Toggle Evidence", systemImage: "sidebar.trailing") { inspectorPresented.toggle() }
            }
        }
        .sheet(item: $taskDraft) { draft in
            TaskComposer(draft: draft) { title in
                Task { await store.createTask(title: title, repositoryPath: draft.repositoryPath) }
            }
        }
        .task { await store.refreshHealth() }
    }

    private func chooseRepository() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Choose Repository"
        if panel.runModal() == .OK, let url = panel.url {
            taskDraft = TaskDraft(repositoryPath: url.path)
        }
    }
}

private struct TaskDraft: Identifiable { let id = UUID(); let repositoryPath: String }

private struct TaskComposer: View {
    let draft: TaskDraft
    let create: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Create Engineering Task").font(.title2.bold())
            Text(draft.repositoryPath).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            TextField("What should Patchwright do?", text: $title)
            HStack { Spacer(); Button("Cancel", role: .cancel) { dismiss() }; Button("Create") { create(title); dismiss() }.keyboardShortcut(.defaultAction).disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) }
        }
        .padding(24)
        .frame(width: 520)
    }
}

