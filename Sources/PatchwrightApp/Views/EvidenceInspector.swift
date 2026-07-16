import PatchwrightCore
import SwiftUI

struct EvidenceInspector: View {
    let task: EngineeringTask?
    let codexStatus: CodexRuntimeStatus?
    var body: some View {
        Form {
            Section("Effective Instructions") { Text("Instruction sources and conflicts appear after repository inspection.").foregroundStyle(.secondary) }
            Section("Approvals") { Text(task?.requiresAttention == true ? "Action required" : "No approval pending") }
            Section("Evidence") { Text("Command output is stored locally and linked by content hash.").foregroundStyle(.secondary) }
            if let codexStatus {
                Section("Codex Runtime") {
                    LabeledContent("State", value: codexStatus.state.rawValue)
                    if let generation = codexStatus.processGeneration {
                        LabeledContent("Generation", value: generation.uuidString)
                            .textSelection(.enabled)
                    }
                    if let threadID = codexStatus.threadID {
                        LabeledContent("Thread", value: threadID)
                            .textSelection(.enabled)
                    }
                    if let turnID = codexStatus.turnID {
                        LabeledContent("Turn", value: turnID)
                            .textSelection(.enabled)
                    }
                    LabeledContent("Event cursor", value: String(codexStatus.lastSequence))
                }
            }
        }
        .formStyle(.grouped)
    }
}
