#include "notifications.h"

static const char *params_string_member(JsonNode *params, const char *key)
{
    if (!params || JSON_NODE_TYPE(params) != JSON_NODE_OBJECT) return NULL;
    JsonObject *o = json_node_get_object(params);
    if (!json_object_has_member(o, key)) return NULL;
    JsonNode *n = json_object_get_member(o, key);
    if (json_node_get_value_type(n) != G_TYPE_STRING) return NULL;
    return json_node_get_string(n);
}

static gboolean params_bool_member(JsonNode *params, const char *key, gboolean *out)
{
    if (!params || JSON_NODE_TYPE(params) != JSON_NODE_OBJECT) return FALSE;
    JsonObject *o = json_node_get_object(params);
    if (!json_object_has_member(o, key)) return FALSE;
    *out = json_object_get_boolean_member(o, key);
    return TRUE;
}

MailMcpNotif *mailmcp_notif_new_from_jsonrpc(const char *method, JsonNode *params)
{
    if (!method) return NULL;
    MailMcpNotif *n = g_new0(MailMcpNotif, 1);

    if (g_strcmp0(method, "approval.requested") == 0) {
        n->kind = MAILMCP_NOTIF_APPROVAL_REQUESTED;
        n->approval = mailmcp_pending_approval_new_from_json(params);
        if (!n->approval) {
            mailmcp_notif_free(n);
            return NULL;
        }
        return n;
    }

    if (g_strcmp0(method, "approval.resolved") == 0) {
        const char *id = params_string_member(params, "id");
        const char *decision = params_string_member(params, "decision");
        if (!id || !decision) {
            mailmcp_notif_free(n);
            return NULL;
        }
        n->kind = MAILMCP_NOTIF_APPROVAL_RESOLVED;
        n->resolved_id = g_strdup(id);
        n->resolved_decision = g_strdup(decision);
        return n;
    }

    if (g_strcmp0(method, "account.added") == 0) {
        n->kind = MAILMCP_NOTIF_ACCOUNT_ADDED;
        n->account_payload = params ? json_node_copy(params) : NULL;
        return n;
    }

    if (g_strcmp0(method, "account.removed") == 0) {
        const char *id = params_string_member(params, "account_id");
        if (!id) {
            mailmcp_notif_free(n);
            return NULL;
        }
        n->kind = MAILMCP_NOTIF_ACCOUNT_REMOVED;
        n->account_id = g_strdup(id);
        return n;
    }

    if (g_strcmp0(method, "account.needs_reauth") == 0) {
        const char *id = params_string_member(params, "account_id");
        if (!id) {
            mailmcp_notif_free(n);
            return NULL;
        }
        n->kind = MAILMCP_NOTIF_ACCOUNT_NEEDS_REAUTH;
        n->account_id = g_strdup(id);
        return n;
    }

    if (g_strcmp0(method, "mcp.paused_changed") == 0) {
        gboolean paused;
        if (!params_bool_member(params, "paused", &paused)) {
            mailmcp_notif_free(n);
            return NULL;
        }
        n->kind = MAILMCP_NOTIF_MCP_PAUSED_CHANGED;
        n->paused = paused;
        return n;
    }

    n->kind = MAILMCP_NOTIF_UNKNOWN;
    n->unknown_method = g_strdup(method);
    n->unknown_params = params ? json_node_copy(params) : NULL;
    return n;
}

void mailmcp_notif_free(MailMcpNotif *n)
{
    if (!n) return;
    if (n->approval)        mailmcp_pending_approval_free(n->approval);
    g_free(n->account_id);
    if (n->account_payload) json_node_unref(n->account_payload);
    g_free(n->resolved_id);
    g_free(n->resolved_decision);
    g_free(n->unknown_method);
    if (n->unknown_params)  json_node_unref(n->unknown_params);
    g_free(n);
}
