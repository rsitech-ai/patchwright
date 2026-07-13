import Combine
import Foundation

@MainActor
public final class WorkspaceStore: ObservableObject {
    @Published public private(set) var connectionState: EngineConnectionState = .disconnected
    @Published public private(set) var tasks: [EngineeringTask] = []
    @Published public var selectedTaskID: EngineeringTask.ID?
    @Published public private(set) var githubStatus: GitHubStatus?
    @Published public private(set) var repositories: [GitHubRepository] = []
    @Published public private(set) var selectedRepository: GitHubRepositorySnapshot?
    @Published public private(set) var githubSyncSummary: GitHubSyncSummary?
    @Published public private(set) var isSyncingGitHub = false
    @Published public private(set) var conversionPreview: ConversionPreview?
    @Published public private(set) var isConvertingGitHubItem = false
    @Published public private(set) var conversionError: String?
    @Published public var githubError: String?
    public let engine: any EngineServing
    private let healthRetryAttempts: Int
    private let healthRetryDelay: Duration

    public init(
        engine: any EngineServing,
        healthRetryAttempts: Int = 20,
        healthRetryDelay: Duration = .milliseconds(100)
    ) {
        self.engine = engine
        self.healthRetryAttempts = max(1, healthRetryAttempts)
        self.healthRetryDelay = healthRetryDelay
    }

    public func refreshHealth() async {
        connectionState = .connecting
        var lastError: Error?
        for attempt in 0..<healthRetryAttempts {
            do {
                let health = try await engine.call(method: "system.health", params: [:], as: HealthResponse.self)
                connectionState = .connected(health.version)
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
            githubError = nil
        } catch {
            githubError = error.localizedDescription
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
            selectedTaskID = nil
            githubError = nil
        } catch {
            githubError = error.localizedDescription
        }
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

    public func previewTask(from item: GitHubWorkItem) async {
        isConvertingGitHubItem = true
        defer { isConvertingGitHubItem = false }
        do {
            conversionPreview = try await engine.previewTaskFromGitHub(item)
            conversionError = nil
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
