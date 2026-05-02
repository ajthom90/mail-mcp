using System.Text.Json;
using System.Windows.Forms;
using MailMCP.IPC;

namespace MailMCP.Approvals;

/// <summary>
/// Subscribes to <c>approval.requested</c> notifications and presents a modal
/// MessageBox per request. v0.1c Phase A uses MessageBox for simplicity; a
/// later Phase can swap to <c>ContentDialog</c> hosted on a hidden parent
/// Window for nicer styling. Decision is dispatched back to the daemon via
/// <c>approvals.decide</c>.
/// </summary>
public sealed class ApprovalCoordinator : IAsyncDisposable
{
    private readonly IpcClient _client;
    private CancellationTokenSource? _cts;
    private Task? _runTask;

    public ApprovalCoordinator(IpcClient client) { _client = client; }

    public void Start()
    {
        _cts = new CancellationTokenSource();
        _runTask = Task.Run(() => RunAsync(_cts.Token));
    }

    private async Task RunAsync(CancellationToken ct)
    {
        try
        {
            var stream = await _client.SubscribeAsync(
                new[] { "approval.requested" }, ct).ConfigureAwait(false);
            await foreach (var note in stream.WithCancellation(ct).ConfigureAwait(false))
            {
                if (note is DaemonNotification.ApprovalRequested ar)
                {
                    await PresentAsync(ar.Approval).ConfigureAwait(false);
                }
            }
        }
        catch (OperationCanceledException) { /* graceful */ }
    }

    private async Task PresentAsync(PendingApproval p)
    {
        var title = $"{p.Summary} on {p.Account}";
        var body = InformativeText(p);
        // MessageBox.Show is synchronous; bounce to a Task so we don't block
        // the receive loop. RunOnThreadPool by default.
        var result = await Task.Run(() =>
            MessageBox.Show(
                body,
                title,
                MessageBoxButtons.YesNo,
                MessageBoxIcon.Warning,
                MessageBoxDefaultButton.Button2)
        ).ConfigureAwait(false);
        var decision = result == DialogResult.Yes ? "approve" : "reject";
        try
        {
            _ = await _client.CallAsync("approvals.decide", new { id = p.Id, decision })
                .ConfigureAwait(false);
        }
        catch
        {
            // Daemon may have already auto-resolved on its own (timeout or
            // admin CLI decision); harmless.
        }
    }

    private static string InformativeText(PendingApproval p)
    {
        // For Send approvals, surface the recipient + subject. For Trash,
        // surface the message count. Otherwise generic.
        try
        {
            switch (p.Category)
            {
                case Category.Send when p.Details.ValueKind == JsonValueKind.Object:
                {
                    string to = "(none)";
                    string subject = "(no subject)";
                    if (p.Details.TryGetProperty("to", out var toEl)
                        && toEl.ValueKind == JsonValueKind.Array)
                    {
                        var addrs = new List<string>();
                        foreach (var a in toEl.EnumerateArray())
                        {
                            if (a.ValueKind == JsonValueKind.String) addrs.Add(a.GetString() ?? "");
                        }
                        if (addrs.Count > 0) to = string.Join(", ", addrs);
                    }
                    if (p.Details.TryGetProperty("subject", out var subjEl)
                        && subjEl.ValueKind == JsonValueKind.String)
                    {
                        subject = subjEl.GetString() ?? subject;
                    }
                    return $"To: {to}\nSubject: {subject}\n\nApprove?";
                }
                case Category.Trash when p.Details.ValueKind == JsonValueKind.Object
                    && p.Details.TryGetProperty("message_ids", out var ids)
                    && ids.ValueKind == JsonValueKind.Array:
                    return $"Move {ids.GetArrayLength()} message(s) to Trash. Approve?";
                default:
                    return "Action requires your approval. Approve?";
            }
        }
        catch
        {
            return "Action requires your approval. Approve?";
        }
    }

    public async ValueTask DisposeAsync()
    {
        _cts?.Cancel();
        try
        {
            if (_runTask is not null) await _runTask.ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }
    }
}
