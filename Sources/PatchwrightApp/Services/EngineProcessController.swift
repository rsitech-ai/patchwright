import Darwin
import Foundation

@MainActor
final class EngineProcessController: ObservableObject {
    private var process: Process?
    let socketPath: String

    init() {
        let environment = ProcessInfo.processInfo.environment
        let stateDirectory = FileManager.default.homeDirectoryForCurrentUser.appending(path: ".patchwright", directoryHint: .isDirectory)
        socketPath = environment["PATCHWRIGHT_SOCKET"] ?? stateDirectory.appending(path: "engine.sock").path
        guard environment["PATCHWRIGHT_EXTERNAL_ENGINE"] != "1" else { return }
        do {
            try FileManager.default.createDirectory(at: stateDirectory, withIntermediateDirectories: true)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: stateDirectory.path)
            let bundled = Bundle.main.bundleURL.appending(path: "Contents/Helpers/patchwright-engine")
            let executable = environment["PATCHWRIGHT_ENGINE_BINARY"].map(URL.init(fileURLWithPath:)) ?? bundled
            guard FileManager.default.isExecutableFile(atPath: executable.path) else { return }
            let process = Process()
            process.executableURL = executable
            process.arguments = [
                "serve", "--socket", socketPath,
                "--database", environment["PATCHWRIGHT_DATABASE"] ?? stateDirectory.appending(path: "patchwright.sqlite3").path,
            ]
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.standardError
            try process.run()
            self.process = process
        } catch {
            NSLog("Patchwright engine launch failed: %@", error.localizedDescription)
        }
    }

    func shutdown() {
        guard let process else { return }
        self.process = nil
        guard process.isRunning else { return }

        process.terminate()
        let shutdownDeadline = Date.now.addingTimeInterval(2)
        while process.isRunning, Date.now < shutdownDeadline {
            Thread.sleep(forTimeInterval: 0.01)
        }
        if process.isRunning {
            _ = Darwin.kill(process.processIdentifier, SIGKILL)
            process.waitUntilExit()
        }
    }

    deinit {
        if process?.isRunning == true { process?.terminate() }
    }
}
