import PatchwrightCore
import SwiftUI

struct PatchwrightCommands: Commands {
    @ObservedObject var store: WorkspaceStore
    @ObservedObject var updateController: UpdateController

    var body: some Commands {
        CommandGroup(after: .appInfo) {
            Button("Check for Updates…") {
                updateController.checkForUpdates()
            }
            .disabled(!updateController.canCheckForUpdates)
        }

        CommandMenu("Task") {
            Button("Refresh Engine") { Task { await store.refreshHealth() } }
                .keyboardShortcut("r", modifiers: [.command, .shift])
            Button(store.isSyncingGitHub ? "Cancel GitHub Sync" : "Sync GitHub") {
                Task {
                    if store.isSyncingGitHub { await store.cancelGitHubSync() }
                    else { await store.syncGitHub() }
                }
            }
                .keyboardShortcut("g", modifiers: [.command, .shift])
        }
    }
}
