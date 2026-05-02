#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <cmocka.h>
#include <json-glib/json-glib.h>
#include "ipc/models.h"

static JsonNode *parse(const char *src)
{
    g_autoptr(JsonParser) p = json_parser_new();
    g_assert(json_parser_load_from_data(p, src, -1, NULL));
    return json_node_copy(json_parser_get_root(p));
}

static void status_parses_full_payload(void **state)
{
    (void)state;
    g_autoptr(JsonNode) n = parse(
        "{\"version\":\"0.1.0\",\"uptime_secs\":42,"
        "\"account_count\":2,\"mcp_paused\":false,"
        "\"onboarding_complete\":true}");
    g_autoptr(MailMcpDaemonStatus) s = mailmcp_daemon_status_new_from_json(n);
    assert_non_null(s);
    assert_string_equal(s->version, "0.1.0");
    assert_int_equal(s->uptime_secs, 42);
    assert_int_equal(s->account_count, 2);
    assert_false(s->mcp_paused);
    assert_true(s->onboarding_complete);
}

static void status_returns_null_on_array(void **state)
{
    (void)state;
    g_autoptr(JsonNode) n = parse("[]");
    g_autoptr(MailMcpDaemonStatus) s = mailmcp_daemon_status_new_from_json(n);
    assert_null(s);
}

static void status_returns_null_on_missing_field(void **state)
{
    (void)state;
    /* missing onboarding_complete */
    g_autoptr(JsonNode) n = parse(
        "{\"version\":\"0.1.0\",\"uptime_secs\":1,"
        "\"account_count\":1,\"mcp_paused\":false}");
    g_autoptr(MailMcpDaemonStatus) s = mailmcp_daemon_status_new_from_json(n);
    assert_null(s);
}

static void account_list_item_parses(void **state)
{
    (void)state;
    g_autoptr(JsonNode) n = parse(
        "{\"id\":\"01H123\",\"label\":\"Work\",\"provider\":\"gmail\","
        "\"email\":\"alice@example.com\",\"status\":\"needs_reauth\"}");
    g_autoptr(MailMcpAccountListItem) a = mailmcp_account_list_item_new_from_json(n);
    assert_non_null(a);
    assert_string_equal(a->email, "alice@example.com");
    assert_int_equal(a->status, MAILMCP_ACCOUNT_STATUS_NEEDS_REAUTH);
}

static void permission_map_parses(void **state)
{
    (void)state;
    g_autoptr(JsonNode) n = parse(
        "{\"read\":\"allow\",\"modify\":\"confirm\","
        "\"trash\":\"confirm\",\"draft\":\"allow\",\"send\":\"draftify\"}");
    g_autoptr(MailMcpPermissionMap) m = mailmcp_permission_map_new_from_json(n);
    assert_non_null(m);
    assert_int_equal(m->read, MAILMCP_POLICY_ALLOW);
    assert_int_equal(m->send, MAILMCP_POLICY_DRAFTIFY);
}

static void permission_map_rejects_unknown_policy(void **state)
{
    (void)state;
    g_autoptr(JsonNode) n = parse(
        "{\"read\":\"yolo\",\"modify\":\"confirm\","
        "\"trash\":\"confirm\",\"draft\":\"allow\",\"send\":\"draftify\"}");
    g_autoptr(MailMcpPermissionMap) m = mailmcp_permission_map_new_from_json(n);
    assert_null(m);
}

static void pending_approval_parses_with_details(void **state)
{
    (void)state;
    g_autoptr(JsonNode) n = parse(
        "{\"id\":\"01H4567\",\"account\":\"01H123\",\"category\":\"send\","
        "\"summary\":\"send_message\",\"details\":{\"to\":[\"a@b.com\"]},"
        "\"created_at\":\"2026-05-01T00:00:00Z\","
        "\"expires_at\":\"2026-05-01T00:05:00Z\"}");
    g_autoptr(MailMcpPendingApproval) p = mailmcp_pending_approval_new_from_json(n);
    assert_non_null(p);
    assert_string_equal(p->id, "01H4567");
    assert_int_equal(p->category, MAILMCP_CATEGORY_SEND);
    assert_non_null(p->details);
}

static void enum_round_trips(void **state)
{
    (void)state;
    assert_int_equal(mailmcp_policy_from_str("allow"), MAILMCP_POLICY_ALLOW);
    assert_int_equal(mailmcp_policy_from_str("draftify"), MAILMCP_POLICY_DRAFTIFY);
    assert_int_equal(mailmcp_policy_from_str("nonsense"), -1);
    assert_int_equal(mailmcp_policy_from_str(NULL), -1);
    assert_string_equal(mailmcp_policy_to_str(MAILMCP_POLICY_BLOCK), "block");
}

int main(void)
{
    const struct CMUnitTest tests[] = {
        cmocka_unit_test(status_parses_full_payload),
        cmocka_unit_test(status_returns_null_on_array),
        cmocka_unit_test(status_returns_null_on_missing_field),
        cmocka_unit_test(account_list_item_parses),
        cmocka_unit_test(permission_map_parses),
        cmocka_unit_test(permission_map_rejects_unknown_policy),
        cmocka_unit_test(pending_approval_parses_with_details),
        cmocka_unit_test(enum_round_trips),
    };
    return cmocka_run_group_tests(tests, NULL, NULL);
}
