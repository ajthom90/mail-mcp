#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <cmocka.h>
#include "paths.h"

static void ipc_socket_path_uses_xdg_runtime_dir(void **state)
{
    (void)state;
    g_setenv("XDG_RUNTIME_DIR", "/run/user/1000", TRUE);
    g_autofree char *p = mailmcp_ipc_socket_path();
    assert_string_equal(p, "/run/user/1000/mail-mcp/ipc.sock");
}

static void ipc_socket_path_falls_back_to_tmp(void **state)
{
    (void)state;
    g_unsetenv("XDG_RUNTIME_DIR");
    g_autofree char *p = mailmcp_ipc_socket_path();
    assert_non_null(strstr(p, "/tmp/mail-mcp-"));
    assert_non_null(strstr(p, "/ipc.sock"));
}

static void endpoint_file_lives_next_to_socket(void **state)
{
    (void)state;
    g_setenv("XDG_RUNTIME_DIR", "/run/user/1000", TRUE);
    g_autofree char *e = mailmcp_endpoint_file_path();
    assert_string_equal(e, "/run/user/1000/mail-mcp/endpoint.json");
}

static void config_dir_uses_xdg(void **state)
{
    (void)state;
    g_setenv("XDG_CONFIG_HOME", "/tmp/cfg", TRUE);
    g_autofree char *c = mailmcp_config_dir();
    assert_string_equal(c, "/tmp/cfg/mail-mcp");
}

int main(void)
{
    const struct CMUnitTest tests[] = {
        cmocka_unit_test(ipc_socket_path_uses_xdg_runtime_dir),
        cmocka_unit_test(ipc_socket_path_falls_back_to_tmp),
        cmocka_unit_test(endpoint_file_lives_next_to_socket),
        cmocka_unit_test(config_dir_uses_xdg),
    };
    return cmocka_run_group_tests(tests, NULL, NULL);
}
