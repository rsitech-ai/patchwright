import PatchwrightCore
import SwiftUI

struct CodexThreadView: View {
    @ObservedObject var store: WorkspaceStore
    let task: EngineeringTask
    @State private var draft = ""
    @State private var selectedApproval: CodexRuntimeApproval?
    @State private var preparationApprovalRequest: PreparationPreview?

    private var status: CodexRuntimeStatus? { store.codexStatus(for: task.id) }
    private var transcript: CodexTranscript { store.codexTranscript(for: task.id) }
    private var isBusy: Bool { store.codexBusyTaskIDs.contains(task.id) }

    var body: some View {
        VStack(spacing: 0) {
            runtimeBar
            Divider()
            transcriptContent
            Divider()
            composer
        }
        .task(id: task.id) {
            while !Task.isCancelled {
                await store.refreshCodex(taskID: task.id)
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .sheet(item: $selectedApproval) { approval in
            CodexApprovalSheet(approval: approval) { approve in
                Task { await store.resolveCodexApproval(approval, approve: approve) }
            }
        }
        .sheet(item: $preparationApprovalRequest) { preview in
            PreparationApprovalSheet(store: store, preview: preview)
        }
    }

    @ViewBuilder private var runtimeBar: some View {
        HStack(spacing: 10) {
            Label(statusLabel, systemImage: statusSymbol)
                .font(.callout.weight(.medium))
            if let accountState = status?.accountState {
                Text(accountState == .signedIn ? "Signed in" : accountState == .signedOut ? "Signed out" : "Account unavailable")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if isBusy {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel("Codex is working")
            }
            if let approval = store.codexApprovalsByTask[task.id]?.first(where: { $0.state == .pending }) {
                Button("Review Request", systemImage: "checkmark.shield") { selectedApproval = approval }
                    .buttonStyle(.borderedProminent)
            }
            if status?.state == .ready {
                Button("Pause", systemImage: "pause.fill") { Task { await store.interruptCodex(taskID: task.id, cancel: false) } }
                    .disabled(isBusy)
                    .help("Interrupt Codex and retain the task worktree and evidence for resume")
                Button("Cancel", systemImage: "xmark", role: .destructive) { Task { await store.interruptCodex(taskID: task.id, cancel: true) } }
                    .disabled(isBusy)
                    .help("Cancel this task, stop its Codex process group, and retain the worktree and evidence")
            }
            if status?.canStart == true || status == nil {
                if task.state == .awaitingPreparationApproval {
                    Button("Review Preparation", systemImage: "checkmark.shield.fill") {
                        Task {
                            await store.previewPreparation(taskID: task.id)
                            preparationApprovalRequest = store.preparationPreviews[task.id]
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(isBusy || store.taskLifecycleBusyTaskIDs.contains(task.id))
                } else {
                    Button("Start Codex", systemImage: "play.fill") {
                        Task { await store.startCodex(taskID: task.id) }
                    }
                    .disabled(isBusy || ![.preparing, .paused].contains(task.state))
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.bar)
    }

    @ViewBuilder private var transcriptContent: some View {
        if status?.state == .unavailable {
            ContentUnavailableView(
                "Codex Unavailable",
                systemImage: "exclamationmark.triangle",
                description: Text("Install or configure the pinned Codex CLI, then restart Patchwright.")
            )
            .frame(maxHeight: .infinity)
        } else if status?.state == .staleThreadNeedsConfirmation {
            ContentUnavailableView(
                "Saved Thread Needs Confirmation",
                systemImage: "arrow.trianglehead.2.clockwise.rotate.90",
                description: Text("The saved Codex thread could not be resumed. Patchwright did not silently replace it.")
            )
            .frame(maxHeight: .infinity)
        } else if transcript.items.isEmpty {
            ContentUnavailableView(
                status?.state == .ready ? "Ready for a Message" : "Codex Not Started",
                systemImage: "bubble.left.and.text.bubble.right",
                description: Text(status?.state == .ready ? "Send the approved task instructions or steer an active turn." : "Start Codex after the task worktree is prepared.")
            )
            .frame(maxHeight: .infinity)
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(transcript.items) { item in
                            transcriptCard(item)
                                .id(item.id)
                        }
                    }
                    .padding(16)
                }
                .onChange(of: transcript.cursor) { _, _ in
                    if let last = transcript.items.last { proxy.scrollTo(last.id, anchor: .bottom) }
                }
            }
        }
    }

    private func transcriptCard(_ item: CodexTranscriptItem) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Label(cardTitle(item.kind), systemImage: cardSymbol(item.kind))
                .font(.caption.weight(.semibold))
                .foregroundStyle(cardTint(item.kind))
            Text(item.content)
                .textSelection(.enabled)
                .font(cardFont(item.kind))
                .frame(maxWidth: .infinity, alignment: .leading)
            if let turnID = item.turnID {
                Text("Turn \(turnID)")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.tertiary)
                    .textSelection(.enabled)
            }
        }
        .padding(12)
        .background(cardBackground(item.kind), in: RoundedRectangle(cornerRadius: 11))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var composer: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let error = store.codexError {
                Label(error, systemImage: "exclamationmark.circle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }
            HStack(alignment: .bottom, spacing: 10) {
                TextField("Message Codex…", text: $draft, axis: .vertical)
                    .lineLimit(1...8)
                    .textFieldStyle(.roundedBorder)
                    .disabled(status?.canSend != true || isBusy)
                Button(status?.canSteer == true ? "Steer" : "Send", systemImage: "arrow.up.circle.fill") {
                    let message = draft
                    draft = ""
                    Task { await store.sendCodexMessage(taskID: task.id, input: message) }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(
                    status?.canSend != true || isBusy
                        || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        || draft.utf8.count > 64 * 1_024
                )
            }
            Text(status?.canSteer == true ? "Steering appends to the active turn." : "Messages start a new supervised turn.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .background(.bar)
    }

    private var statusLabel: String {
        guard let state = status?.state else { return "Loading Codex" }
        return switch state {
        case .unavailable: "Unavailable"
        case .notStarted: "Not started"
        case .ready: status?.canSteer == true ? "Turn running" : "Ready"
        case .staleThreadNeedsConfirmation: "Recovery required"
        case .failed: "Failed"
        case .exited: "Stopped"
        }
    }

    private var statusSymbol: String {
        switch status?.state {
        case .ready: status?.canSteer == true ? "waveform" : "checkmark.circle.fill"
        case .failed, .staleThreadNeedsConfirmation: "exclamationmark.triangle.fill"
        case .unavailable: "slash.circle"
        case .exited: "stop.circle"
        case .notStarted, .none: "circle.dotted"
        }
    }

    private func cardTitle(_ kind: CodexTranscriptItemKind) -> String {
        switch kind {
        case .operatorMessage: "You"
        case .agentMessage: "Codex"
        case .reasoning: "Reasoning"
        case .command: "Command"
        case .fileChange: "File change"
        case .status: "Status"
        case .unknown(let value): value
        }
    }

    private func cardSymbol(_ kind: CodexTranscriptItemKind) -> String {
        switch kind {
        case .operatorMessage: "person.crop.circle"
        case .agentMessage: "sparkles"
        case .reasoning: "brain.head.profile"
        case .command: "terminal"
        case .fileChange: "doc.badge.gearshape"
        case .status: "info.circle"
        case .unknown: "questionmark.diamond"
        }
    }

    private func cardTint(_ kind: CodexTranscriptItemKind) -> Color {
        switch kind {
        case .operatorMessage: .blue
        case .agentMessage: .primary
        case .reasoning: .purple
        case .command: .orange
        case .fileChange: .green
        case .status, .unknown: .secondary
        }
    }

    private func cardFont(_ kind: CodexTranscriptItemKind) -> Font {
        switch kind {
        case .command, .fileChange: .callout.monospaced()
        default: .body
        }
    }

    private func cardBackground(_ kind: CodexTranscriptItemKind) -> Color {
        switch kind {
        case .operatorMessage: .blue.opacity(0.10)
        case .command: .orange.opacity(0.09)
        case .fileChange: .green.opacity(0.09)
        default: .secondary.opacity(0.08)
        }
    }
}
