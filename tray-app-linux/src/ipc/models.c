#include "models.h"

/* ------------- enum mappings ------------- */

static const char *POLICY_STR[] = {
    [MAILMCP_POLICY_ALLOW]    = "allow",
    [MAILMCP_POLICY_CONFIRM]  = "confirm",
    [MAILMCP_POLICY_SESSION]  = "session",
    [MAILMCP_POLICY_DRAFTIFY] = "draftify",
    [MAILMCP_POLICY_BLOCK]    = "block",
};
#define POLICY_COUNT G_N_ELEMENTS(POLICY_STR)

const char *mailmcp_policy_to_str(MailMcpPolicy p)
{
    return ((guint)p < POLICY_COUNT) ? POLICY_STR[p] : "";
}

gint mailmcp_policy_from_str(const char *s)
{
    if (!s) return -1;
    for (guint i = 0; i < POLICY_COUNT; i++) {
        if (g_strcmp0(POLICY_STR[i], s) == 0) return (gint)i;
    }
    return -1;
}

static const char *CATEGORY_STR[] = {
    [MAILMCP_CATEGORY_READ]   = "read",
    [MAILMCP_CATEGORY_MODIFY] = "modify",
    [MAILMCP_CATEGORY_TRASH]  = "trash",
    [MAILMCP_CATEGORY_DRAFT]  = "draft",
    [MAILMCP_CATEGORY_SEND]   = "send",
};
#define CATEGORY_COUNT G_N_ELEMENTS(CATEGORY_STR)

const char *mailmcp_category_to_str(MailMcpCategory c)
{
    return ((guint)c < CATEGORY_COUNT) ? CATEGORY_STR[c] : "";
}

gint mailmcp_category_from_str(const char *s)
{
    if (!s) return -1;
    for (guint i = 0; i < CATEGORY_COUNT; i++) {
        if (g_strcmp0(CATEGORY_STR[i], s) == 0) return (gint)i;
    }
    return -1;
}

static const char *STATUS_STR[] = {
    [MAILMCP_ACCOUNT_STATUS_OK]            = "ok",
    [MAILMCP_ACCOUNT_STATUS_NEEDS_REAUTH]  = "needs_reauth",
    [MAILMCP_ACCOUNT_STATUS_NETWORK_ERROR] = "network_error",
};
#define STATUS_COUNT G_N_ELEMENTS(STATUS_STR)

const char *mailmcp_account_status_to_str(MailMcpAccountStatus s)
{
    return ((guint)s < STATUS_COUNT) ? STATUS_STR[s] : "";
}

gint mailmcp_account_status_from_str(const char *s)
{
    if (!s) return -1;
    for (guint i = 0; i < STATUS_COUNT; i++) {
        if (g_strcmp0(STATUS_STR[i], s) == 0) return (gint)i;
    }
    return -1;
}

/* ------------- helpers ------------- */

static JsonObject *as_object(JsonNode *node)
{
    if (!node || JSON_NODE_TYPE(node) != JSON_NODE_OBJECT) return NULL;
    return json_node_get_object(node);
}

/* Returns NULL when the member is missing or not a string. The string is
 * borrowed from the JsonObject — caller must g_strdup before freeing the node. */
static const char *get_string_or_null(JsonObject *o, const char *key)
{
    if (!json_object_has_member(o, key)) return NULL;
    JsonNode *n = json_object_get_member(o, key);
    if (json_node_get_value_type(n) != G_TYPE_STRING) return NULL;
    return json_node_get_string(n);
}

/* ------------- AccountListItem ------------- */

MailMcpAccountListItem *mailmcp_account_list_item_new_from_json(JsonNode *node)
{
    JsonObject *o = as_object(node);
    if (!o) return NULL;
    const char *id = get_string_or_null(o, "id");
    const char *label = get_string_or_null(o, "label");
    const char *provider = get_string_or_null(o, "provider");
    const char *email = get_string_or_null(o, "email");
    const char *status = get_string_or_null(o, "status");
    if (!id || !label || !provider || !email || !status) return NULL;
    gint s = mailmcp_account_status_from_str(status);
    if (s < 0) return NULL;

    MailMcpAccountListItem *out = g_new0(MailMcpAccountListItem, 1);
    out->id = g_strdup(id);
    out->label = g_strdup(label);
    out->provider = g_strdup(provider);
    out->email = g_strdup(email);
    out->status = (MailMcpAccountStatus)s;
    return out;
}

void mailmcp_account_list_item_free(MailMcpAccountListItem *a)
{
    if (!a) return;
    g_free(a->id);
    g_free(a->label);
    g_free(a->provider);
    g_free(a->email);
    g_free(a);
}

/* ------------- DaemonStatus ------------- */

MailMcpDaemonStatus *mailmcp_daemon_status_new_from_json(JsonNode *node)
{
    JsonObject *o = as_object(node);
    if (!o) return NULL;
    const char *version = get_string_or_null(o, "version");
    if (!version) return NULL;
    if (!json_object_has_member(o, "uptime_secs")
        || !json_object_has_member(o, "account_count")
        || !json_object_has_member(o, "mcp_paused")
        || !json_object_has_member(o, "onboarding_complete")) {
        return NULL;
    }

    MailMcpDaemonStatus *out = g_new0(MailMcpDaemonStatus, 1);
    out->version = g_strdup(version);
    out->uptime_secs = (guint64)json_object_get_int_member(o, "uptime_secs");
    out->account_count = (guint)json_object_get_int_member(o, "account_count");
    out->mcp_paused = json_object_get_boolean_member(o, "mcp_paused");
    out->onboarding_complete = json_object_get_boolean_member(o, "onboarding_complete");
    return out;
}

void mailmcp_daemon_status_free(MailMcpDaemonStatus *s)
{
    if (!s) return;
    g_free(s->version);
    g_free(s);
}

/* ------------- PermissionMap ------------- */

static gboolean parse_policy_member(JsonObject *o, const char *key, MailMcpPolicy *out)
{
    const char *s = get_string_or_null(o, key);
    if (!s) return FALSE;
    gint p = mailmcp_policy_from_str(s);
    if (p < 0) return FALSE;
    *out = (MailMcpPolicy)p;
    return TRUE;
}

MailMcpPermissionMap *mailmcp_permission_map_new_from_json(JsonNode *node)
{
    JsonObject *o = as_object(node);
    if (!o) return NULL;
    MailMcpPermissionMap m = { 0 };
    if (!parse_policy_member(o, "read",   &m.read)
        || !parse_policy_member(o, "modify", &m.modify)
        || !parse_policy_member(o, "trash",  &m.trash)
        || !parse_policy_member(o, "draft",  &m.draft)
        || !parse_policy_member(o, "send",   &m.send)) {
        return NULL;
    }
    MailMcpPermissionMap *out = g_new(MailMcpPermissionMap, 1);
    *out = m;
    return out;
}

void mailmcp_permission_map_free(MailMcpPermissionMap *m)
{
    g_free(m);
}

/* ------------- PendingApproval ------------- */

MailMcpPendingApproval *mailmcp_pending_approval_new_from_json(JsonNode *node)
{
    JsonObject *o = as_object(node);
    if (!o) return NULL;
    const char *id      = get_string_or_null(o, "id");
    const char *account = get_string_or_null(o, "account");
    const char *cat_str = get_string_or_null(o, "category");
    const char *summary = get_string_or_null(o, "summary");
    const char *created = get_string_or_null(o, "created_at");
    const char *expires = get_string_or_null(o, "expires_at");
    if (!id || !account || !cat_str || !summary || !created || !expires) return NULL;
    gint cat = mailmcp_category_from_str(cat_str);
    if (cat < 0) return NULL;

    MailMcpPendingApproval *out = g_new0(MailMcpPendingApproval, 1);
    out->id = g_strdup(id);
    out->account = g_strdup(account);
    out->category = (MailMcpCategory)cat;
    out->summary = g_strdup(summary);
    out->created_at = g_strdup(created);
    out->expires_at = g_strdup(expires);
    if (json_object_has_member(o, "details")) {
        out->details = json_node_copy(json_object_get_member(o, "details"));
    }
    return out;
}

void mailmcp_pending_approval_free(MailMcpPendingApproval *p)
{
    if (!p) return;
    g_free(p->id);
    g_free(p->account);
    g_free(p->summary);
    g_free(p->created_at);
    g_free(p->expires_at);
    if (p->details) json_node_unref(p->details);
    g_free(p);
}
