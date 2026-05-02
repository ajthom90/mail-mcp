#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <cmocka.h>
#include <json-glib/json-glib.h>
#include "ipc/ipc_client.h"
#include "mock_uds_server.h"

G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpMockUds, mailmcp_mock_uds_free)

/* Pumps the default main context until *flag is TRUE or `timeout_ms` passes.
 * Returns the value of *flag. */
static gboolean wait_flag(gboolean *flag, guint timeout_ms)
{
    gint64 deadline = g_get_monotonic_time() + (gint64)timeout_ms * 1000;
    while (!*flag && g_get_monotonic_time() < deadline) {
        g_main_context_iteration(NULL, FALSE);
        g_usleep(1000);
    }
    return *flag;
}

/* --- Test 1: status RPC parses result --- */

typedef struct { gboolean done; char *version; GError *err; } StatusCtx;

static void on_status_response(JsonNode *result, GError *err, gpointer user_data)
{
    StatusCtx *c = user_data;
    if (err) {
        c->err = g_error_copy(err);
    } else if (result && JSON_NODE_TYPE(result) == JSON_NODE_OBJECT) {
        JsonObject *o = json_node_get_object(result);
        if (json_object_has_member(o, "version"))
            c->version = g_strdup(json_object_get_string_member(o, "version"));
    }
    c->done = TRUE;
}

static void status_call_parses_result(void **state)
{
    (void)state;
    g_autoptr(MailMcpMockUds) srv = mailmcp_mock_uds_new();
    mailmcp_mock_uds_set_response(srv, "status",
        "{\"jsonrpc\":\"2.0\",\"id\":$ID,\"result\":{\"version\":\"0.1.0\","
        "\"uptime_secs\":1,\"account_count\":0,"
        "\"mcp_paused\":false,\"onboarding_complete\":false}}");

    g_autoptr(MailMcpIpcClient) client =
        mailmcp_ipc_client_new(mailmcp_mock_uds_socket_path(srv));
    GError *err = NULL;
    assert_true(mailmcp_ipc_client_connect(client, &err));
    assert_null(err);
    assert_true(mailmcp_mock_uds_wait_for_client(srv, 1000));

    StatusCtx ctx = { 0 };
    mailmcp_ipc_call_async(client, "status", NULL, on_status_response, &ctx);
    assert_true(wait_flag(&ctx.done, 2000));
    assert_null(ctx.err);
    assert_string_equal(ctx.version, "0.1.0");
    g_free(ctx.version);
}

/* --- Test 2: RPC error propagates --- */

typedef struct { gboolean done; GError *err; } ErrCtx;

static void on_err_response(JsonNode *result, GError *err, gpointer user_data)
{
    (void)result;
    ErrCtx *c = user_data;
    c->err = err ? g_error_copy(err) : NULL;
    c->done = TRUE;
}

static void rpc_error_propagates(void **state)
{
    (void)state;
    g_autoptr(MailMcpMockUds) srv = mailmcp_mock_uds_new();
    mailmcp_mock_uds_set_response(srv, "boom",
        "{\"jsonrpc\":\"2.0\",\"id\":$ID,"
        "\"error\":{\"code\":-32000,\"message\":\"explosion\"}}");

    g_autoptr(MailMcpIpcClient) client =
        mailmcp_ipc_client_new(mailmcp_mock_uds_socket_path(srv));
    assert_true(mailmcp_ipc_client_connect(client, NULL));
    assert_true(mailmcp_mock_uds_wait_for_client(srv, 1000));

    ErrCtx ctx = { 0 };
    mailmcp_ipc_call_async(client, "boom", NULL, on_err_response, &ctx);
    assert_true(wait_flag(&ctx.done, 2000));
    assert_non_null(ctx.err);
    assert_non_null(strstr(ctx.err->message, "explosion"));
    g_error_free(ctx.err);
}

/* --- Test 3: subscribe receives notification (issue-#6 race closed) --- */

typedef struct { gboolean subscribed; gboolean got_notif; char *method; gboolean paused; } SubCtx;

static void on_subscribe(GError *err, gpointer user_data)
{
    SubCtx *c = user_data;
    assert_null(err);
    c->subscribed = TRUE;
}

static void on_notif(const char *method, JsonNode *params, gpointer user_data)
{
    SubCtx *c = user_data;
    if (g_strcmp0(method, "mcp.paused_changed") == 0
        && params && JSON_NODE_TYPE(params) == JSON_NODE_OBJECT) {
        JsonObject *o = json_node_get_object(params);
        if (json_object_has_member(o, "paused"))
            c->paused = json_object_get_boolean_member(o, "paused");
    }
    c->method = g_strdup(method);
    c->got_notif = TRUE;
}

static void subscribe_receives_notification(void **state)
{
    (void)state;
    g_autoptr(MailMcpMockUds) srv = mailmcp_mock_uds_new();
    mailmcp_mock_uds_set_response(srv, "subscribe",
        "{\"jsonrpc\":\"2.0\",\"id\":$ID,\"result\":{\"subscribed\":[]}}");

    g_autoptr(MailMcpIpcClient) client =
        mailmcp_ipc_client_new(mailmcp_mock_uds_socket_path(srv));
    assert_true(mailmcp_ipc_client_connect(client, NULL));
    assert_true(mailmcp_mock_uds_wait_for_client(srv, 1000));

    SubCtx ctx = { 0 };
    const char *events[] = { "mcp.paused_changed", NULL };
    mailmcp_ipc_subscribe_async(client, events, on_notif, &ctx, on_subscribe, &ctx);

    /* Wait for the subscribe ack to land + register the per-event subscriber. */
    assert_true(wait_flag(&ctx.subscribed, 2000));

    /* Now push the notification. It must arrive BECAUSE the subscriber is
     * registered; if the issue-#6 race re-opened, this would silently drop. */
    mailmcp_mock_uds_push(srv,
        "{\"jsonrpc\":\"2.0\",\"method\":\"mcp.paused_changed\","
        "\"params\":{\"paused\":true}}");

    assert_true(wait_flag(&ctx.got_notif, 2000));
    assert_string_equal(ctx.method, "mcp.paused_changed");
    assert_true(ctx.paused);
    g_free(ctx.method);
}

int main(void)
{
    const struct CMUnitTest tests[] = {
        cmocka_unit_test(status_call_parses_result),
        cmocka_unit_test(rpc_error_propagates),
        cmocka_unit_test(subscribe_receives_notification),
    };
    return cmocka_run_group_tests(tests, NULL, NULL);
}
