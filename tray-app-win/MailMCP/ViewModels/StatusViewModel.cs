using CommunityToolkit.Mvvm.ComponentModel;
using MailMCP.IPC;

namespace MailMCP.ViewModels;

/// <summary>
/// Polls the daemon for status every 30 s and reacts to notifications. Properties
/// are observable so the tray menu / status bar text rebinds automatically when
/// state changes.
/// </summary>
public partial class StatusViewModel : ObservableObject
{
    private readonly IpcClient _client;
    private CancellationTokenSource? _cts;
    private Task? _pollTask;
    private Task? _notifTask;

    [ObservableProperty] private DaemonStatus? _status;
    [ObservableProperty] private int _pendingApprovalCount;
    [ObservableProperty] private string? _lastError;

    public StatusViewModel(IpcClient client) { _client = client; }

    public void Start()
    {
        _cts = new CancellationTokenSource();
        _pollTask = Task.Run(() => PollLoop(_cts.Token));
        _notifTask = Task.Run(() => NotificationLoop(_cts.Token));
    }

    public async Task StopAsync()
    {
        _cts?.Cancel();
        try
        {
            if (_pollTask is not null) await _pollTask.ConfigureAwait(false);
            if (_notifTask is not null) await _notifTask.ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }
    }

    public async Task RefreshAsync(CancellationToken ct = default)
    {
        try
        {
            Status = await _client.CallAsync<DaemonStatus>("status", ct: ct).ConfigureAwait(false);
            var pending = await _client.CallAsync<PendingApproval[]>("approvals.list", ct: ct).ConfigureAwait(false);
            PendingApprovalCount = pending.Length;
            LastError = null;
        }
        catch (Exception ex)
        {
            LastError = ex.Message;
        }
    }

    private async Task PollLoop(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            await RefreshAsync(ct).ConfigureAwait(false);
            try { await Task.Delay(TimeSpan.FromSeconds(30), ct).ConfigureAwait(false); }
            catch (OperationCanceledException) { return; }
        }
    }

    private async Task NotificationLoop(CancellationToken ct)
    {
        try
        {
            await foreach (var note in _client.SubscribeAsync(
                new[]
                {
                    "approval.requested",
                    "approval.resolved",
                    "account.added",
                    "account.removed",
                    "mcp.paused_changed",
                }, ct).ConfigureAwait(false))
            {
                switch (note)
                {
                    case DaemonNotification.ApprovalRequested:
                        PendingApprovalCount += 1;
                        break;
                    case DaemonNotification.ApprovalResolved:
                        PendingApprovalCount = Math.Max(0, PendingApprovalCount - 1);
                        break;
                    case DaemonNotification.AccountAdded:
                    case DaemonNotification.AccountRemoved:
                    case DaemonNotification.McpPausedChanged:
                        await RefreshAsync(ct).ConfigureAwait(false);
                        break;
                }
            }
        }
        catch (OperationCanceledException) { /* graceful */ }
        catch (Exception ex) { LastError = ex.Message; }
    }
}
