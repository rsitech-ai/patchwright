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
                Link("Open on GitHub", destination: url)
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
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline) {
                    Text("#\(item.number)").foregroundStyle(.secondary)
                    Text(item.title).font(.title2.bold())
                    Spacer()
                    if let url = URL(string: item.htmlURL) { Link("Open", destination: url) }
                }
                HStack(spacing: 12) {
                    Label(item.kind == .pullRequest ? "Pull request" : "Issue", systemImage: item.kind == .pullRequest ? "arrow.triangle.pull" : "record.circle")
                    Text(item.state.capitalized)
                    Text("by \(item.author)").foregroundStyle(.secondary)
                    TimestampText(date: item.updatedAt)
                }
                if !item.labels.isEmpty {
                    Label(item.labels.joined(separator: ", "), systemImage: "tag").foregroundStyle(.secondary)
                }
                if let body = item.body, !body.isEmpty {
                    Text(body).textSelection(.enabled)
                }
            }
            conversionBox(item)
            discussion(for: item)
            checks(for: item)
        }
    }

    private func conversionBox(_ item: GitHubWorkItem) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("Patchwright task", systemImage: "hammer")
                .font(.headline)
            Divider()
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
                Text("Preview the typed goal, source SHAs, and acceptance criteria before creating a task.")
                    .foregroundStyle(.secondary)
                HStack {
                    Spacer(minLength: 0)
                    Button("Preview Task") { Task { await store.previewTask(from: item) } }
                }
            }
            if let error = store.conversionError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }
            if store.isConvertingGitHubItem { ProgressView().controlSize(.small) }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 10))
    }

    private func discussion(for item: GitHubWorkItem) -> some View {
        let entries = snapshot.discussions.filter { $0.itemNumber == item.number }
        return detailCard("Discussion and reviews (\(entries.count))") {
            VStack(alignment: .leading, spacing: 10) {
                if entries.isEmpty { Text("No discussion ingested").foregroundStyle(.secondary) }
                ForEach(entries) { entry in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(entry.author).bold()
                            Text(entry.state ?? entry.kind).font(.caption).foregroundStyle(.secondary)
                            Spacer()
                        }
                        Text(entry.body ?? "No written comment").textSelection(.enabled)
                    }
                    if entry.id != entries.last?.id { Divider() }
                }
            }
        }
    }

    private func checks(for item: GitHubWorkItem) -> some View {
        let checks = snapshot.checks.filter { $0.itemNumber == item.number }
        return detailCard("Checks (\(checks.count))") {
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
}
