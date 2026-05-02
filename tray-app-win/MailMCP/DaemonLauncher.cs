using System.Diagnostics;
using System.IO;
using MailMCP.IPC;

namespace MailMCP;

/// <summary>
/// Spawns the bundled mail-mcp-daemon.exe when the IPC pipe is missing or
/// unreachable. Waits up to <see cref="ReadyTimeoutSeconds"/> for the daemon
/// to write endpoint.json, then returns. Mirrors the v0.1b macOS DaemonLauncher.
/// </summary>
public sealed class DaemonLauncher
{
    public sealed class LaunchException : Exception
    {
        public LaunchException(string message) : base(message) { }
    }

    private readonly MailMCPPaths _paths;
    private readonly string _daemonExePath;

    public int ReadyTimeoutSeconds { get; init; } = 5;

    public DaemonLauncher(MailMCPPaths? paths = null, string? daemonExePathOverride = null)
    {
        _paths = paths ?? MailMCPPaths.DefaultForUser();
        _daemonExePath = daemonExePathOverride ?? DefaultDaemonExePath();
    }

    /// <summary>
    /// Spawn the daemon if it isn't already running. No-op if the IPC pipe
    /// already exists (which means another daemon instance is up — its PID
    /// lock will refuse a second spawn anyway).
    /// </summary>
    public async Task EnsureRunningAsync(CancellationToken ct = default)
    {
        if (IsAliveQuick()) return;
        await SpawnAndWaitAsync(ct).ConfigureAwait(false);
    }

    /// <summary>
    /// Quick check: does the IPC pipe address exist? On Windows, named pipes
    /// don't show up as filesystem entries, so we instead try a brief connect
    /// — if it succeeds, the daemon is up.
    /// </summary>
    public bool IsAliveQuick()
    {
        // Try a non-blocking connect with a tiny timeout. If the pipe isn't
        // there we get TimeoutException; the cost on a normal "daemon up"
        // path is one syscall.
        try
        {
            using var client = new System.IO.Pipes.NamedPipeClientStream(
                serverName: ".",
                pipeName: PipeNameFromAddress(_paths.IpcPipe),
                direction: System.IO.Pipes.PipeDirection.InOut,
                options: System.IO.Pipes.PipeOptions.Asynchronous);
            client.Connect(timeout: 100);
            return client.IsConnected;
        }
        catch (TimeoutException) { return false; }
        catch (IOException) { return false; }
    }

    private async Task SpawnAndWaitAsync(CancellationToken ct)
    {
        if (!File.Exists(_daemonExePath))
        {
            throw new LaunchException(
                $"Bundled daemon not found at {_daemonExePath}. Did the BuildDaemon target run?");
        }

        var clientId = Environment.GetEnvironmentVariable("MAIL_MCP_GOOGLE_CLIENT_ID")
            ?? GetClientIdFromConfig();
        if (string.IsNullOrEmpty(clientId)
            || clientId == "your-client-id.apps.googleusercontent.com")
        {
            throw new LaunchException(
                "MAIL_MCP_GOOGLE_CLIENT_ID is not configured. Set it in the env or " +
                "in the per-user config (Phase B will surface this in the wizard).");
        }

        Directory.CreateDirectory(_paths.DataDir);

        var psi = new ProcessStartInfo
        {
            FileName = _daemonExePath,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        psi.ArgumentList.Add("--google-client-id");
        psi.ArgumentList.Add(clientId);

        try
        {
            using var proc = Process.Start(psi);
            if (proc is null) throw new LaunchException("Process.Start returned null");
            // Daemon is now spawned; we don't wait on the process — it lives for
            // the rest of the user's login session (or until Quit from the tray).
        }
        catch (Exception ex) when (ex is not LaunchException)
        {
            throw new LaunchException($"Spawning daemon failed: {ex.Message}");
        }

        // Poll for endpoint.json + the IPC pipe.
        var deadline = DateTime.UtcNow.AddSeconds(ReadyTimeoutSeconds);
        while (DateTime.UtcNow < deadline)
        {
            ct.ThrowIfCancellationRequested();
            if (File.Exists(_paths.EndpointJson) && IsAliveQuick()) return;
            await Task.Delay(100, ct).ConfigureAwait(false);
        }
        throw new LaunchException(
            $"Daemon failed to write {_paths.EndpointJson} within {ReadyTimeoutSeconds}s.");
    }

    /// <summary>
    /// Locate the bundled daemon. The MSBuild BuildDaemon target copies it
    /// next to the .NET executable, so it's a sibling of the entry assembly.
    /// </summary>
    private static string DefaultDaemonExePath()
    {
        var dir = AppContext.BaseDirectory;
        return Path.Combine(dir, "mail-mcp-daemon.exe");
    }

    /// <summary>
    /// Read an optional per-user config that may carry the OAuth client_id.
    /// In Phase A we just check %LOCALAPPDATA%\mail-mcp\client_id (a one-line
    /// file). The wizard's "Configure AI client" page can write it later.
    /// </summary>
    private static string? GetClientIdFromConfig()
    {
        try
        {
            var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            var path = Path.Combine(local, "mail-mcp", "client_id");
            return File.Exists(path) ? File.ReadAllText(path).Trim() : null;
        }
        catch { return null; }
    }

    private static string PipeNameFromAddress(string address)
    {
        const string Prefix = @"\\.\pipe\";
        return address.StartsWith(Prefix, StringComparison.Ordinal)
            ? address[Prefix.Length..]
            : address;
    }
}
