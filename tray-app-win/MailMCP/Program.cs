using System.Windows.Forms;

namespace MailMCP;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        var tray = new TrayController();
        try
        {
            tray.Start();
            Application.Run();
        }
        finally
        {
            tray.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }
}
