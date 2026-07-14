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
                        if let number = preview.action.action.issueNumber ?? preview.action.action.pullRequestNumber {
                            LabeledContent("Item", value: "#\(number)")
                        }
                        LabeledContent("Action", value: preview.fingerprint.actionKind)
                        if let branch = preview.action.action.branch ?? preview.action.action.head {
                            LabeledContent("Branch", value: branch)
                        }
                        if let deliveryHead = preview.action.action.headSha {
                            LabeledContent("Delivery commit", value: short(deliveryHead))
                        }
                        LabeledContent("Head", value: short(preview.fingerprint.headSha))
                        LabeledContent("Base", value: short(preview.fingerprint.baseSha))
                        LabeledContent("Snapshot", value: "Generation \(preview.fingerprint.invalidationGeneration)")
                        LabeledContent("Payload", value: String(preview.action.payloadSha256.prefix(12)))
                        LabeledContent("Permissions", value: preview.action.requiredPermissions.joined(separator: ", "))
                    }
                    Section(preview.action.action.kind == "mergePullRequest" ? "Merge operation" : "Remote content") {
                        Text(actionSummary(preview.action.action))
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
            .navigationTitle(preview?.action.action.kind == "mergePullRequest" ? "Approve Pull Request Merge" : "Approve GitHub Delivery")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if execution == nil {
                        if approval == nil {
                            Button(preview?.action.action.kind == "mergePullRequest" ? "Approve Merge" : "Approve Action") {
                                Task { await store.approveDelivery(taskID: task.id) }
                            }
                                .disabled(preview == nil || store.deliveryBusyTaskIDs.contains(task.id))
                        } else {
                            Button(preview?.action.action.kind == "mergePullRequest" ? "Execute Approved Merge" : "Execute Approved Action") {
                                Task { await store.executeDelivery(taskID: task.id) }
                            }
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

    private func actionSummary(_ action: GitHubActionPayload) -> String {
        if let body = action.body, !body.isEmpty { return body }
        return switch action.kind {
        case "pushIntent": "Push the exact committed worktree HEAD to the isolated task branch."
        case "closeIssue": "Close this issue as completed."
        case "closePullRequest": "Close this pull request without merging it."
        case "updatePullRequestBranch": "Request GitHub to update the pull request branch at the captured head SHA."
        case "readyPullRequest": "Mark this draft pull request ready for review only if its remote head still matches the captured SHA."
        case "checkRun": "Publish the exact Patchwright check-run status for this commit."
        case "mergePullRequest": "\((action.method ?? "merge").capitalized) the exact approved head SHA into the default branch."
        default: "Execute this exact approval-bound GitHub action."
        }
    }
}
