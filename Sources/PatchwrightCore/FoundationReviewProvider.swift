import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

public enum ReviewAvailability: Equatable, Sendable {
    case available
    case unavailable(String)
}

public protocol ReviewProviding: Sendable {
    var availability: ReviewAvailability { get }
    func review(diff: String, instructions: String) async throws -> String
}

public struct FoundationReviewProvider: ReviewProviding {
    public init() {}

    public var availability: ReviewAvailability {
        #if canImport(FoundationModels)
        switch SystemLanguageModel.default.availability {
        case .available: .available
        case .unavailable(.deviceNotEligible): .unavailable("This Mac is not eligible for Apple Intelligence.")
        case .unavailable(.appleIntelligenceNotEnabled): .unavailable("Enable Apple Intelligence in System Settings.")
        case .unavailable(.modelNotReady): .unavailable("The on-device model is still preparing.")
        @unknown default: .unavailable("Foundation Models are temporarily unavailable.")
        }
        #else
        .unavailable("Foundation Models are not available in this build.")
        #endif
    }

    public func review(diff: String, instructions: String) async throws -> String {
        #if canImport(FoundationModels)
        guard availability == .available else { throw EngineError.connectionFailed("Foundation Models are unavailable.") }
        let session = LanguageModelSession(instructions: "Review code conservatively. Report only concrete failure scenarios with file evidence and a suggested test.")
        let response = try await session.respond(to: "Project instructions:\n\(instructions)\n\nDiff:\n\(diff)")
        return response.content
        #else
        throw EngineError.connectionFailed("Foundation Models are unavailable.")
        #endif
    }
}
