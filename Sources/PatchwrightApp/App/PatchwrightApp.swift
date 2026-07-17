import AppKit
import PatchwrightCore
import SwiftUI

@MainActor
final class PatchwrightApplicationDelegate: NSObject, NSApplicationDelegate {
    static weak var engineProcessController: EngineProcessController?

    func applicationWillTerminate(_: Notification) {
        Self.engineProcessController?.shutdown()
    }
}

@main
struct PatchwrightApp: App {
    @NSApplicationDelegateAdaptor(PatchwrightApplicationDelegate.self) private var appDelegate
    @StateObject private var engineProcess: EngineProcessController
    @StateObject private var store: WorkspaceStore
    @StateObject private var updateController: UpdateController

    init() {
        let engineProcess = EngineProcessController()
        _engineProcess = StateObject(wrappedValue: engineProcess)
        _store = StateObject(wrappedValue: WorkspaceStore(engine: UnixEngineClient(socketPath: engineProcess.socketPath)))
        _updateController = StateObject(wrappedValue: UpdateController())
        PatchwrightApplicationDelegate.engineProcessController = engineProcess
    }

    var body: some Scene {
        WindowGroup("Patchwright") { ContentView(store: store) }
            .defaultSize(width: 1180, height: 760)
            .commands { PatchwrightCommands(store: store, updateController: updateController) }
        Settings { SettingsView() }
    }
}
