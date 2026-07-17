import PatchwrightCore
import SwiftUI

struct PreparationApprovalSheet: View {
    @ObservedObject var store: WorkspaceStore
    let preview: PreparationPreview
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Implementation contract") {
                    LabeledContent("Goal", value: preview.contract.goal)
                    LabeledContent("Risk", value: preview.contract.risk.displayName)
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Acceptance criteria").font(.headline)
                        ForEach(
                            Array(preview.contract.acceptanceCriteria.enumerated()),
                            id: \.offset
                        ) { _, criterion in
                            Label(criterion, systemImage: "checkmark.circle")
                        }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Exact verification commands").font(.headline)
                        ForEach(
                            Array(preview.contract.verificationCommands.enumerated()),
                            id: \.offset
                        ) { _, command in
                            Text(command.argvDisplay)
                                .font(.body.monospaced())
                                .textSelection(.enabled)
                        }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Sensitive paths").font(.headline)
                        if preview.contract.sensitivePaths.isEmpty {
                            Text("None declared").foregroundStyle(.secondary)
                        } else {
                            ForEach(
                                Array(preview.contract.sensitivePaths.enumerated()),
                                id: \.offset
                            ) { _, path in
                                LabeledContent(path.path, value: path.reason)
                            }
                        }
                    }
                }
                Section("Exact local preparation") {
                    LabeledContent("Repository", value: preview.repositoryFullName)
                    LabeledContent("Repository path", value: preview.repositoryPath)
                    LabeledContent("Source commit", value: short(preview.sourceSha))
                    LabeledContent("Worktree", value: preview.worktreePath)
                    LabeledContent("Branch", value: preview.branch)
                    LabeledContent("Generation", value: String(preview.invalidationGeneration))
                }
                Section("Approval boundary") {
                    LabeledContent("Policy", value: short(preview.policySha256))
                    LabeledContent("Instructions", value: short(preview.instructionSha256))
                    LabeledContent("Action", value: preview.fingerprint.actionKind)
                    Text("Approval is short-lived, single-use, and valid only for these exact paths, source commit, policy, instructions, and task generation.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let error = store.taskLifecycleError {
                    Section("Blocked") {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Approve Worktree Preparation")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Approve & Prepare") {
                        Task {
                            await store.approveAndPrepare(preview)
                            if store.preparationPreviews[preview.taskId] == nil {
                                dismiss()
                            }
                        }
                    }
                    .disabled(store.taskLifecycleBusyTaskIDs.contains(preview.taskId))
                }
            }
        }
        .frame(minWidth: 700, minHeight: 620)
    }

    private func short(_ value: String) -> String {
        String(value.prefix(12))
    }
}
