using MailMCP.IPC;
using Xunit;

namespace MailMCP.Tests;

public class PathsTests
{
    [Fact]
    public void DefaultPathsLandUnderLocalAppData()
    {
        // Make sure no leftover MAIL_MCP_ROOT from a sibling test pollutes.
        Environment.SetEnvironmentVariable("MAIL_MCP_ROOT", null);
        var p = MailMCPPaths.DefaultForUser();
        Assert.Contains("mail-mcp", p.DataDir);
        Assert.StartsWith(@"\\.\pipe\mail-mcp-", p.IpcPipe);
    }

    [Fact]
    public void EnvOverrideRoutesToCustomRoot()
    {
        var root = Path.Combine(Path.GetTempPath(), $"mailmcp-test-{Guid.NewGuid():N}");
        Environment.SetEnvironmentVariable("MAIL_MCP_ROOT", root);
        try
        {
            var p = MailMCPPaths.DefaultForUser();
            Assert.Equal(Path.Combine(root, "data"), p.DataDir);
            Assert.Equal(Path.Combine(root, "logs"), p.LogsDir);
            Assert.Equal(Path.Combine(root, "cache"), p.CacheDir);
            Assert.Equal(Path.Combine(root, "run"), p.RuntimeDir);
            // Pipe address still uses the OS pipe namespace, not the temp root.
            Assert.StartsWith(@"\\.\pipe\mail-mcp-", p.IpcPipe);
        }
        finally
        {
            Environment.SetEnvironmentVariable("MAIL_MCP_ROOT", null);
        }
    }
}
