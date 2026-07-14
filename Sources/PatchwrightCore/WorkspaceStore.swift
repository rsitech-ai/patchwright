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
    @Published public private(set) var githubSyncJob: GitHubSyncJob?
    @Published public private(set) var queueDecisions: [PullRequestQueueDecision] = []
    @Published public private(set) var selectedWorkflowPreset: PullRequestWorkflowPreset = .quickWins
    @Published public private(set) var isSyncingGitHub = false
    @Published public private(set) var conversionPreview: ConversionPreview?
    @Published public private(set) var isConvertingGitHubItem = false
    @Published public private(set) var conversionError: String?
    @Published public private(set) var codexStatuses: [UUID: CodexRuntimeStatus] = [:]
    @Published public private(set) var codexEventsByTask: [UUID: [CodexEvent]] = [:]
    @Published public private(set) var codexApprovalsByTask: [UUID: [CodexRuntimeApproval]] = [:]
    @Published public private(set) var codexBusyTaskIDs: Set<UUID> = []
    @Published public private(set) var codexError: String?
    @Published public private(set) var taskTimelineByTask: [UUID: [EngineeringTask]] = [:]
    @Published public private(set) var taskLifecycleBusyTaskIDs: Set<UUID> = []
    @Published public private(set) var taskLifecycleError: String?
    @Published public private(set) var worktreeByTask: [UUID: WorktreeInspection] = [:]
    @Published public private(set) var deliveryPreviews: [UUID: DeliveryPreview] = [:]
    @Published public private(set) var deliveryApprovals: [UUID: DeliveryApproval] = [:]
    @Published public private(set) var deliveryExecutions: [UUID: DeliveryExecution] = [:]
    @Published public private(set) var deliveryBusyTaskIDs: Set<UUID> = []
    @Published public private(set) var deliveryError: String?
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
            if let decisions = try? await engine.call(
                method: "github.queue.decisions", params: [:], as: [PullRequestQueueDecision].self
            ) {
                queueDecisions = decisions
            }
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

    public func refreshTaskTimeline(taskID: UUID) async {
        do {
            taskTimelineByTask[taskID] = try await engine.taskTimeline(taskID: taskID)
            taskLifecycleError = nil
        } catch {
            taskLifecycleError = error.localizedDescription
        }
    }

    public func refreshTaskWorktree(taskID: UUID) async {
        do {
            worktreeByTask[taskID] = try await engine.inspectTaskWorktree(taskID: taskID)
        } catch {
            worktreeByTask[taskID] = nil
        }
    }

    public func planTask(taskID: UUID) async {
        guard !taskLifecycleBusyTaskIDs.contains(taskID) else { return }
        taskLifecycleBusyTaskIDs.insert(taskID)
        defer { taskLifecycleBusyTaskIDs.remove(taskID) }
        do {
            replaceTask(try await engine.planTask(taskID: taskID))
            taskLifecycleError = nil
            await refreshTaskTimeline(taskID: taskID)
        } catch {
            taskLifecycleError = error.localizedDescription
        }
    }

    public func prepareTask(taskID: UUID) async {
        guard !taskLifecycleBusyTaskIDs.contains(taskID) else { return }
        taskLifecycleBusyTaskIDs.insert(taskID)
        defer { taskLifecycleBusyTaskIDs.remove(taskID) }
        do {
            replaceTask(try await engine.prepareTask(taskID: taskID))
            taskLifecycleError = nil
            await refreshTaskTimeline(taskID: taskID)
            await refreshTaskWorktree(taskID: taskID)
        } catch {
            taskLifecycleError = error.localizedDescription
        }
    }

    public func readyTaskForDelivery(taskID: UUID) async {
        guard !taskLifecycleBusyTaskIDs.contains(taskID) else { return }
        taskLifecycleBusyTaskIDs.insert(taskID)
        defer { taskLifecycleBusyTaskIDs.remove(taskID) }
        do {
            replaceTask(try await engine.readyTaskForDelivery(taskID: taskID))
            taskLifecycleError = nil
            await refreshTaskTimeline(taskID: taskID)
            await refreshTaskWorktree(taskID: taskID)
        } catch {
            taskLifecycleError = error.localizedDescription
        }
    }

    public func syncGitHub(repositoryLimit: Int = 100, resourceLimit: Int = 1_000) async {
        guard !isSyncingGitHub else { return }
        let selectedRepositoryName = selectedRepository?.repository.fullName
        isSyncingGitHub = true
        defer { isSyncingGitHub = false }
        do {
            var job = try await engine.call(
                method: "github.sync.start",
                params: ["repositoryLimit": String(repositoryLimit), "resourceLimit": String(resourceLimit)],
                as: GitHubSyncJob.self
            )
            githubSyncJob = job
            var refreshCounter = 0
            while !job.state.isTerminal {
                try await Task.sleep(for: .milliseconds(350))
                job = try await engine.call(
                    method: "github.sync.status",
                    params: ["jobId": job.id.uuidString],
                    as: GitHubSyncJob.self
                )
                githubSyncJob = job
                refreshCounter += 1
                if refreshCounter.isMultiple(of: 6) { await refreshGitHub() }
            }
            if job.state == .failed || job.state == .interrupted {
                githubError = job.summary
                return
            }
            if job.state == .cancelled { githubError = nil }
            await refreshGitHub()
            await applyWorkflowPreset(selectedWorkflowPreset)
            if let selectedRepositoryName,
               let repository = repositories.first(where: { $0.fullName == selectedRepositoryName }) {
                await selectRepository(repository)
            }
        } catch {
            githubError = error.localizedDescription
        }
    }

    public func cancelGitHubSync() async {
        guard let job = githubSyncJob, !job.state.isTerminal else { return }
        do {
            githubSyncJob = try await engine.call(
                method: "github.sync.cancel",
                params: ["jobId": job.id.uuidString],
                as: GitHubSyncJob.self
            )
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

    public func previewCommentDelivery(task: EngineeringTask, body: String) async {
        let body = body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty, !deliveryBusyTaskIDs.contains(task.id) else { return }
        let number: UInt64
        switch task.source {
        case .githubIssue(let source):
            number = source.number
        case .githubPullRequest(let source):
            number = source.number
        default:
            deliveryError = "GitHub delivery requires an ingested issue or pull request task."
            return
        }
        await previewDelivery(task: task, action: GitHubActionPayload(commentNumber: number, body: body))
    }

    public func previewDelivery(task: EngineeringTask, action: GitHubActionPayload) async {
        guard !deliveryBusyTaskIDs.contains(task.id) else { return }
        let identity: (UInt64, String, String?, String?, Date)
        switch task.source {
        case .githubIssue(let source):
            let baseSHA = repositories.first(where: { $0.id == source.repositoryID })?.defaultBranchSHA
            identity = (source.repositoryID, source.repositoryFullName, nil, baseSHA, source.snapshotAt)
        case .githubPullRequest(let source):
            identity = (
                source.repositoryID, source.repositoryFullName, source.headSHA,
                source.baseSHA, source.snapshotAt
            )
        default:
            deliveryError = "GitHub delivery requires an ingested issue or pull request task."
            return
        }
        guard let installationID = repositories.first(where: { $0.id == identity.0 })?.installationID else {
            deliveryError = "Verify Patchwright GitHub App access before preparing delivery."
            return
        }
        deliveryBusyTaskIDs.insert(task.id)
        defer { deliveryBusyTaskIDs.remove(task.id) }
        do {
            let draft = GitHubActionPreviewDraft(
                remote: GitHubRemoteIdentity(
                    repositoryId: identity.0,
                    installationId: installationID,
                    repositoryFullName: identity.1
                ),
                action: action,
                expectedHeadSha: identity.2,
                expectedBaseSha: identity.3,
                snapshotGeneration: max(1, UInt64(identity.4.timeIntervalSince1970))
            )
            deliveryPreviews[task.id] = try await engine.previewDelivery(taskID: task.id, draft: draft)
            deliveryApprovals[task.id] = nil
            deliveryExecutions[task.id] = nil
            deliveryError = nil
        } catch {
            deliveryError = error.localizedDescription
        }
    }

    public func previewMergeDelivery(task: EngineeringTask, method: GitHubMergeMethod) async {
        guard !deliveryBusyTaskIDs.contains(task.id) else { return }
        guard case .githubPullRequest(let source) = task.source else {
            deliveryError = "Merge approval requires an ingested pull request task."
            return
        }
        await previewDelivery(
            task: task,
            action: GitHubActionPayload(
                pullRequestNumber: source.number,
                expectedHeadSha: source.headSHA,
                method: method
            )
        )
    }

    public func approveDelivery(taskID: UUID) async {
        guard let preview = deliveryPreviews[taskID], !deliveryBusyTaskIDs.contains(taskID) else { return }
        deliveryBusyTaskIDs.insert(taskID)
        defer { deliveryBusyTaskIDs.remove(taskID) }
        do {
            deliveryApprovals[taskID] = try await engine.approveDelivery(
                preview,
                approvedBy: ProcessInfo.processInfo.userName
            )
            deliveryError = nil
        } catch {
            deliveryError = error.localizedDescription
        }
    }

    public func executeDelivery(taskID: UUID) async {
        guard let preview = deliveryPreviews[taskID],
              let approval = deliveryApprovals[taskID],
              !deliveryBusyTaskIDs.contains(taskID) else { return }
        deliveryBusyTaskIDs.insert(taskID)
        defer { deliveryBusyTaskIDs.remove(taskID) }
        do {
            deliveryExecutions[taskID] = try await engine.executeDelivery(preview, approvalID: approval.id)
            deliveryError = nil
            await refreshTasks()
            await refreshTaskTimeline(taskID: taskID)
        } catch {
            deliveryError = error.localizedDescription
        }
    }

    public func codexStatus(for taskID: UUID) -> CodexRuntimeStatus? {
        codexStatuses[taskID]
    }

    public func codexTranscript(for taskID: UUID) -> CodexTranscript {
        CodexTranscript(events: codexEventsByTask[taskID] ?? [])
    }

    public func refreshCodex(taskID: UUID) async {
        do {
            let status = try await engine.codexStatus(taskID: taskID)
            let cursor = codexEventsByTask[taskID]?.last?.sequence ?? 0
            let newEvents = try await engine.codexEvents(taskID: taskID, after: cursor)
            codexApprovalsByTask[taskID] = try await engine.codexApprovals(taskID: taskID)
            if !newEvents.isEmpty {
                let existing = codexEventsByTask[taskID] ?? []
                let seen = Set(existing.map(\.sequence))
                codexEventsByTask[taskID] = (existing + newEvents.filter { !seen.contains($0.sequence) })
                    .sorted { $0.sequence < $1.sequence }
            }
            codexStatuses[taskID] = status
            codexError = nil
            if newEvents.contains(where: { $0.kind == .turnCompleted }) {
                await refreshTasks()
            }
        } catch {
            codexError = error.localizedDescription
        }
    }

    public func resolveCodexApproval(_ approval: CodexRuntimeApproval, approve: Bool) async {
        guard approval.state == .pending, !codexBusyTaskIDs.contains(approval.taskId) else { return }
        codexBusyTaskIDs.insert(approval.taskId)
        defer { codexBusyTaskIDs.remove(approval.taskId) }
        do {
            _ = try await engine.resolveCodexApproval(taskID: approval.taskId, approvalID: approval.id, processGeneration: approval.processGeneration, approve: approve)
            codexError = nil
            await refreshCodex(taskID: approval.taskId)
        } catch { codexError = error.localizedDescription }
    }

    public func interruptCodex(taskID: UUID, cancel: Bool) async {
        guard !codexBusyTaskIDs.contains(taskID) else { return }
        codexBusyTaskIDs.insert(taskID)
        defer { codexBusyTaskIDs.remove(taskID) }
        do {
            codexStatuses[taskID] = try await engine.interruptCodex(taskID: taskID, cancel: cancel)
            codexError = nil
            await refreshTasks()
        } catch { codexError = error.localizedDescription }
    }

    public func startCodex(taskID: UUID) async {
        guard !codexBusyTaskIDs.contains(taskID) else { return }
        codexBusyTaskIDs.insert(taskID)
        defer { codexBusyTaskIDs.remove(taskID) }
        do {
            codexStatuses[taskID] = try await engine.startCodex(taskID: taskID)
            codexError = nil
            await refreshTasks()
            await refreshCodex(taskID: taskID)
        } catch {
            codexError = error.localizedDescription
        }
    }

    public func sendCodexMessage(taskID: UUID, input: String) async {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty,
              input.utf8.count <= 64 * 1_024,
              !codexBusyTaskIDs.contains(taskID),
              let status = codexStatuses[taskID],
              status.canSend else { return }
        codexBusyTaskIDs.insert(taskID)
        defer { codexBusyTaskIDs.remove(taskID) }
        do {
            if status.canSteer {
                _ = try await engine.steerCodexTurn(
                    taskID: taskID,
                    clientMessageID: UUID(),
                    input: input
                )
            } else {
                _ = try await engine.startCodexTurn(
                    taskID: taskID,
                    clientMessageID: UUID(),
                    input: input
                )
            }
            codexError = nil
            await refreshCodex(taskID: taskID)
        } catch {
            codexError = error.localizedDescription
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
        let presentationSorted = sortPullRequests(matching, by: presentationPreferences.pullRequestSort)
        guard !queueDecisions.isEmpty else {
            let byID = Dictionary(uniqueKeysWithValues: pulls.map { ($0.id, $0) })
            return presentationSorted.map(\.id).compactMap { byID[$0] }
        }
        let workflowOrder = Dictionary(uniqueKeysWithValues: queueDecisions.enumerated().map {
            ("\($0.element.repositoryFullName)#\($0.element.number)", $0.offset)
        })
        let workItemByID = Dictionary(uniqueKeysWithValues: pulls.map { ($0.id, $0) })
        let presentationOrder = Dictionary(uniqueKeysWithValues: presentationSorted.enumerated().map {
            ($0.element.id, $0.offset)
        })
        let sorted = matching.sorted { left, right in
            let leftItem = workItemByID[left.id]
            let rightItem = workItemByID[right.id]
            let leftKey = leftItem.map { "\($0.repositoryFullName)#\($0.number)" } ?? ""
            let rightKey = rightItem.map { "\($0.repositoryFullName)#\($0.number)" } ?? ""
            let leftPosition = workflowOrder[leftKey] ?? Int.max
            let rightPosition = workflowOrder[rightKey] ?? Int.max
            if leftPosition != rightPosition { return leftPosition < rightPosition }
            return (presentationOrder[left.id] ?? Int.max)
                < (presentationOrder[right.id] ?? Int.max)
        }
        let ids = sorted.map { $0.id }
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
        if let decision = queueDecision(for: item) {
            switch decision.tier {
            case .critical, .ready: return .ready
            case .repair, .review: return .needsWork
            case .draft: return .inbox
            case .stale: return .assessed
            case .blocked: return .blocked
            }
        }
        if item.hasConflicts == true { return .blocked }
        if item.reviewDecision == "changesRequested" || item.ciHealth == "failing" { return .needsWork }
        if item.reviewDecision == "approved" && item.ciHealth == "passing" { return .ready }
        if item.draft { return .inbox }
        return .assessed
    }

    public func queueDecision(for item: GitHubWorkItem) -> PullRequestQueueDecision? {
        queueDecisions.first {
            $0.repositoryFullName == item.repositoryFullName && $0.number == item.number
        }
    }

    public func applyWorkflowPreset(_ preset: PullRequestWorkflowPreset) async {
        do {
            selectedWorkflowPreset = preset
            queueDecisions = try await engine.call(
                method: "github.queue.assess",
                params: ["preset": preset.rawValue],
                as: [PullRequestQueueDecision].self
            )
            githubError = nil
        } catch {
            githubError = error.localizedDescription
        }
    }

    public func previewTask(from item: GitHubWorkItem) async {
        isConvertingGitHubItem = true
        defer { isConvertingGitHubItem = false }
        do {
            conversionPreview = try await engine.previewTaskFromGitHub(item)
            conversionError = nil
        } catch EngineError.remote(let code, _) where code == -32033 {
            do {
                guard var repository = repositories.first(where: { $0.fullName == item.repositoryFullName }) else {
                    throw EngineError.remote(code: -32020, message: "Refresh the repository snapshot before creating a task.")
                }
                if repository.installationID == nil {
                    let snapshot = try await engine.syncRepositoryWithGitHubApp(repository)
                    repository = snapshot.repository
                    if let index = repositories.firstIndex(where: { $0.id == repository.id }) {
                        repositories[index] = repository
                    }
                    if selectedRepository?.repository.id == repository.id {
                        selectedRepository = snapshot
                    }
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
            var task = outcome.task
            if task.state == .discovered {
                task = try await engine.planTask(taskID: task.id)
            }
            replaceTask(task)
            selectedTaskID = outcome.task.id
            conversionPreview = nil
            conversionError = nil
            await refreshTaskTimeline(taskID: task.id)
        } catch {
            conversionPreview = nil
            conversionError = error.localizedDescription
        }
    }

    private func replaceTask(_ task: EngineeringTask) {
        if let index = tasks.firstIndex(where: { $0.id == task.id }) {
            tasks[index] = task
        } else {
            tasks.append(task)
        }
    }
}
