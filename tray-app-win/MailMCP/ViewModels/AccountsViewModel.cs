using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using MailMCP.IPC;

namespace MailMCP.ViewModels;

/// <summary>
/// Live account list view-model. Fetches via <c>accounts.list</c> on start
/// and refreshes on every <c>account.added</c> / <c>account.removed</c> /
/// <c>account.needs_reauth</c> notification. Mirrors the v0.1b
/// AccountsViewModel.
/// </summary>
public partial class AccountsViewModel : ObservableObject
{
    private readonly IpcClient _client;
    private CancellationTokenSource? _cts;
    private Task? _notifTask;

    public ObservableCollection<AccountListItem> Accounts { get; } = new();

    [ObservableProperty] private bool _isLoading;
    [ObservableProperty] private string? _lastError;

    public AccountsViewModel(IpcClient client) { _client = client; }

    public void Start()
    {
        _cts = new CancellationTokenSource();
        _ = RefreshAsync(_cts.Token);
        _notifTask = Task.Run(() => NotificationLoop(_cts.Token));
    }

    public async Task StopAsync()
    {
        _cts?.Cancel();
        try
        {
            if (_notifTask is not null) await _notifTask.ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }
    }

    public async Task RefreshAsync(CancellationToken ct = default)
    {
        IsLoading = true;
        try
        {
            var fresh = await _client.CallAsync<AccountListItem[]>("accounts.list", ct: ct)
                .ConfigureAwait(false);
            Accounts.Clear();
            foreach (var a in fresh) Accounts.Add(a);
            LastError = null;
        }
        catch (Exception ex)
        {
            LastError = ex.Message;
        }
        finally { IsLoading = false; }
    }

    public async Task RemoveAsync(string accountId, CancellationToken ct = default)
    {
        try
        {
            _ = await _client.CallAsync("accounts.remove", new { account_id = accountId }, ct)
                .ConfigureAwait(false);
            await RefreshAsync(ct).ConfigureAwait(false);
        }
        catch (Exception ex) { LastError = ex.Message; }
    }

    private async Task NotificationLoop(CancellationToken ct)
    {
        try
        {
            var stream = await _client.SubscribeAsync(
                new[] { "account.added", "account.removed", "account.needs_reauth" }, ct)
                .ConfigureAwait(false);
            await foreach (var note in stream.WithCancellation(ct).ConfigureAwait(false))
            {
                _ = note;  // any of these means refresh
                await RefreshAsync(ct).ConfigureAwait(false);
            }
        }
        catch (OperationCanceledException) { /* graceful */ }
        catch (Exception ex) { LastError = ex.Message; }
    }
}
