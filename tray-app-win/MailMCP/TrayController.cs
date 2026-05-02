using System.Windows.Forms;

namespace MailMCP;

/// <summary>
/// Owns the <see cref="NotifyIcon"/> (system-tray icon) and the per-app
/// long-running components: IPC client, daemon launcher, view-models,
/// approval coordinator. In Phase A this exposes a minimal menu; richer
/// status rendering ships in later tasks.
/// </summary>
public sealed class TrayController
{
    private NotifyIcon? _icon;

    public void Start()
    {
        // Use the application's default icon from the build output until we
        // ship a proper monochrome tray icon in Resources/Assets (Task 21).
        _icon = new NotifyIcon
        {
            Icon = System.Drawing.SystemIcons.Application,
            Visible = true,
            Text = "MailMCP — starting…",
            ContextMenuStrip = BuildMenu(),
        };
    }

    private static ContextMenuStrip BuildMenu()
    {
        var menu = new ContextMenuStrip();
        menu.Items.Add("MailMCP — starting…").Enabled = false;
        menu.Items.Add(new ToolStripSeparator());
        var quit = new ToolStripMenuItem("Quit");
        quit.Click += (_, _) => Application.Exit();
        menu.Items.Add(quit);
        return menu;
    }
}
