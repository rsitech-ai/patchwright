import PatchwrightCore
import SwiftUI

@main
struct PatchwrightApp: App {
    @StateObject private var engineProcess: EngineProcessController
    @StateObject private var store: WorkspaceStore

    init() {
        let engineProcess = EngineProcessController()
        _engineProcess = StateObject(wrappedValue: engineProcess)
        _store = StateObject(wrappedValue: WorkspaceStore(engine: UnixEngineClient(socketPath: engineProcess.socketPath)))
    }

    var body: some Scene {
        WindowGroup("Patchwright") { ContentView(store: store) }
            .defaultSize(width: 1180, height: 760)
            .commands { PatchwrightCommands(store: store) }
        Settings { SettingsView() }
    }
}
