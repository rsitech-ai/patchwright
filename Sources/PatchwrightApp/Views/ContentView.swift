import AppKit
import PatchwrightCore
import SwiftUI

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var inspectorPresented = false
    @State private var taskDraft: TaskDraft?

    var body: some View {
        NavigationSplitView {
            SidebarView(store: store)
                .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 280)
        } content: {
            WorkspaceTableView(store: store)
                .navigationSplitViewColumnWidth(min: 420, ideal: 670)
        } detail: {
            detailContent
                .frame(minWidth: 420)
                .inspector(isPresented: $inspectorPresented) {
                    inspectorContent
                        .inspectorColumnWidth(min: 260, ideal: 320, max: 420)
                }
        }
        .toolbar {
            ToolbarItemGroup {
                Button("New Local Task", systemImage: "plus") { chooseRepository() }
                    .keyboardShortcut("n", modifiers: .command)
                    .help("Create a task from a local Git repository")
                Button("Inspector", systemImage: "sidebar.trailing") { inspectorPresented.toggle() }
                    .help("Show evidence, approvals, instructions, and credential state")
                if store.isSyncingGitHub {
                    Button("Cancel GitHub Sync", systemImage: "xmark.circle", role: .destructive) {
                        Task { await store.cancelGitHubSync() }
                    }
                    .help(store.githubSyncJob?.summary ?? "Cancel the active GitHub sync")
                } else {
                    Button("Sync GitHub", systemImage: "arrow.triangle.2.circlepath") {
                        Task { await store.syncGitHub() }
                    }
                    .help("Refresh the local GitHub snapshot")
                }
            }
        }
        .sheet(item: $taskDraft) { draft in
            TaskComposer(draft: draft) { title in
                Task { await store.createTask(title: title, repositoryPath: draft.repositoryPath) }
            }
        }
        .task { await store.refreshHealth() }
    }

    @ViewBuilder private var detailContent: some View {
        if let task = store.selectedTask {
            TaskDetailView(store: store, task: task)
        } else if let snapshot = store.selectedRepository {
            GitHubRepositoryView(store: store, snapshot: snapshot, item: store.selectedWorkItem)
        } else {
            ContentUnavailableView(
                "Choose Work to Inspect",
                systemImage: "sidebar.left",
                description: Text("Select a repository, pull request, or task from the workspace table.")
            )
        }
    }

    @ViewBuilder private var inspectorContent: some View {
        if let snapshot = store.selectedRepository, store.selectedTaskID == nil {
            GitHubInspector(snapshot: snapshot, status: store.githubStatus, summary: store.githubSyncSummary)
        } else {
            EvidenceInspector(
                task: store.selectedTask,
                codexStatus: store.selectedTask.flatMap { store.codexStatus(for: $0.id) }
            )
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
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Create") { create(title); dismiss() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 520)
    }
}
