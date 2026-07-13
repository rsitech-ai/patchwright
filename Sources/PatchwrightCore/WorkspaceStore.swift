import Combine
import Foundation

@MainActor
public final class WorkspaceStore: ObservableObject {
    @Published public private(set) var connectionState: EngineConnectionState = .disconnected
    @Published public private(set) var tasks: [EngineeringTask] = []
    @Published public var selectedTaskID: EngineeringTask.ID?
    public let engine: any EngineServing

    public init(engine: any EngineServing) { self.engine = engine }

    public func refreshHealth() async {
        connectionState = .connecting
        do {
            let health = try await engine.call(method: "system.health", params: [:], as: HealthResponse.self)
            connectionState = .connected(health.version)
        } catch {
            connectionState = .failed(error.localizedDescription)
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
}

