#include <adwaita.h>

static void on_activate(AdwApplication *app, gpointer user_data)
{
    (void)app;
    (void)user_data;
    g_message("MailMCP tray — version %s", MAIL_MCP_VERSION);
}

int main(int argc, char **argv)
{
    g_autoptr(AdwApplication) app =
        adw_application_new("io.github.ajthom90.MailMCP",
                            G_APPLICATION_DEFAULT_FLAGS);
    g_signal_connect(app, "activate", G_CALLBACK(on_activate), NULL);
    return g_application_run(G_APPLICATION(app), argc, argv);
}
