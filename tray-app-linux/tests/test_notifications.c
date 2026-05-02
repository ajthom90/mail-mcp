#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <cmocka.h>
#include <json-glib/json-glib.h>
#include "ipc/notifications.h"

static JsonNode *parse(const char *src)
{
    g_autoptr(JsonParser) p = json_parser_new();
    g_assert(json_parser_load_from_data(p, src, -1, NULL));
    return json_node_copy(json_parser_get_root(p));
}

static void approval_requested_decodes(void **state)
{
    (void)state;
    g_autoptr(JsonNode) params = parse(
        "{\"id\":\"01H4567\",\"account\":\"01H123\",\"category\":\"send\","
        "\"summary\":\"send_message\",\"details\":{\"to\":[\"a@b.com\"]},"
        "\"created_at\":\"2026-05-01T00:00:00Z\","
        "\"expires_at\":\"2026-05-01T00:05:00Z\"}");
    g_autoptr(MailMcpNotif) n = mailmcp_notif_new_from_jsonrpc("approval.requested", params);
    assert_non_null(n);
    assert_int_equal(n->kind, MAILMCP_NOTIF_APPROVAL_REQUESTED);
    assert_non_null(n->approval);
    assert_string_equal(n->approval->id, "01H4567");
}

static void approval_resolved_decodes(void **state)
{
    (void)state;
    g_autoptr(JsonNode) params = parse("{\"id\":\"01H4567\",\"decision\":\"approve\"}");
    g_autoptr(MailMcpNotif) n = mailmcp_notif_new_from_jsonrpc("approval.resolved", params);
    assert_non_null(n);
    assert_string_equal(n->resolved_id, "01H4567");
    assert_string_equal(n->resolved_decision, "approve");
}

static void account_removed_decodes(void **state)
{
    (void)state;
    g_autoptr(JsonNode) params = parse("{\"account_id\":\"01H123\"}");
    g_autoptr(MailMcpNotif) n = mailmcp_notif_new_from_jsonrpc("account.removed", params);
    assert_non_null(n);
    assert_int_equal(n->kind, MAILMCP_NOTIF_ACCOUNT_REMOVED);
    assert_string_equal(n->account_id, "01H123");
}

static void mcp_paused_changed_decodes(void **state)
{
    (void)state;
    g_autoptr(JsonNode) params = parse("{\"paused\":true}");
    g_autoptr(MailMcpNotif) n = mailmcp_notif_new_from_jsonrpc("mcp.paused_changed", params);
    assert_non_null(n);
    assert_int_equal(n->kind, MAILMCP_NOTIF_MCP_PAUSED_CHANGED);
    assert_true(n->paused);
}

static void unknown_method_falls_back(void **state)
{
    (void)state;
    g_autoptr(JsonNode) params = parse("{\"x\":1}");
    g_autoptr(MailMcpNotif) n = mailmcp_notif_new_from_jsonrpc("future.event", params);
    assert_non_null(n);
    assert_int_equal(n->kind, MAILMCP_NOTIF_UNKNOWN);
    assert_string_equal(n->unknown_method, "future.event");
    assert_non_null(n->unknown_params);
}

static void malformed_resolved_returns_null(void **state)
{
    (void)state;
    g_autoptr(JsonNode) params = parse("{\"id\":\"only_id\"}");
    g_autoptr(MailMcpNotif) n = mailmcp_notif_new_from_jsonrpc("approval.resolved", params);
    assert_null(n);
}

int main(void)
{
    const struct CMUnitTest tests[] = {
        cmocka_unit_test(approval_requested_decodes),
        cmocka_unit_test(approval_resolved_decodes),
        cmocka_unit_test(account_removed_decodes),
        cmocka_unit_test(mcp_paused_changed_decodes),
        cmocka_unit_test(unknown_method_falls_back),
        cmocka_unit_test(malformed_resolved_returns_null),
    };
    return cmocka_run_group_tests(tests, NULL, NULL);
}
