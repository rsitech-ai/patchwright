import PatchwrightCore
import SwiftUI

struct DeliveryApprovalSheet: View {
    @ObservedObject var store: WorkspaceStore
    let task: EngineeringTask
    @Environment(\.dismiss) private var dismiss

    private var preview: DeliveryPreview? { store.deliveryPreviews[task.id] }
    private var approval: DeliveryApproval? { store.deliveryApprovals[task.id] }
    private var execution: DeliveryExecution? { store.deliveryExecutions[task.id] }

    var body: some View {
        NavigationStack {
            Form {
                if let preview {
                    Section("Exact GitHub write") {
                        LabeledContent("Target", value: preview.action.remote.repositoryFullName)
                        LabeledContent("Item", value: "#\(preview.action.action.issueNumber ?? 0)")
                        LabeledContent("Action", value: preview.fingerprint.actionKind)
                        LabeledContent("Head", value: short(preview.fingerprint.headSha))
                        LabeledContent("Base", value: short(preview.fingerprint.baseSha))
                        LabeledContent("Snapshot", value: "Generation \(preview.fingerprint.invalidationGeneration)")
                        LabeledContent("Payload", value: String(preview.action.payloadSha256.prefix(12)))
                        LabeledContent("Permissions", value: preview.action.requiredPermissions.joined(separator: ", "))
                    }
                    Section("Remote content") {
                        Text(preview.action.action.body ?? "No body")
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                if let approval {
                    Section("Approval") {
                        LabeledContent("Approved by", value: approval.approvedBy)
                        LabeledContent("Expires") { TimestampText(date: approval.expiresAt) }
                        Text("Approval is valid only for this exact target, content, source SHAs, policy, instructions, and snapshot generation.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                if let execution {
                    Section("Delivery result") {
                        Label("GitHub write completed", systemImage: "checkmark.seal.fill")
                            .foregroundStyle(.green)
                        LabeledContent("Idempotency", value: String(execution.idempotencyKey.prefix(12)))
                        if let url = execution.result.htmlUrl, let destination = URL(string: url) {
                            Link("Open result on GitHub", destination: destination)
                        }
                    }
                }
                if let error = store.deliveryError {
                    Section("Blocked") {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Approve GitHub Delivery")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if execution == nil {
                        if approval == nil {
                            Button("Approve Comment") { Task { await store.approveDelivery(taskID: task.id) } }
                                .disabled(preview == nil || store.deliveryBusyTaskIDs.contains(task.id))
                        } else {
                            Button("Execute Approved Comment") { Task { await store.executeDelivery(taskID: task.id) } }
                                .disabled(store.deliveryBusyTaskIDs.contains(task.id))
                        }
                    }
                }
            }
        }
        .frame(minWidth: 620, minHeight: 560)
    }

    private func short(_ value: String?) -> String {
        value.map { String($0.prefix(12)) } ?? "Not bound"
    }
}
