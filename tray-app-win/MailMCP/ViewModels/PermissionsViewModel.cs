using CommunityToolkit.Mvvm.ComponentModel;
using MailMCP.IPC;

namespace MailMCP.ViewModels;

/// <summary>
/// Per-account permission matrix. Bound by the Settings → Permissions page.
/// Setting <see cref="SelectedAccountId"/> kicks off a refresh; calling
/// <see cref="SetAsync"/> updates one cell of the matrix and re-fetches.
/// Mirrors the v0.1b PermissionsViewModel.
/// </summary>
public partial class PermissionsViewModel : ObservableObject
{
    private readonly IpcClient _client;

    [ObservableProperty] private PermissionMap? _permissions;
    [ObservableProperty] private string? _lastError;
    [ObservableProperty] private string? _selectedAccountId;

    public PermissionsViewModel(IpcClient client) { _client = client; }

    partial void OnSelectedAccountIdChanged(string? value)
    {
        _ = RefreshAsync();
    }

    public async Task RefreshAsync(CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(SelectedAccountId))
        {
            Permissions = null;
            return;
        }
        try
        {
            Permissions = await _client.CallAsync<PermissionMap>(
                "permissions.get",
                new { account_id = SelectedAccountId },
                ct).ConfigureAwait(false);
            LastError = null;
        }
        catch (Exception ex)
        {
            LastError = ex.Message;
        }
    }

    public async Task SetAsync(Category category, Policy policy, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(SelectedAccountId)) return;
        try
        {
            _ = await _client.CallAsync(
                "permissions.set",
                new
                {
                    account_id = SelectedAccountId,
                    category = category.ToString().ToLowerInvariant(),
                    policy = policy.ToString().ToLowerInvariant(),
                },
                ct).ConfigureAwait(false);
            await RefreshAsync(ct).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            LastError = ex.Message;
        }
    }
}
