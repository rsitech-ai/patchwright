import PatchwrightCore
import SwiftUI

struct WorkspaceTableView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var search = ""

    var body: some View {
        Group {
            switch store.contentState(for: store.primarySelection) {
            case .loading:
                ProgressView("Refreshing \(store.primarySelection.title.lowercased())…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .empty:
                ContentUnavailableView(
                    "No \(store.primarySelection.title)",
                    systemImage: store.primarySelection.systemImage,
                    description: Text(emptyDescription)
                )
            case .blocked(let message):
                ContentUnavailableView(
                    "Unable to Load \(store.primarySelection.title)",
                    systemImage: "exclamationmark.triangle",
                    description: Text(message)
                )
            case .ready, .partial:
                table
            }
        }
        .navigationTitle(store.primarySelection.title)
        .searchable(text: $search, placement: .toolbar, prompt: "Search")
        .toolbar { tableToolbar }
        .safeAreaInset(edge: .bottom) {
            if case .partial(let message) = store.contentState(for: store.primarySelection) {
                Label(message, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.bar)
            }
        }
    }

    @ViewBuilder private var table: some View {
        switch store.primarySelection {
        case .queue:
            pullRequestTable
        case .issues:
            issueTable
        case .repositories:
            repositoryTable
        case .activeTasks, .awaitingApproval, .monitoring, .completed:
            taskTable
        }
    }

    private var pullRequests: [GitHubWorkItem] {
        guard !search.isEmpty else { return store.visiblePullRequests }
        return store.visiblePullRequests.filter {
            $0.title.localizedCaseInsensitiveContains(search)
                || $0.repositoryFullName.localizedCaseInsensitiveContains(search)
                || String($0.number).contains(search)
        }
    }

    private var repositories: [GitHubRepository] {
        guard !search.isEmpty else { return store.visibleRepositories }
        return store.visibleRepositories.filter { $0.fullName.localizedCaseInsensitiveContains(search) }
    }

    private var issues: [GitHubWorkItem] {
        guard !search.isEmpty else { return store.visibleIssues }
        return store.visibleIssues.filter {
            $0.title.localizedCaseInsensitiveContains(search)
                || $0.repositoryFullName.localizedCaseInsensitiveContains(search)
                || String($0.number).contains(search)
        }
    }

    private var pullRequestTable: some View {
        Table(pullRequests, selection: $store.selectedWorkItemID) {
            TableColumn("Priority") { item in
                let decision = store.queueDecision(for: item)
                Text(decision?.tier.label ?? store.queueState(for: item).label)
                    .help(decision?.reasons.joined(separator: "\n") ?? "No workflow decision")
            }
            .width(min: 88, ideal: 110)
            TableColumn("Repository", value: \.repositoryFullName)
                .width(min: 130, ideal: 180)
            TableColumn("PR") { item in
                VStack(alignment: .leading, spacing: 2) {
                    Text("#\(item.number) \(item.title)").lineLimit(1)
                    Text(item.author).font(.caption).foregroundStyle(.secondary)
                }
            }
            .width(min: 190, ideal: 300)
            TableColumn("CI") { item in StatusText(value: item.ciHealth ?? "unknown") }
                .width(min: 70, ideal: 82)
            TableColumn("Review") { item in StatusText(value: item.reviewDecision ?? "required") }
                .width(min: 82, ideal: 108)
            TableColumn("Base") { item in
                Label(
                    item.hasConflicts == true ? "Conflict" : (item.baseRef ?? "Unknown"),
                    systemImage: item.hasConflicts == true ? "exclamationmark.triangle.fill" : "arrow.triangle.branch"
                )
                .foregroundStyle(item.hasConflicts == true ? .red : .secondary)
            }
            .width(min: 88, ideal: 108)
            TableColumn("Latest Commit") { item in TimestampText(date: item.headCommittedAt) }
                .width(min: 105, ideal: 130)
            TableColumn("Updated") { item in TimestampText(date: item.updatedAt) }
                .width(min: 105, ideal: 130)
            TableColumn("Task") { item in
                Text(store.assignedTask(for: item)?.state.displayName ?? "Unassigned")
                    .foregroundStyle(store.assignedTask(for: item) == nil ? .secondary : .primary)
            }
            .width(min: 95, ideal: 120)
        }
        .onChange(of: store.selectedWorkItemID) { _, selectedID in
            guard let selectedID, let item = store.githubWorkItems.first(where: { $0.id == selectedID }) else { return }
            Task { await store.selectWorkItem(item) }
        }
    }

    private var repositoryTable: some View {
        Table(repositories, selection: $store.selectedRepositoryID) {
            TableColumn("Repository") { repository in
                Label(repository.fullName, systemImage: repository.private ? "lock" : "shippingbox")
            }
            TableColumn("Open PRs") { repository in
                Text((repository.openPullRequestCount ?? 0).formatted()).monospacedDigit()
            }
            .width(min: 68, ideal: 80)
            TableColumn("Failing") { repository in
                Text((repository.failingCheckCount ?? 0).formatted()).monospacedDigit()
            }
            .width(min: 60, ideal: 72)
            TableColumn("Updated") { repository in TimestampText(date: repository.updatedAt) }
            TableColumn("Pushed") { repository in TimestampText(date: repository.pushedAt) }
            TableColumn("Latest Commit") { repository in
                TimestampText(date: repository.defaultBranchCommittedAt)
            }
        }
        .onChange(of: store.selectedRepositoryID) { _, selectedID in
            guard let selectedID, let repository = store.repositories.first(where: { $0.id == selectedID }) else { return }
            Task { await store.selectRepository(repository) }
        }
    }

    private var issueTable: some View {
        Table(issues, selection: $store.selectedWorkItemID) {
            TableColumn("Repository", value: \.repositoryFullName)
                .width(min: 130, ideal: 180)
            TableColumn("Issue") { item in
                VStack(alignment: .leading, spacing: 2) {
                    Text("#\(item.number) \(item.title)").lineLimit(1)
                    Text(item.author).font(.caption).foregroundStyle(.secondary)
                }
            }
            .width(min: 220, ideal: 360)
            TableColumn("Labels") { item in
                Text(item.labels.isEmpty ? "—" : item.labels.joined(separator: ", ")).lineLimit(1)
            }
            TableColumn("Updated") { item in TimestampText(date: item.updatedAt) }
            TableColumn("Task") { item in
                Text(store.assignedTask(for: item)?.state.displayName ?? "Unassigned")
                    .foregroundStyle(store.assignedTask(for: item) == nil ? .secondary : .primary)
            }
        }
        .onChange(of: store.selectedWorkItemID) { _, selectedID in
            guard let selectedID, let item = store.githubWorkItems.first(where: { $0.id == selectedID }) else { return }
            Task { await store.selectWorkItem(item) }
        }
    }

    private var taskTable: some View {
        Table(filteredTasks, selection: $store.selectedTaskID) {
            TableColumn("Task", value: \.title)
            TableColumn("Repository") { task in
                Text(task.source?.repositoryName ?? URL(fileURLWithPath: task.repositoryPath).lastPathComponent)
            }
            TableColumn("Stage") { task in StatusText(value: task.state.displayName) }
            TableColumn("Updated") { task in TimestampText(date: task.updatedAt) }
        }
        .onChange(of: store.selectedTaskID) { _, selectedID in
            if selectedID != nil { store.selectedWorkItemID = nil }
        }
    }

    private var filteredTasks: [EngineeringTask] {
        let tasks = store.tasks(for: store.primarySelection)
        guard !search.isEmpty else { return tasks }
        return tasks.filter { $0.title.localizedCaseInsensitiveContains(search) }
    }

    @ToolbarContentBuilder private var tableToolbar: some ToolbarContent {
        if store.primarySelection == .queue {
            ToolbarItem {
                Menu(store.selectedWorkflowPreset.label, systemImage: "point.3.connected.trianglepath.dotted") {
                    ForEach(PullRequestWorkflowPreset.allCases) { preset in
                        Button {
                            Task { await store.applyWorkflowPreset(preset) }
                        } label: {
                            Label(preset.label, systemImage: preset == store.selectedWorkflowPreset ? "checkmark" : "circle")
                        }
                    }
                }
                .help("Apply an explainable pull request workflow")
            }
            ToolbarItem {
                Menu("Sort", systemImage: "arrow.up.arrow.down") {
                    ForEach(PullRequestSortKey.allCases, id: \.self) { key in
                        Button(key.label) {
                            store.setPullRequestSort(PullRequestSort(key: key, direction: .descending))
                        }
                    }
                    Divider()
                    Button("Reverse Order") {
                        let current = store.presentationPreferences.pullRequestSort
                        let direction: SortDirection = current.direction == .ascending ? .descending : .ascending
                        store.setPullRequestSort(PullRequestSort(key: current.key, direction: direction))
                    }
                }
                .help("Sort pull requests")
            }
            ToolbarItem {
                Menu("Filter", systemImage: "line.3.horizontal.decrease") {
                    filterButton("Open only", matches: store.presentationPreferences.filter.open == true) { filter in
                        filter.open = filter.open == true ? nil : true
                    }
                    filterButton("Drafts", matches: store.presentationPreferences.filter.draft == true) { filter in
                        filter.draft = filter.draft == true ? nil : true
                    }
                    filterButton("Conflicts", matches: store.presentationPreferences.filter.hasConflicts == true) { filter in
                        filter.hasConflicts = filter.hasConflicts == true ? nil : true
                    }
                    filterButton("Active Codex work", matches: store.presentationPreferences.filter.activeCodexWork == true) { filter in
                        filter.activeCodexWork = filter.activeCodexWork == true ? nil : true
                    }
                    Divider()
                    Button("Clear Filters") { store.setWorkspaceFilter(WorkspaceFilter()) }
                }
                .help("Filter pull requests")
            }
        } else if store.primarySelection == .repositories {
            ToolbarItem {
                Menu("Sort", systemImage: "arrow.up.arrow.down") {
                    ForEach(RepositorySortKey.allCases, id: \.self) { key in
                        Button(key.label) {
                            store.setRepositorySort(RepositorySort(key: key, direction: .descending))
                        }
                    }
                }
                .help("Sort repositories")
            }
        }
    }

    private func filterButton(
        _ label: String,
        matches: Bool,
        update: @escaping (inout WorkspaceFilter) -> Void
    ) -> some View {
        Button {
            var filter = store.presentationPreferences.filter
            update(&filter)
            store.setWorkspaceFilter(filter)
        } label: {
            Label(label, systemImage: matches ? "checkmark" : "circle")
        }
    }

    private var emptyDescription: String {
        switch store.primarySelection {
        case .queue: "Sync GitHub to ingest open pull requests, or clear active filters."
        case .issues: "Sync GitHub to ingest open issues."
        case .repositories: "Sync GitHub to ingest repositories."
        case .activeTasks: "Create a task from an ingested issue, pull request, or local repository."
        case .awaitingApproval: "No task currently needs an approval."
        case .monitoring: "No delivered task is waiting on checks or review."
        case .completed: "Completed tasks will remain available here."
        }
    }
}

private struct StatusText: View {
    let value: String
    var body: some View {
        Text(value.replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression).capitalized)
            .lineLimit(1)
    }
}

private extension PullRequestQueueState {
    var label: String {
        rawValue.replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression).capitalized
    }
}

private extension PullRequestSortKey {
    static var allCases: [Self] {
        [.queuePriority, .recentlyUpdated, .latestHeadCommit, .latestReviewActivity, .ciHealth,
         .reviewState, .createdNewest, .createdOldest, .changeSize, .number]
    }

    var label: String {
        rawValue.replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression).capitalized
    }
}

private extension RepositorySortKey {
    static var allCases: [Self] {
        [.queuePriority, .recentlyUpdated, .recentlyPushed, .latestDefaultBranchCommit,
         .openPullRequestCount, .failingCheckCount, .name]
    }

    var label: String {
        rawValue.replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression).capitalized
    }
}

private extension TaskSource {
    var repositoryName: String? {
        switch self {
        case .githubIssue(let source): source.repositoryFullName
        case .githubPullRequest(let source): source.repositoryFullName
        case .localRequest: nil
        }
    }
}
