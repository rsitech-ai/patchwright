import PatchwrightCore
import SwiftUI

struct TaskDetailView: View {
    let task: EngineeringTask
    @State private var tab: TaskWorkbenchTab = .overview

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            Picker("Task workbench", selection: $tab) {
                ForEach(TaskWorkbenchTab.allCases) { tab in
                    Text(tab.title).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(12)
            Divider()
            tabContent
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
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    switch tab {
                    case .overview: overview
                    case .codex: placeholder("Codex Thread", "The supervised embedded thread attaches when preparation starts.", "bubble.left.and.text.bubble.right")
                    case .changes: placeholder("Changes", "Worktree file changes and diffs will appear here.", "doc.badge.gearshape")
                    case .verification: placeholder("Verification", "Commands, checks, findings, and evidence will appear here.", "checkmark.shield")
                    case .delivery: placeholder("Delivery", "Approval-bound branch, comment, review, check, and draft PR actions will appear here.", "paperplane")
                    case .merge: placeholder("Merge", "Exact-SHA merge readiness and the separate merge approval will appear here.", "arrow.triangle.merge")
                    }
                }
                .padding(20)
                .frame(maxWidth: 860, alignment: .leading)
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
