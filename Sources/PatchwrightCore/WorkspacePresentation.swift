import Foundation

public enum WorkspaceSection: String, Codable, CaseIterable, Identifiable, Sendable {
    case queue
    case issues
    case repositories
    case activeTasks
    case awaitingApproval
    case monitoring
    case completed

    public var id: String { rawValue }
}

public enum WorkspaceContentState: Equatable, Sendable {
    case loading
    case empty
    case ready
    case partial(String)
    case blocked(String)

    public static func resolve(hasContent: Bool, loading: Bool, error: String?) -> Self {
        if loading { return .loading }
        if let error, hasContent { return .partial(error) }
        if let error { return .blocked(error) }
        return hasContent ? .ready : .empty
    }
}

public enum PullRequestTableDensity: Equatable, Sendable {
    case compact
    case expanded

    public static func resolve(availableWidth: Double) -> Self {
        availableWidth >= 1_050 ? .expanded : .compact
    }
}

public enum TaskSurfaceState: Equatable, Sendable {
    case ready
    case completed
    case cancelled
    case blocked(String)

    public static func resolve(state: TaskState, reason: String?) -> Self {
        switch state {
        case .completed:
            .completed
        case .cancelled:
            .cancelled
        case .blocked:
            .blocked(reason ?? "This task is blocked.")
        default:
            .ready
        }
    }
}

public struct TimestampPresentation: Equatable, Sendable {
    public let relative: String
    public let exact: String

    public init(
        date: Date,
        now: Date = Date(),
        locale: Locale = .current,
        timeZone: TimeZone = .current
    ) {
        let relativeFormatter = RelativeDateTimeFormatter()
        relativeFormatter.locale = locale
        relativeFormatter.unitsStyle = .full
        relative = relativeFormatter.localizedString(for: date, relativeTo: now)

        let exactFormatter = DateFormatter()
        exactFormatter.locale = locale
        exactFormatter.timeZone = timeZone
        exactFormatter.dateStyle = .medium
        exactFormatter.timeStyle = .medium
        exact = exactFormatter.string(from: date)
    }
}

@MainActor
public protocol WorkspacePreferencesPersisting: AnyObject {
    func load(workspaceID: String) -> WorkspacePresentationPreferences?
    func save(_ preferences: WorkspacePresentationPreferences, workspaceID: String)
}

@MainActor
public final class UserDefaultsWorkspacePreferences: WorkspacePreferencesPersisting {
    private let defaults: UserDefaults
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public func load(workspaceID: String) -> WorkspacePresentationPreferences? {
        guard let data = defaults.data(forKey: key(workspaceID)) else { return nil }
        return try? decoder.decode(WorkspacePresentationPreferences.self, from: data)
    }

    public func save(_ preferences: WorkspacePresentationPreferences, workspaceID: String) {
        guard let data = try? encoder.encode(preferences) else { return }
        defaults.set(data, forKey: key(workspaceID))
    }

    private func key(_ workspaceID: String) -> String {
        "ai.patchwright.workspace.\(workspaceID).presentation"
    }
}
