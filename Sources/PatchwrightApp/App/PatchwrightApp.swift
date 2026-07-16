import PatchwrightCore
import SwiftUI

@main
struct PatchwrightApp: App {
    @StateObject private var engineProcess: EngineProcessController
    @StateObject private var store: WorkspaceStore
    @StateObject private var updateController: UpdateController

    init() {
        let engineProcess = EngineProcessController()
        _engineProcess = StateObject(wrappedValue: engineProcess)
        _store = StateObject(wrappedValue: WorkspaceStore(engine: UnixEngineClient(socketPath: engineProcess.socketPath)))
        _updateController = StateObject(wrappedValue: UpdateController())
    }

    var body: some Scene {
        WindowGroup("Patchwright") { ContentView(store: store) }
            .defaultSize(width: 1180, height: 760)
            .commands { PatchwrightCommands(store: store, updateController: updateController) }
        Settings { SettingsView() }
    }
}
