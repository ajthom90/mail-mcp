import Foundation

/// Mirrors the macOS branch of `mail-mcp-core::paths::Paths::default_for_user`.
/// Override via `MAIL_MCP_ROOT` env var (matches the daemon CLI flag).
public struct MailMCPPaths {
    public let dataDir: URL
    public let logsDir: URL
    public let cacheDir: URL
    public let runtimeDir: URL

    public var endpointJSON: URL { dataDir.appendingPathComponent("endpoint.json") }
    public var ipcSocket: URL    { runtimeDir.appendingPathComponent("ipc.sock") }
    public var pidFile: URL      { runtimeDir.appendingPathComponent("daemon.pid") }
    public var stateDB: URL      { dataDir.appendingPathComponent("state.db") }

    public static func defaultForUser() -> MailMCPPaths {
        if let root = ProcessInfo.processInfo.environment["MAIL_MCP_ROOT"] {
            return withRoot(URL(fileURLWithPath: root))
        }
        let home = FileManager.default.homeDirectoryForCurrentUser
        let data  = home.appendingPathComponent("Library/Application Support/mail-mcp")
        let logs  = home.appendingPathComponent("Library/Logs/mail-mcp")
        let cache = home.appendingPathComponent("Library/Caches/mail-mcp")
        let tmp = ProcessInfo.processInfo.environment["TMPDIR"]
            .map { URL(fileURLWithPath: $0) } ?? URL(fileURLWithPath: "/tmp")
        let uid = getuid()
        let runtime = tmp.appendingPathComponent("mail-mcp-\(uid)")
        return MailMCPPaths(dataDir: data, logsDir: logs, cacheDir: cache, runtimeDir: runtime)
    }

    public static func withRoot(_ root: URL) -> MailMCPPaths {
        MailMCPPaths(
            dataDir: root.appendingPathComponent("data"),
            logsDir: root.appendingPathComponent("logs"),
            cacheDir: root.appendingPathComponent("cache"),
            runtimeDir: root.appendingPathComponent("run")
        )
    }
}
