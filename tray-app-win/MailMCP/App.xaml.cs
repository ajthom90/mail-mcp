using Microsoft.UI.Xaml;

namespace MailMCP;

/// <summary>
/// MailMCP application entry point. Sets up the system-tray controller and
/// hands off to it for the lifetime of the process. The Window-mode pages
/// (Settings, Wizard) are owned by the tray controller and opened on demand.
/// </summary>
public partial class App : Application
{
    private TrayController? _tray;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _tray = new TrayController();
        _tray.Start();
    }
}
