import PatchwrightCore
import SwiftUI

struct PatchwrightCommands: Commands {
    @ObservedObject var store: WorkspaceStore
    var body: some Commands {
        CommandMenu("Task") {
            Button("Refresh Engine") { Task { await store.refreshHealth() } }
                .keyboardShortcut("r", modifiers: [.command, .shift])
            Button("Sync GitHub") { Task { await store.syncGitHub() } }
                .keyboardShortcut("g", modifiers: [.command, .shift])
                .disabled(store.isSyncingGitHub)
        }
    }
}
