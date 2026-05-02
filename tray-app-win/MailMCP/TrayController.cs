using System.ComponentModel;
using System.Windows.Forms;
using MailMCP.Approvals;
using MailMCP.IPC;
using MailMCP.ViewModels;

namespace MailMCP;

/// <summary>
/// Owns the <see cref="NotifyIcon"/> (system-tray icon) and the per-app
/// long-running components: IPC client, daemon launcher, status view-model.
/// In Phase A this exposes a status row + Pause/Resume + Quit. The Settings
/// and Wizard windows are wired in later tasks.
/// </summary>
public sealed class TrayController : IAsyncDisposable
{
    private readonly MailMCPPaths _paths = MailMCPPaths.DefaultForUser();
    private readonly DaemonLauncher _launcher;
    private readonly IpcClient _client;
    private readonly StatusViewModel _statusVM;
    private readonly AccountsViewModel _accountsVM;
    private readonly ApprovalCoordinator _approvals;
    private NotifyIcon? _icon;
    private ContextMenuStrip? _menu;
    private ToolStripMenuItem? _statusItem;
    private ToolStripMenuItem? _approvalsItem;
    private ToolStripMenuItem? _pauseItem;

    public TrayController()
    {
        _launcher = new DaemonLauncher(_paths);
        _client = new IpcClient(_paths.IpcPipe);
        _statusVM = new StatusViewModel(_client);
        _statusVM.PropertyChanged += OnStatusPropertyChanged;
        _accountsVM = new AccountsViewModel(_client);
        _approvals = new ApprovalCoordinator(_client);
    }

    /// <summary>Exposed for the Settings window so it can bind to the same VM.</summary>
    public AccountsViewModel AccountsVM => _accountsVM;
    public StatusViewModel StatusVM => _statusVM;

    public void Start()
    {
        _menu = BuildMenu();
        _icon = new NotifyIcon
        {
            Icon = System.Drawing.SystemIcons.Application,
            Visible = true,
            Text = "MailMCP — starting…",
            ContextMenuStrip = _menu,
        };

        _ = Task.Run(StartupAsync);
    }

    private async Task StartupAsync()
    {
        try
        {
            await _launcher.EnsureRunningAsync().ConfigureAwait(false);
            await _client.ConnectAsync().ConfigureAwait(false);
            _statusVM.Start();
            _accountsVM.Start();
            _approvals.Start();
        }
        catch (Exception ex)
        {
            UpdateUI(() =>
            {
                if (_icon is not null) _icon.Text = "MailMCP — daemon error";
                if (_statusItem is not null) _statusItem.Text = $"Error: {ex.Message}";
            });
        }
    }

    private ContextMenuStrip BuildMenu()
    {
        var menu = new ContextMenuStrip();
        _statusItem = new ToolStripMenuItem("Connecting…") { Enabled = false };
        _approvalsItem = new ToolStripMenuItem("Pending approvals: 0")
        {
            Enabled = false,
            Visible = false,
        };
        menu.Items.Add(_statusItem);
        menu.Items.Add(_approvalsItem);
        menu.Items.Add(new ToolStripSeparator());

        _pauseItem = new ToolStripMenuItem("Pause MCP");
        _pauseItem.Click += async (_, _) => await TogglePauseAsync().ConfigureAwait(false);
        menu.Items.Add(_pauseItem);

        menu.Items.Add(new ToolStripSeparator());
        var quit = new ToolStripMenuItem("Quit MailMCP");
        quit.Click += (_, _) => Application.Exit();
        menu.Items.Add(quit);
        return menu;
    }

    private void OnStatusPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        UpdateUI(RefreshMenu);
    }

    private void RefreshMenu()
    {
        var s = _statusVM.Status;
        if (s is not null)
        {
            var line = $"Status: {(s.McpPaused ? "Paused" : "Connected")} — {s.AccountCount} account{(s.AccountCount == 1 ? "" : "s")}";
            if (_statusItem is not null) _statusItem.Text = line;
            if (_icon is not null) _icon.Text = line.Length > 63 ? line[..63] : line;
            if (_pauseItem is not null) _pauseItem.Text = s.McpPaused ? "Resume MCP" : "Pause MCP";
        }
        else if (_statusVM.LastError is not null && _statusItem is not null)
        {
            _statusItem.Text = $"Error: {_statusVM.LastError}";
        }
        if (_approvalsItem is not null)
        {
            var count = _statusVM.PendingApprovalCount;
            _approvalsItem.Text = $"Pending approvals: {count}";
            _approvalsItem.Visible = count > 0;
        }
    }

    private async Task TogglePauseAsync()
    {
        try
        {
            var paused = !(_statusVM.Status?.McpPaused ?? false);
            _ = await _client.CallAsync("mcp.pause", new { paused }).ConfigureAwait(false);
            await _statusVM.RefreshAsync().ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            MessageBox.Show($"Pause/Resume failed: {ex.Message}", "MailMCP",
                MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    /// <summary>
    /// Marshal a UI update back to the WinForms message-loop thread that owns
    /// the menu. ContextMenuStrip exposes InvokeRequired/BeginInvoke; NotifyIcon
    /// itself does not.
    /// </summary>
    private void UpdateUI(Action action)
    {
        if (_menu is null || !_menu.InvokeRequired) action();
        else _menu.BeginInvoke(action);
    }

    public async ValueTask DisposeAsync()
    {
        await _approvals.DisposeAsync().ConfigureAwait(false);
        await _accountsVM.StopAsync().ConfigureAwait(false);
        await _statusVM.StopAsync().ConfigureAwait(false);
        await _client.DisposeAsync().ConfigureAwait(false);
        _icon?.Dispose();
        _menu?.Dispose();
    }
}
