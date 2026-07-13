import PatchwrightCore
import SwiftUI

@main
struct PatchwrightApp: App {
    @StateObject private var store: WorkspaceStore

    init() {
        let socket = ProcessInfo.processInfo.environment["PATCHWRIGHT_SOCKET"]
            ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: ".patchwright/engine.sock").path
        _store = StateObject(wrappedValue: WorkspaceStore(engine: UnixEngineClient(socketPath: socket)))
    }

    var body: some Scene {
        WindowGroup("Patchwright") { ContentView(store: store) }
            .defaultSize(width: 1180, height: 760)
            .commands { PatchwrightCommands(store: store) }
        Settings { SettingsView() }
    }
}

