import Foundation

public struct ContractInstructionDigest: Codable, Equatable, Sendable {
    public let source: String
    public let sha256: String
    public let precedence: UInt16
}

public struct ContractVerificationCommand: Codable, Equatable, Sendable {
    public let program: String
    public let args: [String]

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        program = try container.decode(String.self, forKey: .program)
        args = try container.decode([String].self, forKey: .args)
        guard !program.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !program.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains),
              !args.contains(where: {
                  $0.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
              }) else {
            throw DecodingError.dataCorruptedError(
                forKey: .program,
                in: container,
                debugDescription: "Verification commands must contain a validated executable and argv."
            )
        }
    }

    public var argvDisplay: String {
        let data = try? JSONEncoder().encode([program] + args)
        return data.flatMap { String(data: $0, encoding: .utf8) } ?? "[]"
    }
}

public enum TaskRisk: String, Codable, Sendable {
    case low, moderate, high, critical

    public var displayName: String { rawValue.capitalized }
}

public struct ContractSensitivePath: Codable, Equatable, Sendable {
    public let path: String
    public let reason: String
}

public struct TaskContract: Codable, Equatable, Sendable {
    public let version: UInt32
    public let taskId: UUID
    public let source: TaskSource
    public let repositoryBindingId: UUID
    public let goal: String
    public let acceptanceCriteria: [String]
    public let baseSha: String?
    public let headSha: String?
    public let sourceSha256: String
    public let repositorySha256: String
    public let instructionDigests: [ContractInstructionDigest]
    public let verificationCommands: [ContractVerificationCommand]
    public let requiredCapabilities: [String]
    public let risk: TaskRisk
    public let sensitivePaths: [ContractSensitivePath]
    public let dependencies: [UUID]

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        version = try container.decode(UInt32.self, forKey: .version)
        taskId = try container.decode(UUID.self, forKey: .taskId)
        source = try container.decode(TaskSource.self, forKey: .source)
        repositoryBindingId = try container.decode(UUID.self, forKey: .repositoryBindingId)
        goal = try container.decode(String.self, forKey: .goal)
        acceptanceCriteria = try container.decode([String].self, forKey: .acceptanceCriteria)
        baseSha = try container.decodeIfPresent(String.self, forKey: .baseSha)
        headSha = try container.decodeIfPresent(String.self, forKey: .headSha)
        sourceSha256 = try container.decode(String.self, forKey: .sourceSha256)
        repositorySha256 = try container.decode(String.self, forKey: .repositorySha256)
        instructionDigests = try container.decode(
            [ContractInstructionDigest].self,
            forKey: .instructionDigests
        )
        verificationCommands = try container.decode(
            [ContractVerificationCommand].self,
            forKey: .verificationCommands
        )
        requiredCapabilities = try container.decode([String].self, forKey: .requiredCapabilities)
        risk = try container.decode(TaskRisk.self, forKey: .risk)
        sensitivePaths = try container.decode([ContractSensitivePath].self, forKey: .sensitivePaths)
        dependencies = try container.decode([UUID].self, forKey: .dependencies)
        guard version == 1 || version == 2,
              !goal.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !acceptanceCriteria.isEmpty,
              !acceptanceCriteria.contains(where: {
                  $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
              }),
              !verificationCommands.isEmpty,
              Self.isSHA256(sourceSha256),
              Self.isSHA256(repositorySha256) else {
            throw DecodingError.dataCorruptedError(
                forKey: .verificationCommands,
                in: container,
                debugDescription: "The engine returned an invalid task contract."
            )
        }
    }

    private static func isSHA256(_ value: String) -> Bool {
        value.count == 64 && value.unicodeScalars.allSatisfy {
            CharacterSet(charactersIn: "0123456789abcdefABCDEF").contains($0)
        }
    }
}

public struct TaskContractSnapshot: Decodable, Equatable, Sendable {
    public let version: UInt32
    public let taskId: UUID
    public let source: TaskSource
    public let repositoryBindingId: UUID
    public let goal: String
    public let acceptanceCriteria: [String]
    public let baseSha: String?
    public let headSha: String?
    public let sourceSha256: String?
    public let repositorySha256: String?
    public let instructionDigests: [ContractInstructionDigest]
    public let verificationCommands: [ContractVerificationCommand]
    public let requiredCapabilities: [String]
    public let risk: TaskRisk
    public let sensitivePaths: [ContractSensitivePath]
    public let dependencies: [UUID]
    public let isLegacyReadOnly: Bool

    private enum CodingKeys: String, CodingKey {
        case version, taskId, source, repositoryBindingId, goal, acceptanceCriteria
        case baseSha, headSha, sourceSha256, repositorySha256, instructionDigests
        case verificationCommands, requiredCapabilities, risk, sensitivePaths, dependencies
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        version = try container.decode(UInt32.self, forKey: .version)
        taskId = try container.decode(UUID.self, forKey: .taskId)
        source = try container.decode(TaskSource.self, forKey: .source)
        repositoryBindingId = try container.decode(UUID.self, forKey: .repositoryBindingId)
        goal = try container.decode(String.self, forKey: .goal)
        acceptanceCriteria = try container.decode([String].self, forKey: .acceptanceCriteria)
        baseSha = try container.decodeIfPresent(String.self, forKey: .baseSha)
        headSha = try container.decodeIfPresent(String.self, forKey: .headSha)
        sourceSha256 = try container.decodeIfPresent(String.self, forKey: .sourceSha256)
        repositorySha256 = try container.decodeIfPresent(String.self, forKey: .repositorySha256)
        instructionDigests = try container.decode(
            [ContractInstructionDigest].self,
            forKey: .instructionDigests
        )
        verificationCommands = try container.decode(
            [ContractVerificationCommand].self,
            forKey: .verificationCommands
        )
        requiredCapabilities = try container.decode([String].self, forKey: .requiredCapabilities)
        risk = try container.decode(TaskRisk.self, forKey: .risk)
        sensitivePaths = try container.decode([ContractSensitivePath].self, forKey: .sensitivePaths)
        dependencies = try container.decode([UUID].self, forKey: .dependencies)

        let hasMalformedIntegrityEvidence = sourceSha256.map { !Self.isSHA256($0) } == true
            || repositorySha256.map { !Self.isSHA256($0) } == true
        let hasIntegrityPair = sourceSha256 != nil && repositorySha256 != nil
        let hasPartialIntegrityEvidence = (sourceSha256 != nil || repositorySha256 != nil)
            && !hasIntegrityPair
        let hasExecutableEvidence = sourceSha256.map(Self.isSHA256) == true
            && repositorySha256.map(Self.isSHA256) == true
            && !verificationCommands.isEmpty
        isLegacyReadOnly = version == 1 && !hasExecutableEvidence
        guard version == 1 || version == 2,
              !goal.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !acceptanceCriteria.isEmpty,
              !acceptanceCriteria.contains(where: {
                  $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
              }),
              !hasMalformedIntegrityEvidence,
              !hasPartialIntegrityEvidence,
              isLegacyReadOnly || hasExecutableEvidence else {
            throw DecodingError.dataCorruptedError(
                forKey: .verificationCommands,
                in: container,
                debugDescription: "The engine returned an invalid task contract snapshot."
            )
        }
    }

    private static func isSHA256(_ value: String) -> Bool {
        value.count == 64 && value.unicodeScalars.allSatisfy {
            CharacterSet(charactersIn: "0123456789abcdefABCDEF").contains($0)
        }
    }
}
