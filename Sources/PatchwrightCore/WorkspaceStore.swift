import Combine
import Foundation

@MainActor
public final class WorkspaceStore: ObservableObject {
    @Published public private(set) var connectionState: EngineConnectionState = .disconnected
    @Published public private(set) var tasks: [EngineeringTask] = []
    @Published public var primarySelection: WorkspaceSection = .queue {
        didSet {
            guard primarySelection != oldValue else { return }
            clearDetailSelection()
        }
    }
    @Published public var selectedTaskID: EngineeringTask.ID?
    @Published public var selectedRepositoryID: GitHubRepository.ID?
    @Published public var selectedWorkItemID: GitHubWorkItem.ID?
    @Published public private(set) var githubStatus: GitHubStatus?
    @Published public private(set) var repositories: [GitHubRepository] = []
    @Published public private(set) var githubWorkItems: [GitHubWorkItem] = []
    @Published public private(set) var selectedRepository: GitHubRepositorySnapshot?
    @Published public private(set) var githubSyncSummary: GitHubSyncSummary?
    @Published public private(set) var isSyncingGitHub = false
    @Published public private(set) var conversionPreview: ConversionPreview?
    @Published public private(set) var isConvertingGitHubItem = false
    @Published public private(set) var conversionError: String?
    @Published public private(set) var presentationPreferences: WorkspacePresentationPreferences
    @Published public var githubError: String?
    public let engine: any EngineServing
    private let healthRetryAttempts: Int
    private let healthRetryDelay: Duration
    private let preferences: any WorkspacePreferencesPersisting
    private var preferencesWorkspaceID = "global"

    public init(
        engine: any EngineServing,
        healthRetryAttempts: Int = 20,
        healthRetryDelay: Duration = .milliseconds(100),
        preferences: any WorkspacePreferencesPersisting = UserDefaultsWorkspacePreferences()
    ) {
        self.engine = engine
        self.healthRetryAttempts = max(1, healthRetryAttempts)
        self.healthRetryDelay = healthRetryDelay
        self.preferences = preferences
        presentationPreferences = preferences.load(workspaceID: "global") ?? WorkspacePresentationPreferences()
    }

    public func refreshHealth() async {
        connectionState = .connecting
        var lastError: Error?
        for attempt in 0..<healthRetryAttempts {
            do {
                let health = try await engine.call(method: "system.health", params: [:], as: HealthResponse.self)
                connectionState = .connected(health.version)
                await refreshTasks()
                await refreshGitHub()
                return
            } catch {
                lastError = error
                if attempt < healthRetryAttempts - 1 { try? await Task.sleep(for: healthRetryDelay) }
            }
        }
        connectionState = .failed(lastError?.localizedDescription ?? "Engine unavailable")
    }

    public func refreshGitHub() async {
        do {
            githubStatus = try await engine.call(method: "github.status", params: [:], as: GitHubStatus.self)
            repositories = try await engine.call(method: "github.repositories", params: [:], as: [GitHubRepository].self)
            githubWorkItems = try await engine.call(method: "github.queue", params: [:], as: [GitHubWorkItem].self)
            githubError = nil
        } catch {
            githubError = error.localizedDescription
        }
    }

    public func refreshTasks() async {
        do {
            tasks = try await engine.call(method: "task.list", params: [:], as: [EngineeringTask].self)
        } catch {
            connectionState = .failed(error.localizedDescription)
        }
    }

    public func syncGitHub(repositoryLimit: Int = 100, resourceLimit: Int = 1_000) async {
        let selectedRepositoryName = selectedRepository?.repository.fullName
        isSyncingGitHub = true
        let poller = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(2))
                guard !Task.isCancelled else { return }
                await self?.refreshGitHub()
            }
        }
        defer {
            poller.cancel()
            isSyncingGitHub = false
        }
        do {
            githubSyncSummary = try await engine.call(
                method: "github.sync",
                params: ["repositoryLimit": String(repositoryLimit), "resourceLimit": String(resourceLimit)],
                as: GitHubSyncSummary.self
            )
            await refreshGitHub()
            if let selectedRepositoryName,
               let repository = repositories.first(where: { $0.fullName == selectedRepositoryName }) {
                await selectRepository(repository)
            }
        } catch {
            githubError = error.localizedDescription
        }
    }

    public func selectRepository(_ repository: GitHubRepository) async {
        do {
            selectedRepository = try await engine.call(
                method: "github.repository", params: ["fullName": repository.fullName],
                as: GitHubRepositorySnapshot?.self
            )
            selectedRepositoryID = repository.id
            selectedWorkItemID = nil
            selectedTaskID = nil
            loadPresentationPreferences(workspaceID: repository.fullName)
            githubError = nil
        } catch {
            githubError = error.localizedDescription
        }
    }

    public func selectWorkItem(_ item: GitHubWorkItem) async {
        if selectedRepository?.repository.fullName != item.repositoryFullName,
           let repository = repositories.first(where: { $0.fullName == item.repositoryFullName }) {
            await selectRepository(repository)
        }
        selectedWorkItemID = item.id
        selectedTaskID = nil
    }

    public func createTask(title: String, repositoryPath: String) async {
        do {
            let task = try await engine.call(
                method: "task.create",
                params: ["title": title, "repositoryPath": repositoryPath],
                as: EngineeringTask.self
            )
            tasks.append(task)
            selectedTaskID = task.id
        } catch {
            connectionState = .failed(error.localizedDescription)
        }
    }

    public var attentionTaskCount: Int {
        tasks.lazy.filter(\.requiresAttention).count
    }

    public var selectedTask: EngineeringTask? {
        tasks.first { $0.id == selectedTaskID }
    }

    public var selectedWorkItem: GitHubWorkItem? {
        githubWorkItems.first { $0.id == selectedWorkItemID }
    }

    public func tasks(for section: WorkspaceSection) -> [EngineeringTask] {
        switch section {
        case .activeTasks:
            tasks.filter { ![TaskState.completed, .cancelled, .failed].contains($0.state) }
        case .awaitingApproval:
            tasks.filter { [.awaitingPreparationApproval, .awaitingDeliveryApproval, .awaitingMergeApproval].contains($0.state) }
        case .monitoring:
            tasks.filter { $0.state == .monitoring }
        case .completed:
            tasks.filter { $0.state == .completed }
        case .queue, .issues, .repositories:
            []
        }
    }

    public var visibleRepositories: [GitHubRepository] {
        let records = repositories.map { RepositoryQueueRecord(repository: $0) }
        let ids = sortRepositories(records, by: presentationPreferences.repositorySort).map(\.id)
        let byID = Dictionary(uniqueKeysWithValues: repositories.map { ($0.id, $0) })
        return ids.compactMap { byID[$0] }
    }

    public var visiblePullRequests: [GitHubWorkItem] {
        let pulls = githubWorkItems.filter { $0.kind == .pullRequest }
        let records = pulls.map { item in
            PullRequestQueueRecord(
                workItem: item,
                queueState: queueState(for: item),
                activeCodexWork: assignedTask(for: item) != nil
            )
        }
        let matching = records.filter { presentationPreferences.filter.matches($0, now: Date()) }
        let ids = sortPullRequests(matching, by: presentationPreferences.pullRequestSort).map(\.id)
        let byID = Dictionary(uniqueKeysWithValues: pulls.map { ($0.id, $0) })
        return ids.compactMap { byID[$0] }
    }

    public var visibleIssues: [GitHubWorkItem] {
        githubWorkItems
            .filter { $0.kind == .issue && $0.state.caseInsensitiveCompare("open") == .orderedSame }
            .sorted {
                if $0.updatedAt != $1.updatedAt { return $0.updatedAt > $1.updatedAt }
                if $0.repositoryFullName != $1.repositoryFullName {
                    return $0.repositoryFullName < $1.repositoryFullName
                }
                if $0.number != $1.number { return $0.number < $1.number }
                return $0.id < $1.id
            }
    }

    public func contentState(for section: WorkspaceSection) -> WorkspaceContentState {
        let hasContent = switch section {
        case .queue: !visiblePullRequests.isEmpty
        case .issues: !visibleIssues.isEmpty
        case .repositories: !visibleRepositories.isEmpty
        case .activeTasks, .awaitingApproval, .monitoring, .completed: !tasks(for: section).isEmpty
        }
        let error = section == .queue || section == .issues || section == .repositories ? githubError : nil
        return .resolve(hasContent: hasContent, loading: isSyncingGitHub, error: error)
    }

    public func loadPresentationPreferences(workspaceID: String) {
        preferencesWorkspaceID = workspaceID
        presentationPreferences = preferences.load(workspaceID: workspaceID) ?? WorkspacePresentationPreferences()
    }

    public func setRepositorySort(_ sort: RepositorySort) {
        presentationPreferences.repositorySort = sort
        savePresentationPreferences()
    }

    public func setPullRequestSort(_ sort: PullRequestSort) {
        presentationPreferences.pullRequestSort = sort
        savePresentationPreferences()
    }

    public func setWorkspaceFilter(_ filter: WorkspaceFilter) {
        presentationPreferences.filter = filter
        savePresentationPreferences()
    }

    private func savePresentationPreferences() {
        preferences.save(presentationPreferences, workspaceID: preferencesWorkspaceID)
    }

    private func clearDetailSelection() {
        selectedTaskID = nil
        selectedRepositoryID = nil
        selectedWorkItemID = nil
        selectedRepository = nil
        conversionPreview = nil
        conversionError = nil
    }

    public func assignedTask(for item: GitHubWorkItem) -> EngineeringTask? {
        tasks.first { task in
            switch task.source {
            case .githubIssue(let source):
                source.repositoryFullName == item.repositoryFullName && source.number == item.number
            case .githubPullRequest(let source):
                source.repositoryFullName == item.repositoryFullName && source.number == item.number
            case .localRequest, .none:
                false
            }
        }
    }

    public func queueState(for item: GitHubWorkItem) -> PullRequestQueueState {
        if item.hasConflicts == true { return .blocked }
        if item.reviewDecision == "changesRequested" || item.ciHealth == "failing" { return .needsWork }
        if item.reviewDecision == "approved" && item.ciHealth == "passing" { return .ready }
        if item.draft { return .inbox }
        return .assessed
    }

    public func previewTask(from item: GitHubWorkItem) async {
        isConvertingGitHubItem = true
        defer { isConvertingGitHubItem = false }
        do {
            conversionPreview = try await engine.previewTaskFromGitHub(item)
            conversionError = nil
        } catch EngineError.remote(let code, _) where code == -32033 {
            do {
                guard let repository = repositories.first(where: { $0.fullName == item.repositoryFullName }) else {
                    throw EngineError.remote(code: -32020, message: "Refresh the repository snapshot before creating a task.")
                }
                _ = try await engine.bindRepository(repository)
                conversionPreview = try await engine.previewTaskFromGitHub(item)
                conversionError = nil
            } catch {
                conversionPreview = nil
                conversionError = error.localizedDescription
            }
        } catch {
            conversionPreview = nil
            conversionError = error.localizedDescription
        }
    }

    public func createTask(from item: GitHubWorkItem) async {
        guard let preview = conversionPreview,
              preview.repositoryFullName == item.repositoryFullName,
              preview.itemNumber == item.number,
              preview.sourceUpdatedAt == item.updatedAt else {
            conversionError = "Preview and confirm this GitHub item before creating a task."
            return
        }
        isConvertingGitHubItem = true
        defer { isConvertingGitHubItem = false }
        do {
            let outcome = try await engine.createTaskFromGitHub(item)
            if !tasks.contains(where: { $0.id == outcome.task.id }) {
                tasks.append(outcome.task)
            }
            selectedTaskID = outcome.task.id
            conversionPreview = nil
            conversionError = nil
        } catch {
            conversionPreview = nil
            conversionError = error.localizedDescription
        }
    }
}
