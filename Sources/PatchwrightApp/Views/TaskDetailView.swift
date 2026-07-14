import PatchwrightCore
import SwiftUI

struct TaskDetailView: View {
    @ObservedObject var store: WorkspaceStore
    let task: EngineeringTask
    @SceneStorage("patchwright.taskWorkbenchTab") private var tabRaw = TaskWorkbenchTab.overview.rawValue
    @State private var deliveryBody = ""
    @State private var deliveryApprovalPresented = false
    @State private var mergeApprovalPresented = false
    @State private var mergeMethod = GitHubMergeMethod.squash

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            Picker("Task workbench", selection: $tabRaw) {
                ForEach(TaskWorkbenchTab.allCases) { tab in
                    Text(tab.title).tag(tab.rawValue)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(12)
            Divider()
            tabContent
        }
        .sheet(isPresented: $deliveryApprovalPresented) {
            DeliveryApprovalSheet(store: store, task: task)
        }
        .sheet(isPresented: $mergeApprovalPresented) {
            DeliveryApprovalSheet(store: store, task: task)
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(task.title).font(.title2.bold()).lineLimit(2)
                Spacer()
                Label(task.state.displayName, systemImage: task.requiresAttention ? "person.crop.circle.badge.exclamationmark" : "gearshape.2")
                    .foregroundStyle(task.requiresAttention ? .orange : .secondary)
            }
            Label(task.repositoryPath, systemImage: "folder")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .help(task.repositoryPath)
        }
        .padding(20)
    }

    @ViewBuilder private var tabContent: some View {
        switch TaskSurfaceState.resolve(state: task.state, reason: task.interruption?.reason) {
        case .cancelled:
            ContentUnavailableView(
                "Task Cancelled",
                systemImage: "xmark.circle",
                description: Text("The durable task remains available for audit. Start a new task to resume this outcome.")
            )
        case .blocked(let reason):
            ContentUnavailableView("Task Blocked", systemImage: "exclamationmark.octagon", description: Text(reason))
        case .ready:
            if TaskWorkbenchTab(rawValue: tabRaw) == .codex {
                CodexThreadView(store: store, task: task)
                    .frame(minHeight: 420)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        switch TaskWorkbenchTab(rawValue: tabRaw) ?? .overview {
                        case .overview: overview
                        case .codex: EmptyView()
                        case .changes: placeholder("Changes", "Worktree file changes and diffs will appear here.", "doc.badge.gearshape")
                        case .verification: placeholder("Verification", "Commands, checks, findings, and evidence will appear here.", "checkmark.shield")
                        case .delivery: deliveryPanel
                        case .merge: mergePanel
                        }
                    }
                    .padding(20)
                    .frame(maxWidth: 860, alignment: .leading)
                }
            }
        }
    }

    private var overview: some View {
        Group {
            detailCard("Current stage") {
                LabeledContent("State", value: task.state.displayName)
                LabeledContent("Updated") { TimestampText(date: task.updatedAt) }
                LabeledContent("Contract", value: task.contractVersion.map { "Version \($0)" } ?? "Pending")
            }
            detailCard("Source") {
                sourceSummary
            }
            detailCard("Implementation contract") {
                ContentUnavailableView(
                    "Assessment pending",
                    systemImage: "list.bullet.clipboard",
                    description: Text("Expected behavior, commands, risks, sensitive paths, and rollback are fixed before preparation approval.")
                )
            }
        }
    }

    private var deliveryPanel: some View {
        detailCard("Approval-bound GitHub comment") {
            Text("Prepare one exact comment for the ingested issue or pull request. Previewing does not approve or execute it.")
                .font(.caption)
                .foregroundStyle(.secondary)
            TextEditor(text: $deliveryBody)
                .font(.body)
                .frame(minHeight: 140)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
            HStack {
                if let execution = store.deliveryExecutions[task.id] {
                    Label(execution.state.capitalized, systemImage: "checkmark.seal")
                        .foregroundStyle(.green)
                } else if store.deliveryPreviews[task.id] != nil {
                    Label("Preview ready", systemImage: "doc.text.magnifyingglass")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Preview Exact Comment", systemImage: "eye") {
                    Task {
                        await store.previewCommentDelivery(task: task, body: deliveryBody)
                        if store.deliveryPreviews[task.id] != nil { deliveryApprovalPresented = true }
                    }
                }
                .disabled(
                    deliveryBody.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        || store.deliveryBusyTaskIDs.contains(task.id)
                )
            }
            if let error = store.deliveryError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
    }

    private var mergePanel: some View {
        detailCard("Exact-SHA pull request merge") {
            Text("Merge requires a fresh preview and a separate Merge-class approval bound to the ingested head and base SHAs.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Picker("Method", selection: $mergeMethod) {
                ForEach(GitHubMergeMethod.allCases) { method in Text(method.label).tag(method) }
            }
            .pickerStyle(.segmented)
            HStack {
                if let execution = store.deliveryExecutions[task.id] {
                    Label(execution.state.capitalized, systemImage: "checkmark.seal")
                        .foregroundStyle(.green)
                }
                Spacer()
                Button("Preview Exact Merge", systemImage: "eye") {
                    Task {
                        await store.previewMergeDelivery(task: task, method: mergeMethod)
                        if store.deliveryPreviews[task.id]?.action.action.kind == "mergePullRequest" {
                            mergeApprovalPresented = true
                        }
                    }
                }
                .disabled(store.deliveryBusyTaskIDs.contains(task.id))
            }
            if case .githubPullRequest(let source) = task.source {
                LabeledContent("Head", value: String(source.headSHA.prefix(12)))
                LabeledContent("Base", value: String(source.baseSHA.prefix(12)))
            } else {
                Label("Only ingested pull request tasks can be merged.", systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.secondary)
            }
            if let error = store.deliveryError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
    }

    private func detailCard<Content: View>(
        _ title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.headline)
            Divider()
            content()
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 10))
    }

    @ViewBuilder private var sourceSummary: some View {
        switch task.source {
        case .githubIssue(let source):
            LabeledContent("GitHub issue", value: "\(source.repositoryFullName)#\(source.number)")
            LabeledContent("Snapshot") { TimestampText(date: source.snapshotAt) }
        case .githubPullRequest(let source):
            LabeledContent("Pull request", value: "\(source.repositoryFullName)#\(source.number)")
            LabeledContent("Base", value: "\(source.baseRef) · \(source.baseSHA.prefix(8))")
            LabeledContent("Head", value: "\(source.headRef) · \(source.headSHA.prefix(8))")
        case .localRequest:
            Text("Local repository request").foregroundStyle(.secondary)
        case .none:
            Text("Legacy task source").foregroundStyle(.secondary)
        }
    }

    private func placeholder(_ title: String, _ description: String, _ image: String) -> some View {
        ContentUnavailableView(title, systemImage: image, description: Text(description))
            .frame(maxWidth: .infinity, minHeight: 280)
    }
}

private enum TaskWorkbenchTab: String, CaseIterable, Identifiable {
    case overview, codex, changes, verification, delivery, merge
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
}
