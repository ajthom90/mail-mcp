import Foundation
import AppKit

/// Spawns the bundled mail-mcp-daemon when the IPC socket is missing or
/// unreachable. Waits up to `readyTimeout` for endpoint.json to appear.
public final class DaemonLauncher {
    public enum LaunchError: Error, LocalizedError {
        case daemonBinaryMissing
        case missingClientId
        case neverReady(timeoutSeconds: Int)
        case process(Error)

        public var errorDescription: String? {
            switch self {
            case .daemonBinaryMissing:
                return "Bundled mail-mcp-daemon binary is missing from the app bundle."
            case .missingClientId:
                return "MAIL_MCP_GOOGLE_CLIENT_ID is not configured. Add it to Config-Local.xcconfig."
            case .neverReady(let s):
                return "Daemon failed to write endpoint.json within \(s) seconds."
            case .process(let e):
                return "Daemon launch failed: \(e.localizedDescription)"
            }
        }
    }

    private let paths: MailMCPPaths
    private let bundle: Bundle
    private let readyTimeoutSeconds: Int

    public init(
        paths: MailMCPPaths = .defaultForUser(),
        bundle: Bundle = .main,
        readyTimeoutSeconds: Int = 5
    ) {
        self.paths = paths
        self.bundle = bundle
        self.readyTimeoutSeconds = readyTimeoutSeconds
    }

    /// Returns immediately if daemon is already up, otherwise spawns it and
    /// blocks (async) until endpoint.json appears.
    public func ensureRunning() async throws {
        if isAliveQuick() { return }
        try await spawnAndWait()
    }

    /// Quick no-blocking check: does the IPC socket exist and is the pid file fresh?
    public func isAliveQuick() -> Bool {
        FileManager.default.fileExists(atPath: paths.ipcSocket.path)
    }

    private func spawnAndWait() async throws {
        guard let url = bundle.url(forAuxiliaryExecutable: "mail-mcp-daemon") else {
            throw LaunchError.daemonBinaryMissing
        }
        let clientId = bundle.object(forInfoDictionaryKey: "MAIL_MCP_GOOGLE_CLIENT_ID") as? String
        guard let clientId, !clientId.isEmpty,
              clientId != "your-client-id.apps.googleusercontent.com"
        else {
            throw LaunchError.missingClientId
        }

        try? FileManager.default.createDirectory(
            at: paths.dataDir, withIntermediateDirectories: true
        )

        let proc = Process()
        proc.executableURL = url
        proc.arguments = ["--google-client-id", clientId]
        proc.environment = ProcessInfo.processInfo.environment
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = FileHandle.nullDevice

        do {
            try proc.run()
        } catch {
            throw LaunchError.process(error)
        }

        // Poll for endpoint.json.
        let deadline = Date().addingTimeInterval(TimeInterval(readyTimeoutSeconds))
        while Date() < deadline {
            if FileManager.default.fileExists(atPath: paths.endpointJSON.path)
                && FileManager.default.fileExists(atPath: paths.ipcSocket.path) {
                return
            }
            try? await Task.sleep(for: .milliseconds(100))
        }
        throw LaunchError.neverReady(timeoutSeconds: readyTimeoutSeconds)
    }
}
