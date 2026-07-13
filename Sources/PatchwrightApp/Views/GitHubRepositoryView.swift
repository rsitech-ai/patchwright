import PatchwrightCore
import SwiftUI

struct GitHubRepositoryView: View {
    let snapshot: GitHubRepositorySnapshot
    @State private var selectedItemID: GitHubWorkItem.ID?
    @State private var search = ""

    private var items: [GitHubWorkItem] {
        search.isEmpty ? snapshot.workItems : snapshot.workItems.filter {
            $0.title.localizedCaseInsensitiveContains(search) || String($0.number).contains(search)
        }
    }

    private var selectedItem: GitHubWorkItem? {
        snapshot.workItems.first { $0.id == selectedItemID }
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    HStack {
                        Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                        TextField("Search issues and pull requests", text: $search)
                            .textFieldStyle(.plain)
                            .accessibilityLabel("Search issues and pull requests")
                    }
                    .padding(10)
                    Divider()
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(items) { item in
                                Button {
                                    selectedItemID = item.id
                                } label: {
                                    GitHubWorkItemRow(item: item)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .padding(.horizontal, 12)
                                        .padding(.vertical, 5)
                                        .background(selectedItemID == item.id ? Color.accentColor.opacity(0.14) : .clear)
                                }
                                .buttonStyle(.plain)
                                .contentShape(Rectangle())
                                .accessibilityLabel("\(item.kind == .pullRequest ? "Pull request" : "Issue") #\(item.number): \(item.title)")
                            }
                        }
                    }
                }
                .frame(width: 340)
                Divider()
                workItemDetail
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    private var header: some View {
        HStack(spacing: 14) {
            Image(systemName: snapshot.repository.private ? "lock.square" : "shippingbox").font(.title)
            VStack(alignment: .leading) {
                Text(snapshot.repository.fullName).font(.title2.bold())
                Text(snapshot.repository.description ?? "No repository description")
                    .foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            Text("\(snapshot.workItems.count) items · \(snapshot.discussions.count) discussion · \(snapshot.checks.count) checks · \(snapshot.workflowRuns.count) runs")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            if let repositoryURL = URL(string: snapshot.repository.htmlURL) {
                Link(destination: repositoryURL) {
                    Image(systemName: "arrow.up.right.square")
                }
                .help("Open repository on GitHub")
            }
        }
        .padding(20)
    }

    @ViewBuilder private var workItemDetail: some View {
        if let item = selectedItem {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    HStack {
                        Text("#\(item.number)").foregroundStyle(.secondary)
                        Text(item.title).font(.title2.bold())
                        Spacer()
                        if let itemURL = URL(string: item.htmlURL) {
                            Link("Open on GitHub", destination: itemURL)
                        }
                    }
                    HStack {
                        Label(item.kind == .pullRequest ? "Pull request" : "Issue", systemImage: item.kind == .pullRequest ? "arrow.triangle.pull" : "record.circle")
                        Text(item.state.capitalized)
                        Text("by \(item.author)").foregroundStyle(.secondary)
                    }
                    if !item.labels.isEmpty {
                        Label(item.labels.joined(separator: ", "), systemImage: "tag")
                            .foregroundStyle(.secondary)
                    }
                    if !item.assignees.isEmpty {
                        Label(item.assignees.map { "@\($0)" }.joined(separator: ", "), systemImage: "person.2")
                            .foregroundStyle(.secondary)
                    }
                    if let milestone = item.milestone {
                        Label(milestone, systemImage: "signpost.right")
                            .foregroundStyle(.secondary)
                    }
                    if let body = item.body, !body.isEmpty { Text(body).textSelection(.enabled) }
                    discussion(for: item)
                    checks(for: item)
                }
                .padding(24)
                .frame(maxWidth: 760, alignment: .leading)
            }
        } else {
            VStack(spacing: 10) {
                Image(systemName: "point.topleft.down.to.point.bottomright.curvepath")
                    .font(.largeTitle)
                    .foregroundStyle(.secondary)
                Text("Choose an issue or pull request").font(.headline)
                Text("Select an item from the repository snapshot to inspect its body, discussion, reviews, and checks.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 360)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(24)
        }
    }

    private func discussion(for item: GitHubWorkItem) -> some View {
        let entries = snapshot.discussions.filter { $0.itemNumber == item.number }
        return VStack(alignment: .leading, spacing: 8) {
            Divider()
            Text("Discussion and reviews (\(entries.count))").font(.headline)
            if entries.isEmpty { Text("No discussion ingested").foregroundStyle(.secondary) }
            ForEach(entries) { entry in
                VStack(alignment: .leading, spacing: 5) {
                    HStack {
                        Text(entry.author).bold()
                        Text(entry.state ?? entry.kind).font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        if let entryURL = URL(string: entry.htmlURL) {
                            Link(destination: entryURL) { Image(systemName: "arrow.up.right") }
                                .help("Open this comment on GitHub")
                        }
                    }
                    Text(entry.body ?? "No written comment").textSelection(.enabled)
                    if let path = entry.path {
                        Text("\(path)\(entry.line.map { ":\($0)" } ?? "")")
                            .font(.caption.monospaced()).foregroundStyle(.secondary)
                    }
                }
                .padding(.vertical, 6)
            }
        }
    }

    private func checks(for item: GitHubWorkItem) -> some View {
        let checks = snapshot.checks.filter { $0.itemNumber == item.number }
        return VStack(alignment: .leading, spacing: 8) {
            Divider()
            Text("Checks (\(checks.count))").font(.headline)
            if checks.isEmpty { Text("No checks ingested").foregroundStyle(.secondary) }
            ForEach(checks) { check in
                Label("\(check.name): \(check.conclusion ?? check.status)", systemImage: check.conclusion == "success" ? "checkmark.circle.fill" : "circle.dashed")
            }
        }
    }
}

private struct GitHubWorkItemRow: View {
    let item: GitHubWorkItem
    var body: some View {
        HStack(alignment: .top) {
            Image(systemName: item.kind == .pullRequest ? "arrow.triangle.pull" : "record.circle")
                .foregroundStyle(item.state == "open" ? .green : .purple)
            VStack(alignment: .leading, spacing: 3) {
                Text(item.title).lineLimit(2)
                Text("#\(item.number) · \(item.author)\(item.draft ? " · Draft" : "")")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 3)
    }
}
