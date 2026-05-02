#pragma once
#include "models.h"

/* Tagged union mirroring DaemonNotification in mail-mcp-core. Decoders
 * return MAILMCP_NOTIF_UNKNOWN for unrecognised methods so the caller
 * can route forward-compatibly. */

typedef enum {
    MAILMCP_NOTIF_APPROVAL_REQUESTED,
    MAILMCP_NOTIF_APPROVAL_RESOLVED,
    MAILMCP_NOTIF_ACCOUNT_ADDED,
    MAILMCP_NOTIF_ACCOUNT_REMOVED,
    MAILMCP_NOTIF_ACCOUNT_NEEDS_REAUTH,
    MAILMCP_NOTIF_MCP_PAUSED_CHANGED,
    MAILMCP_NOTIF_UNKNOWN,
} MailMcpNotifKind;

typedef struct {
    MailMcpNotifKind kind;
    /* Only the field for the active variant is set. All owned. */
    MailMcpPendingApproval *approval;
    char                   *account_id;     /* removed / needs_reauth */
    JsonNode               *account_payload; /* added — full payload */
    char                   *resolved_id;
    char                   *resolved_decision;
    gboolean                paused;
    char                   *unknown_method;
    JsonNode               *unknown_params;
} MailMcpNotif;

/* Decode a JSON-RPC notification frame's `method` + `params` payload.
 * Returns NULL only if `method` is NULL. Otherwise returns a heap struct;
 * unrecognised methods produce a MAILMCP_NOTIF_UNKNOWN with the method
 * string + params copied for forward-compat. */
MailMcpNotif *mailmcp_notif_new_from_jsonrpc(const char *method, JsonNode *params);

void          mailmcp_notif_free(MailMcpNotif *);
G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpNotif, mailmcp_notif_free)
