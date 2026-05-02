using System.Text.Json;
using MailMCP.IPC;
using Xunit;

namespace MailMCP.Tests;

public class ModelsTests
{
    [Fact]
    public void AccountListItemDecodesSnakeCase()
    {
        const string json = """
        [{"id":"01H123","label":"Personal","provider":"gmail","email":"alice@example.com","status":"ok"}]
        """;
        var items = JsonSerializer.Deserialize<AccountListItem[]>(json)!;
        Assert.Single(items);
        Assert.Equal("alice@example.com", items[0].Email);
        Assert.Equal(AccountStatus.Ok, items[0].Status);
    }

    [Fact]
    public void AccountStatusNeedsReauth()
    {
        const string json = "\"needs_reauth\"";
        var s = JsonSerializer.Deserialize<AccountStatus>(json);
        Assert.Equal(AccountStatus.NeedsReauth, s);
    }

    [Fact]
    public void NotificationApprovalRequestedDecodes()
    {
        const string json = """
        {
            "method":"approval.requested",
            "params":{
                "id":"01H4567","account":"01H123","category":"send",
                "summary":"send_message","details":{"to":["a@b.com"]},
                "created_at":"2026-05-01T00:00:00Z","expires_at":"2026-05-01T00:05:00Z"
            }
        }
        """;
        using var doc = JsonDocument.Parse(json);
        var n = DaemonNotification.Decode(doc.RootElement);
        var ar = Assert.IsType<DaemonNotification.ApprovalRequested>(n);
        Assert.Equal("01H4567", ar.Approval.Id);
        Assert.Equal(Category.Send, ar.Approval.Category);
    }

    [Fact]
    public void NotificationMcpPausedChangedDecodes()
    {
        const string json = """{"method":"mcp.paused_changed","params":{"paused":true}}""";
        using var doc = JsonDocument.Parse(json);
        var n = DaemonNotification.Decode(doc.RootElement);
        var pc = Assert.IsType<DaemonNotification.McpPausedChanged>(n);
        Assert.True(pc.Paused);
    }

    [Fact]
    public void UnknownNotificationFallsBack()
    {
        const string json = """{"method":"future.event","params":{"x":1}}""";
        using var doc = JsonDocument.Parse(json);
        var n = DaemonNotification.Decode(doc.RootElement);
        var u = Assert.IsType<DaemonNotification.Unknown>(n);
        Assert.Equal("future.event", u.Method);
    }
}
