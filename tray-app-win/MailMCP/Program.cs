using System.Windows.Forms;

namespace MailMCP;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        using var tray = new TrayController();
        tray.Start();
        Application.Run();
    }
}
