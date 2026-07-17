import PatchwrightCore
import SwiftUI

struct GitHubRepositoryView: View {
    @ObservedObject var store: WorkspaceStore
    let snapshot: GitHubRepositorySnapshot
    let item: GitHubWorkItem?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                repositoryHeader
                if let item {
                    itemDetail(item)
                } else {
                    repositoryOverview
                }
            }
            .padding(24)
            .frame(maxWidth: 860, alignment: .leading)
        }
    }

    private var repositoryHeader: some View {
        HStack(spacing: 12) {
            Image(systemName: snapshot.repository.private ? "lock.square" : "shippingbox")
                .font(.title)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                Text(snapshot.repository.fullName).font(.title2.bold())
                Text(snapshot.repository.description ?? "No repository description")
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            if let url = URL(string: snapshot.repository.htmlURL) {
                Link(item == nil ? "Open on GitHub" : "Repository", destination: url)
                    .help("Open the repository on GitHub")
            }
        }
    }

    private var repositoryOverview: some View {
        Group {
            detailCard("Queue health") {
                LabeledContent("Open pull requests", value: (snapshot.repository.openPullRequestCount ?? 0).formatted())
                LabeledContent("Failing checks", value: (snapshot.repository.failingCheckCount ?? 0).formatted())
                LabeledContent("Updated") { TimestampText(date: snapshot.repository.updatedAt) }
                LabeledContent("Latest default-branch commit") {
                    TimestampText(date: snapshot.repository.defaultBranchCommittedAt)
                }
            }
            ContentUnavailableView(
                "Choose an Issue or Pull Request",
                systemImage: "cursorarrow.click.2",
                description: Text("Select an item from the queue table to inspect and convert it into a Patchwright task.")
            )
            .frame(maxWidth: .infinity, minHeight: 280)
        }
    }

    private func itemDetail(_ item: GitHubWorkItem) -> some View {
        Group {
            workItemHeader(item)
            if let body = item.body, !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                section("Description", symbol: "text.alignleft") {
                    MarkdownBodyView(source: body)
                }
            }
            conversionBox(item)
            discussion(for: item)
            checks(for: item)
        }
    }

    private func workItemHeader(_ item: GitHubWorkItem) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 7) {
                Label(item.kind == .pullRequest ? "Pull request" : "Issue", systemImage: item.kind == .pullRequest ? "arrow.triangle.pull" : "record.circle")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Text("#\(item.number)").font(.caption.monospacedDigit()).foregroundStyle(.tertiary)
                Spacer()
                if let url = URL(string: item.htmlURL) {
                    Link(destination: url) { Label("Open on GitHub", systemImage: "arrow.up.right") }
                        .help("Open \(item.repositoryFullName)#\(item.number) on GitHub")
                }
            }
            Text(item.title)
                .font(.title2.weight(.semibold))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                statusPill(item.state.capitalized, symbol: item.state == "open" ? "circle.fill" : "checkmark.circle.fill", tint: item.state == "open" ? .green : .purple)
                if item.draft { statusPill("Draft", symbol: "pencil", tint: .secondary) }
                Label(item.author, systemImage: "person.crop.circle")
                TimestampText(date: item.updatedAt)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            if item.kind == .pullRequest { pullRequestSummary(item) }
            if !item.labels.isEmpty {
                HStack(spacing: 6) {
                    ForEach(item.labels, id: \.self) { label in
                        Text(label).font(.caption2.weight(.medium)).padding(.horizontal, 7).padding(.vertical, 3)
                            .background(.quaternary, in: Capsule())
                    }
                }
            }
        }
    }

    private func pullRequestSummary(_ item: GitHubWorkItem) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let head = item.headRef, let base = item.baseRef {
                HStack(spacing: 6) {
                    Text(head).font(.caption.monospaced()).lineLimit(1).truncationMode(.middle)
                    Image(systemName: "arrow.right").foregroundStyle(.tertiary)
                    Text(base).font(.caption.monospaced()).lineLimit(1).truncationMode(.middle)
                }
                .textSelection(.enabled)
            }
            HStack(spacing: 12) {
                metric("+\(item.additions ?? 0)", tint: .green)
                metric("−\(item.deletions ?? 0)", tint: .red)
                metric("\(item.changedFiles ?? 0) files", tint: .secondary)
                Spacer()
                if let review = item.reviewDecision { Text(review.replacingOccurrences(of: "_", with: " ").capitalized) }
                if let ci = item.ciHealth { Label(ci.capitalized, systemImage: ci == "success" ? "checkmark.circle.fill" : "circle.dashed") }
            }
            .font(.caption.weight(.medium))
        }
        .padding(10)
        .background(.quaternary.opacity(0.32), in: RoundedRectangle(cornerRadius: 9))
    }

    private func conversionBox(_ item: GitHubWorkItem) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label("Patchwright task", systemImage: "hammer.fill").font(.headline)
                Spacer()
                Text("Approval gated").font(.caption.weight(.medium)).foregroundStyle(.secondary)
            }
            if let preview = store.conversionPreview,
               preview.repositoryFullName == item.repositoryFullName,
               preview.itemNumber == item.number {
                Text(preview.goal).font(.headline)
                ForEach(preview.acceptanceCriteria, id: \.self) { criterion in
                    Label(criterion, systemImage: "checkmark.circle")
                }
                Text("No capability is granted by task creation.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack {
                    Spacer(minLength: 0)
                    Button("Create Durable Task") { Task { await store.createTask(from: item) } }
                        .keyboardShortcut(.defaultAction)
                }
            } else if let task = store.assignedTask(for: item) {
                Label("Assigned to \(task.title)", systemImage: "hammer.fill")
                Text(task.state.displayName).foregroundStyle(.secondary)
            } else {
                Text("Review the typed goal, source SHAs, and acceptance criteria before creating durable work.")
                    .foregroundStyle(.secondary)
                HStack {
                    Spacer(minLength: 0)
                    Button("Preview Task") { Task { await store.previewTask(from: item) } }
                        .buttonStyle(.borderedProminent)
                        .help("Preview the local task contract")
                }
            }
            if snapshot.repository.installationID == nil {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "exclamationmark.shield.fill").foregroundStyle(.orange)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Your GitHub App is not installed for this repository").font(.callout.weight(.semibold))
                        Text("Local task preview and read-only gh data remain available. GitHub App access is required only for remote mutations and refreshes the installation identity before a mutation preview.")
                            .font(.caption).foregroundStyle(.secondary)
                        if let url = URL(string: "https://github.com/settings/installations") { Link("Manage your GitHub App installations", destination: url).font(.caption) }
                    }
                }
                .padding(10)
                .background(.orange.opacity(0.09), in: RoundedRectangle(cornerRadius: 8))
            }
            if let error = store.conversionError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            if store.isConvertingGitHubItem {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel("Creating Patchwright task")
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.tint.opacity(0.055), in: RoundedRectangle(cornerRadius: 12))
        .overlay { RoundedRectangle(cornerRadius: 12).stroke(.tint.opacity(0.16)) }
    }

    private func discussion(for item: GitHubWorkItem) -> some View {
        let entries = snapshot.discussions.filter { $0.itemNumber == item.number }
        return section("Discussion and reviews", symbol: "bubble.left.and.bubble.right", count: entries.count) {
            VStack(alignment: .leading, spacing: 10) {
                if entries.isEmpty { Text("No discussion ingested").foregroundStyle(.secondary) }
                ForEach(entries) { entry in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(entry.author).bold()
                            Text(entry.state ?? entry.kind).font(.caption).foregroundStyle(.secondary)
                            if entry.kind == "reviewThread" {
                                Label(
                                    entry.threadResolved == true ? "Resolved" : "Unresolved",
                                    systemImage: entry.threadResolved == true ? "checkmark.circle.fill" : "circle.dashed"
                                )
                                .font(.caption)
                                .foregroundStyle(entry.threadResolved == true ? .green : .orange)
                            }
                            Spacer()
                        }
                        MarkdownBodyView(source: entry.body ?? "No written comment")
                    }
                    if entry.id != entries.last?.id { Divider() }
                }
            }
        }
    }

    private func checks(for item: GitHubWorkItem) -> some View {
        let checks = snapshot.checks.filter { $0.itemNumber == item.number }
        return section("Checks", symbol: "checkmark.shield", count: checks.count) {
            VStack(alignment: .leading, spacing: 8) {
                if checks.isEmpty { Text("No checks ingested").foregroundStyle(.secondary) }
                ForEach(checks) { check in
                    Label(
                        "\(check.name): \(check.conclusion ?? check.status)",
                        systemImage: check.conclusion == "success" ? "checkmark.circle.fill" : "circle.dashed"
                    )
                }
            }
        }
    }

    private func detailCard<Content: View>(
        _ title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.headline)
            Divider()
            content()
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 10))
    }

    private func section<Content: View>(_ title: String, symbol: String, count: Int? = nil, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(title, systemImage: symbol).font(.headline)
                Spacer()
                if let count { Text(count.formatted()).font(.caption.monospacedDigit()).foregroundStyle(.secondary) }
            }
            Divider()
            content()
        }
        .padding(.vertical, 4)
    }

    private func statusPill(_ title: String, symbol: String, tint: Color) -> some View {
        Label(title, systemImage: symbol).font(.caption.weight(.semibold)).foregroundStyle(tint)
            .padding(.horizontal, 8).padding(.vertical, 4).background(tint.opacity(0.1), in: Capsule())
    }

    private func metric(_ title: String, tint: Color) -> some View { Text(title).foregroundStyle(tint) }
}

private struct MarkdownBodyView: View {
    let source: String

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .heading(let text): Text(.init(text)).font(.headline).padding(.top, 4)
                case .bullet(let text): HStack(alignment: .firstTextBaseline, spacing: 8) { Text("•").foregroundStyle(.secondary); Text(.init(text)).fixedSize(horizontal: false, vertical: true) }
                case .paragraph(let text): Text(.init(text)).fixedSize(horizontal: false, vertical: true)
                case .code(let text): Text(text).font(.caption.monospaced()).textSelection(.enabled).padding(10).frame(maxWidth: .infinity, alignment: .leading).background(.quaternary.opacity(0.55), in: RoundedRectangle(cornerRadius: 8))
                }
            }
        }
        .textSelection(.enabled)
    }

    private var blocks: [Block] {
        var result: [Block] = []
        var code: [String] = []
        var inCode = false
        for rawLine in sanitizedSource.split(separator: "\n", omittingEmptySubsequences: false).map(String.init) {
            if rawLine.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                if inCode { result.append(.code(code.joined(separator: "\n"))); code = [] }
                inCode.toggle(); continue
            }
            if inCode { code.append(rawLine); continue }
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.isEmpty { continue }
            if line.hasPrefix("### ") { result.append(.heading(String(line.dropFirst(4)))) }
            else if line.hasPrefix("## ") { result.append(.heading(String(line.dropFirst(3)))) }
            else if line.hasPrefix("# ") { result.append(.heading(String(line.dropFirst(2)))) }
            else if line.hasPrefix("- ") { result.append(.bullet(String(line.dropFirst(2)))) }
            else { result.append(.paragraph(line)) }
        }
        if !code.isEmpty { result.append(.code(code.joined(separator: "\n"))) }
        return result
    }

    private var sanitizedSource: String {
        source
            .replacingOccurrences(of: "(?i)<summary[^>]*>", with: "\n### ", options: .regularExpression)
            .replacingOccurrences(of: "(?i)</summary>", with: "\n", options: .regularExpression)
            .replacingOccurrences(of: "(?i)<li[^>]*>", with: "\n- ", options: .regularExpression)
            .replacingOccurrences(of: "(?i)</li>", with: "\n", options: .regularExpression)
            .replacingOccurrences(of: #"(?i)<br\s*/?>"#, with: "\n", options: .regularExpression)
            .replacingOccurrences(of: "(?i)</?(details|ul|ol)[^>]*>", with: "\n", options: .regularExpression)
            .replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
    }

    private enum Block { case heading(String), bullet(String), paragraph(String), code(String) }
}
