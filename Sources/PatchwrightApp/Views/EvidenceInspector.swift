import PatchwrightCore
import SwiftUI

struct EvidenceInspector: View {
    let task: EngineeringTask?
    var body: some View {
        Form {
            Section("Effective Instructions") { Text("Instruction sources and conflicts appear after repository inspection.").foregroundStyle(.secondary) }
            Section("Approvals") { Text(task?.requiresAttention == true ? "Action required" : "No approval pending") }
            Section("Evidence") { Text("Command output is stored locally and linked by content hash.").foregroundStyle(.secondary) }
        }
        .formStyle(.grouped)
    }
}

