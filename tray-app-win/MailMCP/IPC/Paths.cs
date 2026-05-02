namespace MailMCP.IPC;

/// <summary>
/// Mirrors the Windows branch of <c>mail-mcp-core::paths::Paths::default_for_user</c>.
/// Override every directory under a single root via the <c>MAIL_MCP_ROOT</c>
/// env var (matches the daemon CLI flag).
/// </summary>
public sealed record MailMCPPaths(
    string DataDir,
    string LogsDir,
    string CacheDir,
    string RuntimeDir,
    string IpcPipe)
{
    public string EndpointJson => Path.Combine(DataDir, "endpoint.json");
    public string PidFile => Path.Combine(RuntimeDir, "daemon.pid");
    public string StateDb => Path.Combine(DataDir, "state.db");

    public static MailMCPPaths DefaultForUser()
    {
        var root = Environment.GetEnvironmentVariable("MAIL_MCP_ROOT");
        if (!string.IsNullOrEmpty(root))
        {
            return new MailMCPPaths(
                DataDir: Path.Combine(root, "data"),
                LogsDir: Path.Combine(root, "logs"),
                CacheDir: Path.Combine(root, "cache"),
                RuntimeDir: Path.Combine(root, "run"),
                IpcPipe: PipeName());
        }
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var appDir = Path.Combine(local, "mail-mcp");
        return new MailMCPPaths(
            DataDir: appDir,
            LogsDir: Path.Combine(appDir, "logs"),
            CacheDir: Path.Combine(appDir, "cache"),
            RuntimeDir: Path.Combine(appDir, "run"),
            IpcPipe: PipeName());
    }

    /// <summary>
    /// The named-pipe address: <c>\\.\pipe\mail-mcp-{USERNAME}</c>. Per-user
    /// scoping keeps separate user accounts isolated. Username is enough;
    /// SIDs require P/Invoke and aren't worth the complexity here.
    /// </summary>
    private static string PipeName()
    {
        var user = string.IsNullOrEmpty(Environment.UserName) ? "default" : Environment.UserName;
        return $@"\\.\pipe\mail-mcp-{user}";
    }
}
