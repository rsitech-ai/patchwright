import PatchwrightCore
import SwiftUI

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var inspectorPresented = false
    @State private var taskDraft: TaskDraft?

    var body: some View {
        VStack(spacing: 0) {
            commandBar
            Divider()
            HStack(spacing: 0) {
                SidebarView(store: store)
                    .frame(width: 270)
                Divider()
                HStack(spacing: 0) {
                    detailContent
                    if inspectorPresented {
                        Divider()
                        inspectorContent
                            .frame(width: 300)
                    }
                }
            }
        }
        .overlay {
            if store.isSyncingGitHub {
                ProgressView("Ingesting GitHub… \(store.repositories.count) repositories available")
                    .padding(18)
                    .background(.regularMaterial, in: .rect(cornerRadius: 12))
            }
        }
        .sheet(item: $taskDraft) { draft in
            TaskComposer(draft: draft) { title in
                Task { await store.createTask(title: title, repositoryPath: draft.repositoryPath) }
            }
        }
        .task { await store.refreshHealth() }
    }

    private var commandBar: some View {
        HStack(spacing: 12) {
            Text("Patchwright").font(.headline)
            Spacer()
            Button("New Task", systemImage: "plus") { chooseRepository() }
                .keyboardShortcut("n", modifiers: .command)
                .help("Create a task from a local Git repository")
            Button("Evidence", systemImage: "sidebar.trailing") { inspectorPresented.toggle() }
                .help("Show or hide evidence and ingestion details")
            Button("Sync GitHub", systemImage: "arrow.triangle.2.circlepath") {
                Task { await store.syncGitHub() }
            }
            .disabled(store.isSyncingGitHub)
            .help("Refresh the read-only local GitHub snapshot")
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .frame(height: 42)
        .background(.bar)
    }

    @ViewBuilder private var detailContent: some View {
        if let repository = store.selectedRepository, store.selectedTaskID == nil {
            GitHubRepositoryView(snapshot: repository)
        } else {
            TaskDetailView(task: store.tasks.first { $0.id == store.selectedTaskID })
        }
    }

    @ViewBuilder private var inspectorContent: some View {
        if let repository = store.selectedRepository, store.selectedTaskID == nil {
            GitHubInspector(snapshot: repository, status: store.githubStatus, summary: store.githubSyncSummary)
        } else {
            EvidenceInspector(task: store.tasks.first { $0.id == store.selectedTaskID })
        }
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
