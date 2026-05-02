using MailMCP.IPC;
using Xunit;

namespace MailMCP.Tests;

public class IpcClientTests
{
    [Fact]
    public async Task StatusCallParsesResult()
    {
        await using var server = new MockIpcServer();
        server.Responses["status"] = """
        {"jsonrpc":"2.0","id":$ID,"result":{
            "version":"0.1.0","uptime_secs":42,
            "account_count":1,"mcp_paused":false,"onboarding_complete":true
        }}
        """;
        server.Start();
        await using var client = new IpcClient(server.PipeAddress);
        await client.ConnectAsync();

        var s = await client.CallAsync<DaemonStatus>("status");
        Assert.Equal("0.1.0", s.Version);
        Assert.Equal(42UL, s.UptimeSecs);
        Assert.Equal(1U, s.AccountCount);
        Assert.True(s.OnboardingComplete);
    }

    [Fact]
    public async Task RpcErrorIsThrown()
    {
        await using var server = new MockIpcServer();
        server.Responses["bad"] = """
        {"jsonrpc":"2.0","id":$ID,"error":{"code":-32601,"message":"not found"}}
        """;
        server.Start();
        await using var client = new IpcClient(server.PipeAddress);
        await client.ConnectAsync();

        var ex = await Assert.ThrowsAsync<IpcClient.IpcException>(
            async () => await client.CallAsync<EmptyResp>("bad"));
        Assert.Equal(-32601, ex.Code);
        Assert.Equal("not found", ex.Message);
    }

    [Fact]
    public async Task SubscribeReceivesNotification()
    {
        await using var server = new MockIpcServer();
        server.Start();
        await using var client = new IpcClient(server.PipeAddress);
        await client.ConnectAsync();

        var cts = new CancellationTokenSource(TimeSpan.FromSeconds(2));
        var stream = client.SubscribeAsync(new[] { "mcp.paused_changed" }, cts.Token);

        // Push the notification AFTER subscribe is acknowledged. The default
        // mock-server response for `subscribe` returns subscribed:[] so the
        // client's await unblocks before we push.
        await server.PushNotificationAsync("""
            {"jsonrpc":"2.0","method":"mcp.paused_changed","params":{"paused":true}}
            """);

        await foreach (var note in stream.WithCancellation(cts.Token))
        {
            var pc = Assert.IsType<DaemonNotification.McpPausedChanged>(note);
            Assert.True(pc.Paused);
            return;
        }
        Assert.Fail("notification stream ended before yielding");
    }

    private sealed record EmptyResp();
}
