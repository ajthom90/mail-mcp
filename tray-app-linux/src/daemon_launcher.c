#include "daemon_launcher.h"
#include "paths.h"
#include <gio/gio.h>
#include <unistd.h>

#define DAEMON_BINARY     "mail-mcp-daemon"
#define ENDPOINT_TIMEOUT_S 5

struct _MailMcpDaemonLauncher {
    GSubprocess *proc;
};

MailMcpDaemonLauncher *mailmcp_daemon_launcher_new(void)
{
    return g_new0(MailMcpDaemonLauncher, 1);
}

void mailmcp_daemon_launcher_free(MailMcpDaemonLauncher *l)
{
    if (!l) return;
    if (l->proc) {
        /* We don't kill the daemon on tray exit — it stays running so the
         * MCP host (Claude Desktop, etc.) keeps its connection. */
        g_object_unref(l->proc);
    }
    g_free(l);
}

/* Returns the directory containing the currently-running tray binary, or
 * NULL on failure. Caller frees with g_free. */
static char *self_exe_dir(void)
{
    g_autofree char *target = g_file_read_link("/proc/self/exe", NULL);
    if (!target) return NULL;
    return g_path_get_dirname(target);
}

/* Locates the daemon executable. Search order matches the design spec:
 *   1. Same dir as the tray binary (bundled install case)
 *   2. ${XDG_DATA_HOME}/mail-mcp/bin/mail-mcp-daemon (user-local install)
 *   3. PATH lookup
 * Returns NULL if not found. */
static char *find_daemon_binary(void)
{
    g_autofree char *self_dir = self_exe_dir();
    if (self_dir) {
        g_autofree char *bundled = g_build_filename(self_dir, DAEMON_BINARY, NULL);
        if (g_file_test(bundled, G_FILE_TEST_IS_EXECUTABLE)) return g_steal_pointer(&bundled);
    }
    g_autofree char *data_dir = mailmcp_data_dir();
    g_autofree char *user_local = g_build_filename(data_dir, "bin", DAEMON_BINARY, NULL);
    if (g_file_test(user_local, G_FILE_TEST_IS_EXECUTABLE)) return g_steal_pointer(&user_local);
    return g_find_program_in_path(DAEMON_BINARY);
}

/* Polls every 200ms up to ENDPOINT_TIMEOUT_S for endpoint.json to appear. */
static gboolean wait_for_endpoint(GError **err)
{
    g_autofree char *path = mailmcp_endpoint_file_path();
    gint64 deadline = g_get_monotonic_time() + (gint64)ENDPOINT_TIMEOUT_S * G_USEC_PER_SEC;
    while (g_get_monotonic_time() < deadline) {
        if (g_file_test(path, G_FILE_TEST_EXISTS)) return TRUE;
        g_usleep(200 * 1000);
    }
    g_set_error(err, G_FILE_ERROR, G_FILE_ERROR_NOENT,
                "daemon endpoint file did not appear at %s within %d s",
                path, ENDPOINT_TIMEOUT_S);
    return FALSE;
}

gboolean mailmcp_daemon_launcher_ensure_running(MailMcpDaemonLauncher *l, GError **err)
{
    g_return_val_if_fail(l != NULL, FALSE);

    /* Already running? endpoint.json is the canonical liveness signal. */
    g_autofree char *endpoint = mailmcp_endpoint_file_path();
    if (g_file_test(endpoint, G_FILE_TEST_EXISTS)) return TRUE;

    g_autofree char *daemon = find_daemon_binary();
    if (!daemon) {
        g_set_error(err, G_FILE_ERROR, G_FILE_ERROR_NOENT,
                    "%s not found in tray dir, %s, or PATH",
                    DAEMON_BINARY, "${XDG_DATA_HOME}/mail-mcp/bin");
        return FALSE;
    }

    GSubprocessLauncher *launcher = g_subprocess_launcher_new(
        G_SUBPROCESS_FLAGS_STDOUT_SILENCE | G_SUBPROCESS_FLAGS_STDERR_SILENCE);
    /* MAIL_MCP_GOOGLE_CLIENT_ID env var, if the user has one set, gets
     * inherited automatically. The daemon's own CLI also accepts
     * --google-client-id, but we let the env var path drive it for parity
     * with macOS / Windows trays. */

    l->proc = g_subprocess_launcher_spawn(launcher, err, daemon, NULL);
    g_object_unref(launcher);
    if (!l->proc) return FALSE;

    return wait_for_endpoint(err);
}
