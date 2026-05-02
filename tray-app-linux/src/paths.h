#pragma once
#include <glib.h>

/* Returns paths used by the mail-mcp daemon and tray. The string returned
 * from each function is owned by the caller and freed with g_free(). */

/* ${XDG_RUNTIME_DIR}/mail-mcp/ipc.sock, with /tmp/mail-mcp-${UID}/ipc.sock
 * fallback when XDG_RUNTIME_DIR is unset (rare; covers minimal containers). */
char *mailmcp_ipc_socket_path(void);

/* Sibling of ipc.sock — the daemon writes its bound MCP endpoint URL +
 * bearer token here so the tray can read them without an IPC round trip. */
char *mailmcp_endpoint_file_path(void);

/* ${XDG_CONFIG_HOME}/mail-mcp (defaults to ~/.config/mail-mcp). */
char *mailmcp_config_dir(void);

/* ${XDG_DATA_HOME}/mail-mcp (defaults to ~/.local/share/mail-mcp). */
char *mailmcp_data_dir(void);
