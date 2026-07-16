import PatchwrightCore
import SwiftUI

struct CodexApprovalSheet: View {
    let approval: CodexRuntimeApproval
    let resolve: (Bool) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Label(approval.kind == .command ? "Approve command once" : "Approve file change once", systemImage: approval.kind == .command ? "terminal" : "doc.badge.gearshape")
                .font(.title2.bold())
            Text("This permission applies only to this Codex request and active turn. It grants no GitHub, network, workflow, or merge authority.")
                .foregroundStyle(.secondary)
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 10) {
                detail("Target", approval.command ?? approval.grantRoot ?? "Task worktree")
                if let cwd = approval.cwd { detail("Working directory", cwd) }
                detail("Reason", approval.reason ?? "No reason supplied")
                detail("Expires", approval.expiresAt.formatted(date: .omitted, time: .standard))
                detail("Turn", approval.turnId)
                detail("Process", approval.processGeneration.uuidString)
            }
            .font(.callout)
            HStack {
                Button("Decline", role: .cancel) { resolve(false); dismiss() }
                Spacer()
                Button("Approve Once") { resolve(true); dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 560)
    }

    private func detail(_ label: String, _ value: String) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary)
            Text(value).textSelection(.enabled).lineLimit(4).truncationMode(.middle)
        }
    }
}
