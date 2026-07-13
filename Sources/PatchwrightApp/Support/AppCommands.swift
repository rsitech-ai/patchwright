import PatchwrightCore
import SwiftUI

struct PatchwrightCommands: Commands {
    @ObservedObject var store: WorkspaceStore
    var body: some Commands {
        CommandMenu("Task") {
            Button("Refresh Engine") { Task { await store.refreshHealth() } }
                .keyboardShortcut("r", modifiers: [.command, .shift])
        }
    }
}
