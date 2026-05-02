#pragma once
#include <gio/gio.h>
#include <json-glib/json-glib.h>

typedef struct _MailMcpIpcClient MailMcpIpcClient;

/* Response callback. `result` is the parsed `result` JsonNode (borrowed —
 * caller must json_node_copy if it needs to outlive the callback). On RPC
 * error, `err` is non-NULL with a domain-specific message and `result` is
 * NULL. Always exactly one of result/err is set. */
typedef void (*MailMcpIpcResponseCallback)(JsonNode *result,
                                           GError   *err,
                                           gpointer  user_data);

/* Notification callback fired for every matching event after a successful
 * subscribe ack. `params` is borrowed. */
typedef void (*MailMcpIpcNotifCallback)(const char *method,
                                        JsonNode   *params,
                                        gpointer    user_data);

/* Subscribe-completion callback. `err` non-NULL on failure to subscribe;
 * notif_cb is NOT registered in that case. */
typedef void (*MailMcpIpcSubscribeCallback)(GError  *err,
                                            gpointer user_data);

MailMcpIpcClient *mailmcp_ipc_client_new(const char *socket_path);
void              mailmcp_ipc_client_free(MailMcpIpcClient *);
G_DEFINE_AUTOPTR_CLEANUP_FUNC(MailMcpIpcClient, mailmcp_ipc_client_free)

/* Sync connect to the UDS. Returns FALSE + sets *err on failure. */
gboolean mailmcp_ipc_client_connect(MailMcpIpcClient *, GError **err);

/* Close the socket. Pending requests are completed with a "disconnected"
 * GError; subscribers see no further callbacks. */
void mailmcp_ipc_client_disconnect(MailMcpIpcClient *);

/* Send a JSON-RPC request. `params` may be NULL; ownership is NOT taken
 * (caller frees). The callback fires on the GMainContext that the client
 * was created under, exactly once. */
void mailmcp_ipc_call_async(MailMcpIpcClient          *client,
                            const char                *method,
                            JsonNode                  *params,
                            MailMcpIpcResponseCallback cb,
                            gpointer                   user_data);

/* Sends `subscribe` for the given NULL-terminated method list. After the
 * ack arrives (or fails), `subscribed_cb` runs. On success, `notif_cb` is
 * registered first so it sees every notification published from the ack
 * onward — closing the issue-#6 race. */
void mailmcp_ipc_subscribe_async(MailMcpIpcClient           *client,
                                 const char * const         *event_methods,
                                 MailMcpIpcNotifCallback     notif_cb,
                                 gpointer                    notif_user_data,
                                 MailMcpIpcSubscribeCallback subscribed_cb,
                                 gpointer                    subscribed_user_data);
