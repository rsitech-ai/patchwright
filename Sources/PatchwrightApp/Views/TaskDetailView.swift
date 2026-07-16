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
    @State private var reviewEvent = GitHubReviewEvent.comment

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
        .task(id: task.id) {
            while !Task.isCancelled {
                await store.refreshTaskTimeline(taskID: task.id)
                await store.refreshTaskWorktree(taskID: task.id)
                await store.refreshTaskRepository(task: task)
                try? await Task.sleep(for: .seconds(2))
            }
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
                        case .changes: changesPanel
                        case .verification: verificationPanel
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
                lifecycleControls
                if let error = store.taskLifecycleError {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            detailCard("Progress") { lifecycleTimeline }
            detailCard("Source") {
                sourceSummary
                if task.source != nil, task.state != .completed {
                    Divider()
                    HStack {
                        VStack(alignment: .leading, spacing: 3) {
                            Text("Remote completion")
                                .font(.subheadline.weight(.semibold))
                            Text("Refresh GitHub and complete this task only when its exact captured issue or PR outcome is confirmed.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("Reconcile with GitHub") {
                            Task { await store.reconcileTaskWithGitHub(task) }
                        }
                        .disabled(store.taskLifecycleBusyTaskIDs.contains(task.id))
                    }
                }
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

    @ViewBuilder private var lifecycleControls: some View {
        let busy = store.taskLifecycleBusyTaskIDs.contains(task.id)
        HStack {
            if busy {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel("Task operation in progress")
            }
            Spacer()
            if task.state == .discovered {
                Button("Assess & Plan", systemImage: "list.bullet.clipboard") {
                    Task { await store.planTask(taskID: task.id) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(busy)
            } else if task.state == .awaitingPreparationApproval {
                Button("Approve & Prepare Worktree", systemImage: "checkmark.shield.fill") {
                    Task { await store.prepareTask(taskID: task.id) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(busy)
                .help("Create an isolated task branch at the captured source SHA")
            } else if [.preparing, .implementing].contains(task.state) {
                Label("Worktree ready", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            }
        }
    }

    private var lifecycleTimeline: some View {
        let events = store.taskTimelineByTask[task.id] ?? [task]
        return VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(events.enumerated()), id: \.offset) { index, event in
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Image(systemName: index == events.indices.last ? "circle.inset.filled" : "checkmark.circle.fill")
                        .foregroundStyle(index == events.indices.last ? Color.accentColor : .green)
                    Text(event.state.displayName).font(.callout.weight(.medium))
                    Spacer()
                    TimestampText(date: event.updatedAt)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var deliveryPanel: some View {
        Group {
            detailCard("Task branch") {
                Text("Pushes use an ephemeral GitHub App credential and can only target the isolated task branch. The default branch is changed only by an exact-SHA merge.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let worktree = store.worktreeByTask[task.id] {
                    LabeledContent("Branch", value: worktree.branch)
                    LabeledContent("Head", value: String(worktree.headSHA.prefix(12)))
                    LabeledContent("Working tree", value: worktree.dirty ? "Uncommitted changes" : "Clean")
                    HStack {
                        Spacer()
                        Button("Preview Push Branch", systemImage: "arrow.up.circle") {
                            preview(
                                GitHubActionPayload(
                                    kind: "pushIntent",
                                    branch: worktree.branch,
                                    headSha: worktree.headSHA
                                )
                            )
                        }
                        .disabled(worktree.dirty || store.deliveryBusyTaskIDs.contains(task.id))
                    }
                } else {
                    Label("Prepare the worktree before pushing a task branch.", systemImage: "info.circle")
                        .foregroundStyle(.secondary)
                }
            }
            detailCard("Comment or review") {
                Text("Every write is previewed, approved, and executed as a separate exact action.")
                .font(.caption)
                .foregroundStyle(.secondary)
                TextEditor(text: $deliveryBody)
                    .accessibilityLabel("GitHub delivery message")
                    .font(.body)
                    .frame(minHeight: 120)
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                if case .githubPullRequest = task.source {
                    Picker("Review", selection: $reviewEvent) {
                        ForEach(GitHubReviewEvent.allCases) { event in Text(event.label).tag(event) }
                    }
                    .pickerStyle(.segmented)
                }
                HStack {
                    if let execution = store.deliveryExecutions[task.id] {
                        Label(execution.state.capitalized, systemImage: "checkmark.seal")
                            .foregroundStyle(.green)
                    }
                    Spacer()
                    Button("Preview Comment", systemImage: "bubble.left") {
                        Task {
                            await store.previewCommentDelivery(task: task, body: deliveryBody)
                            if store.deliveryPreviews[task.id] != nil { deliveryApprovalPresented = true }
                        }
                    }
                    .disabled(deliveryTextIsEmpty || store.deliveryBusyTaskIDs.contains(task.id))
                    if case .githubPullRequest(let source) = task.source {
                        Button("Preview Review", systemImage: "checkmark.bubble") {
                            preview(
                                GitHubActionPayload(
                                    kind: "review",
                                    body: deliveryBody,
                                    pullRequestNumber: source.number,
                                    expectedHeadSha: source.headSHA,
                                    event: reviewEvent.rawValue,
                                    inlineComments: []
                                )
                            )
                        }
                        .disabled(deliveryTextIsEmpty || store.deliveryBusyTaskIDs.contains(task.id))
                    }
                }
            }
            if case .githubPullRequest(let source) = task.source {
                reviewThreadActions(source: source)
            }
            deliveryActions
            if let error = store.deliveryError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
    }

    private func reviewThreadActions(source: GitHubPullRequestTaskSource) -> some View {
        let threads = store.repositorySnapshotByTask[task.id]?.discussions.filter {
            $0.itemNumber == source.number && $0.kind == "reviewThread"
        } ?? []
        return detailCard("Review threads") {
            if threads.isEmpty {
                Text("No review threads ingested.").foregroundStyle(.secondary)
            } else {
                ForEach(threads) { thread in
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Label(
                                thread.threadResolved == true ? "Resolved" : "Unresolved",
                                systemImage: thread.threadResolved == true ? "checkmark.circle.fill" : "bubble.left.and.exclamationmark.bubble.right"
                            )
                            .foregroundStyle(thread.threadResolved == true ? .green : .orange)
                            Spacer()
                            if thread.threadResolved != true, let threadID = thread.threadNodeID {
                                Button("Preview Resolve") {
                                    preview(
                                        GitHubActionPayload(
                                            kind: "resolveReviewThread",
                                            pullRequestNumber: source.number,
                                            threadId: threadID,
                                            expectedHeadSha: source.headSHA
                                        )
                                    )
                                }
                            }
                        }
                        if thread.threadResolved != true, thread.viewerCanResolve == false {
                            Text("GitHub requires signed-in user authority for this thread. Patchwright revalidates that authority at execution without storing the user token.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        if let path = thread.path {
                            Text("\(path)\(thread.line.map { ":\($0)" } ?? "")")
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                        }
                        Text(thread.body ?? "No written comment").font(.callout)
                    }
                    if thread.id != threads.last?.id { Divider() }
                }
            }
        }
    }

    @ViewBuilder private var deliveryActions: some View {
        if case .githubIssue(let source) = task.source {
            detailCard("Resolve issue") {
                Text("Open a draft pull request from the pushed task branch, or close the issue as completed after its outcome is delivered.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Spacer()
                    if let worktree = store.worktreeByTask[task.id],
                       let repository = store.repositories.first(where: { $0.id == source.repositoryID }) {
                        Button("Preview Draft PR", systemImage: "arrow.triangle.pull") {
                            preview(
                                GitHubActionPayload(
                                    kind: "draftPullRequest",
                                    body: deliveryTextIsEmpty ? "Resolves #\(source.number)" : deliveryBody,
                                    title: task.title,
                                    head: worktree.branch,
                                    base: repository.defaultBranch
                                )
                            )
                        }
                    }
                    Button("Preview Close Issue", systemImage: "checkmark.circle") {
                        preview(GitHubActionPayload(kind: "closeIssue", issueNumber: source.number))
                    }
                }
            }
        } else if case .githubPullRequest(let source) = task.source {
            detailCard("Pull request operations") {
                Text("Update or close the pull request as exact approval-bound actions. Merge remains in the separate Merge tab.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Spacer()
                    Button("Preview Update Branch", systemImage: "arrow.triangle.2.circlepath") {
                        preview(
                            GitHubActionPayload(
                                kind: "updatePullRequestBranch",
                                pullRequestNumber: source.number,
                                expectedHeadSha: source.headSHA
                            )
                        )
                    }
                    Button("Preview Ready for Review", systemImage: "person.crop.circle.badge.checkmark") {
                        preview(
                            GitHubActionPayload(
                                kind: "readyPullRequest",
                                pullRequestNumber: source.number,
                                expectedHeadSha: source.headSHA
                            )
                        )
                    }
                    Button("Preview Close PR", systemImage: "xmark.circle") {
                        preview(
                            GitHubActionPayload(
                                kind: "closePullRequest",
                                pullRequestNumber: source.number
                            )
                        )
                    }
                }
            }
        }
    }

    private var changesPanel: some View {
        Group {
            if let worktree = store.worktreeByTask[task.id] {
                detailCard("Worktree") {
                    LabeledContent("Path", value: worktree.root)
                    LabeledContent("Branch", value: worktree.branch)
                    LabeledContent("HEAD", value: worktree.headSHA)
                    LabeledContent("Status", value: worktree.dirty ? "Uncommitted changes" : "Clean")
                }
            } else {
                placeholder("No Worktree Yet", "Approve preparation to create the isolated task branch.", "doc.badge.gearshape")
            }
        }
    }

    private var verificationPanel: some View {
        detailCard("Delivery readiness") {
            Text("Patchwright requires a clean committed worktree before exposing approval-bound GitHub delivery.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let worktree = store.worktreeByTask[task.id] {
                LabeledContent("Branch", value: worktree.branch)
                LabeledContent("Commit", value: String(worktree.headSHA.prefix(12)))
                LabeledContent("Worktree", value: worktree.dirty ? "Dirty" : "Clean")
                HStack {
                    Spacer()
                    Button("Complete Verification & Review", systemImage: "checkmark.shield.fill") {
                        Task { await store.readyTaskForDelivery(taskID: task.id) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(
                        worktree.dirty
                            || store.taskLifecycleBusyTaskIDs.contains(task.id)
                            || task.state == .awaitingDeliveryApproval
                    )
                }
            } else {
                Label("No prepared worktree is available.", systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.secondary)
            }
            if task.state == .awaitingDeliveryApproval {
                Label("Ready for exact GitHub delivery approval", systemImage: "checkmark.seal.fill")
                    .foregroundStyle(.green)
            }
        }
    }

    private var deliveryTextIsEmpty: Bool {
        deliveryBody.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func preview(_ action: GitHubActionPayload) {
        Task {
            await store.previewDelivery(task: task, action: action)
            if store.deliveryPreviews[task.id] != nil { deliveryApprovalPresented = true }
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
