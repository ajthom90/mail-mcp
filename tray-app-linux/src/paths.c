#include "paths.h"
#include <unistd.h>
#include <sys/types.h>

static char *runtime_dir(void)
{
    const char *xdg = g_getenv("XDG_RUNTIME_DIR");
    if (xdg && *xdg) return g_build_filename(xdg, "mail-mcp", NULL);
    return g_strdup_printf("/tmp/mail-mcp-%u", (unsigned)getuid());
}

char *mailmcp_ipc_socket_path(void)
{
    g_autofree char *dir = runtime_dir();
    return g_build_filename(dir, "ipc.sock", NULL);
}

char *mailmcp_endpoint_file_path(void)
{
    g_autofree char *dir = runtime_dir();
    return g_build_filename(dir, "endpoint.json", NULL);
}

char *mailmcp_config_dir(void)
{
    return g_build_filename(g_get_user_config_dir(), "mail-mcp", NULL);
}

char *mailmcp_data_dir(void)
{
    return g_build_filename(g_get_user_data_dir(), "mail-mcp", NULL);
}
