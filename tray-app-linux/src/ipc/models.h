#pragma once
#include <glib.h>
#include <json-glib/json-glib.h>

/* C mirrors of the JSON-RPC types defined in mail-mcp-core::ipc::messages
 * and ::permissions. Field names follow the wire format (snake_case). */

typedef enum {
    MAILMCP_POLICY_ALLOW,
    MAILMCP_POLICY_CONFIRM,
    MAILMCP_POLICY_SESSION,
    MAILMCP_POLICY_DRAFTIFY,
    MAILMCP_POLICY_BLOCK,
} MailMcpPolicy;

typedef enum {
    MAILMCP_CATEGORY_READ,
    MAILMCP_CATEGORY_MODIFY,
    MAILMCP_CATEGORY_TRASH,
    MAILMCP_CATEGORY_DRAFT,
    MAILMCP_CATEGORY_SEND,
} MailMcpCategory;

typedef enum {
    MAILMCP_ACCOUNT_STATUS_OK,
    MAILMCP_ACCOUNT_STATUS_NEEDS_REAUTH,
    MAILMCP_ACCOUNT_STATUS_NETWORK_ERROR,
} MailMcpAccountStatus;

typedef struct {
    char *id;
    char *label;
    char *provider;
    char *email;
    MailMcpAccountStatus status;
} MailMcpAccountListItem;

typedef struct {
    char *version;
    guint64 uptime_secs;
    guint account_count;
    gboolean mcp_paused;
    gboolean onboarding_complete;
} MailMcpDaemonStatus;

typedef struct {
    MailMcpPolicy read;
    MailMcpPolicy modify;
    MailMcpPolicy trash;
    MailMcpPolicy draft;
    MailMcpPolicy send;
} MailMcpPermissionMap;

typedef struct {
    char *id;
    char *account;
    MailMcpCategory category;
    char *summary;
    JsonNode *details;            /* owned, may be NULL */
    char *created_at;
    char *expires_at;
} MailMcpPendingApproval;

/* Constructors. Each parses a JSON object node into the struct; return NULL
 * if the node is the wrong shape (missing required field, wrong JSON kind).
 * Caller frees with the matching _free. */
MailMcpAccountListItem *mailmcp_account_list_item_new_from_json(JsonNode *node);
MailMcpDaemonStatus    *mailmcp_daemon_status_new_from_json(JsonNode *node);
MailMcpPermissionMap   *mailmcp_permission_map_new_from_json(JsonNode *node);
MailMcpPendingApproval *mailmcp_pending_approval_new_from_json(JsonNode *node);

void mailmcp_account_list_item_free(MailMcpAccountListItem *);
void mailmcp_daemon_status_free(MailMcpDaemonStatus *);
void mailmcp_permission_map_free(MailMcpPermissionMap *);
void mailmcp_pending_approval_free(MailMcpPendingApproval *);

G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpAccountListItem, mailmcp_account_list_item_free)
G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpDaemonStatus,    mailmcp_daemon_status_free)
G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpPermissionMap,   mailmcp_permission_map_free)
G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpPendingApproval, mailmcp_pending_approval_free)

/* Enum <-> wire-string conversions. The from_str variants return -1 for
 * unrecognised input so the caller can decide whether to drop the frame
 * or accept a default. */
const char    *mailmcp_policy_to_str(MailMcpPolicy);
gint           mailmcp_policy_from_str(const char *);
const char    *mailmcp_category_to_str(MailMcpCategory);
gint           mailmcp_category_from_str(const char *);
const char    *mailmcp_account_status_to_str(MailMcpAccountStatus);
gint           mailmcp_account_status_from_str(const char *);
