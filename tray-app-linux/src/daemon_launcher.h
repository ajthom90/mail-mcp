#pragma once
#include <glib.h>

typedef struct _MailMcpDaemonLauncher MailMcpDaemonLauncher;

MailMcpDaemonLauncher *mailmcp_daemon_launcher_new(void);
void                   mailmcp_daemon_launcher_free(MailMcpDaemonLauncher *);
G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpDaemonLauncher, mailmcp_daemon_launcher_free)

/* If the daemon is already running (endpoint.json present + IPC socket
 * connectable), returns TRUE immediately. Otherwise locates `mail-mcp-daemon`,
 * spawns it, and polls up to 5 s for endpoint.json to appear. Returns FALSE
 * + sets *err on failure. */
gboolean mailmcp_daemon_launcher_ensure_running(MailMcpDaemonLauncher *,
                                                 GError                **err);
