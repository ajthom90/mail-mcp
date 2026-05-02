#include "ipc_client.h"
#include <gio/gunixsocketaddress.h>
#include <string.h>

#define MAILMCP_IPC_ERROR (mailmcp_ipc_error_quark())
GQuark mailmcp_ipc_error_quark(void)
{
    return g_quark_from_static_string("mail-mcp-ipc-error");
}
typedef enum {
    MAILMCP_IPC_ERR_DISCONNECTED = 1,
    MAILMCP_IPC_ERR_RPC,
    MAILMCP_IPC_ERR_PROTOCOL,
} MailMcpIpcErr;

/* Pending RPC: callback fires once when the matching response (or error)
 * lands, or when the client disconnects. */
typedef struct {
    MailMcpIpcResponseCallback cb;
    gpointer                   user_data;
} PendingRequest;

static void pending_request_free(PendingRequest *p) { g_free(p); }

/* Per-method subscriber list. Multiple subscribers per method are allowed;
 * each subscription installs ONE entry which fires for every notification
 * with the matching method name. */
typedef struct {
    MailMcpIpcNotifCallback cb;
    gpointer                user_data;
} Subscriber;

static void subscriber_free(Subscriber *s) { g_free(s); }

struct _MailMcpIpcClient {
    char       *socket_path;
    GSocket    *socket;
    GSource    *read_source;
    GString    *read_buf;          /* accumulates bytes until we see '\n' */
    GHashTable *pending;           /* gint64 id -> PendingRequest* */
    GHashTable *subscribers;       /* method (char*) -> GList<Subscriber*> */
    gint64      next_id;
    gboolean    disconnected;
};

/* --- forward decls --- */
static gboolean on_socket_readable(GSocket *socket, GIOCondition cond, gpointer user_data);
static void     handle_frame(MailMcpIpcClient *c, const char *frame);
static void     emit_disconnected(MailMcpIpcClient *c);
static void     fail_pending(gpointer key, gpointer value, gpointer user_data);

MailMcpIpcClient *mailmcp_ipc_client_new(const char *socket_path)
{
    g_return_val_if_fail(socket_path != NULL, NULL);
    MailMcpIpcClient *c = g_new0(MailMcpIpcClient, 1);
    c->socket_path = g_strdup(socket_path);
    c->read_buf = g_string_new(NULL);
    c->pending = g_hash_table_new_full(g_int64_hash, g_int64_equal,
                                        g_free, (GDestroyNotify)pending_request_free);
    c->subscribers = g_hash_table_new_full(g_str_hash, g_str_equal,
                                            g_free, NULL /* GList freed manually */);
    c->next_id = 1;
    return c;
}

static void subscribers_table_free_lists(GHashTable *t)
{
    GHashTableIter it;
    g_hash_table_iter_init(&it, t);
    gpointer k, v;
    while (g_hash_table_iter_next(&it, &k, &v)) {
        g_list_free_full((GList *)v, (GDestroyNotify)subscriber_free);
    }
    g_hash_table_destroy(t);
}

void mailmcp_ipc_client_free(MailMcpIpcClient *c)
{
    if (!c) return;
    mailmcp_ipc_client_disconnect(c);
    if (c->read_buf) g_string_free(c->read_buf, TRUE);
    if (c->pending) g_hash_table_destroy(c->pending);
    if (c->subscribers) subscribers_table_free_lists(c->subscribers);
    g_free(c->socket_path);
    g_free(c);
}

gboolean mailmcp_ipc_client_connect(MailMcpIpcClient *c, GError **err)
{
    g_return_val_if_fail(c != NULL, FALSE);
    if (c->socket) return TRUE;

    g_autoptr(GSocket) sock = g_socket_new(G_SOCKET_FAMILY_UNIX,
                                            G_SOCKET_TYPE_STREAM,
                                            G_SOCKET_PROTOCOL_DEFAULT,
                                            err);
    if (!sock) return FALSE;
    g_socket_set_blocking(sock, FALSE);

    g_autoptr(GSocketAddress) addr = g_unix_socket_address_new(c->socket_path);
    if (!g_socket_connect(sock, addr, NULL, err)) return FALSE;

    c->socket = g_object_ref(sock);
    c->read_source = g_socket_create_source(c->socket, G_IO_IN | G_IO_HUP | G_IO_ERR, NULL);
    g_source_set_callback(c->read_source, G_SOURCE_FUNC(on_socket_readable), c, NULL);
    g_source_attach(c->read_source, g_main_context_get_thread_default());
    return TRUE;
}

void mailmcp_ipc_client_disconnect(MailMcpIpcClient *c)
{
    if (!c || c->disconnected) return;
    c->disconnected = TRUE;
    if (c->read_source) {
        g_source_destroy(c->read_source);
        g_source_unref(c->read_source);
        c->read_source = NULL;
    }
    if (c->socket) {
        g_socket_close(c->socket, NULL);
        g_object_unref(c->socket);
        c->socket = NULL;
    }
    g_hash_table_foreach(c->pending, fail_pending, NULL);
    g_hash_table_remove_all(c->pending);
    /* Subscribers stay registered (they get nothing more, but freeing them
     * here would risk firing under us if a frame is mid-dispatch). The
     * client_free path tears them down. */
}

static void fail_pending(gpointer key, gpointer value, gpointer user_data)
{
    (void)key; (void)user_data;
    PendingRequest *p = value;
    g_autoptr(GError) err = g_error_new(MAILMCP_IPC_ERROR,
                                         MAILMCP_IPC_ERR_DISCONNECTED,
                                         "daemon disconnected");
    p->cb(NULL, err, p->user_data);
}

/* --- I/O loop --- */

static gboolean on_socket_readable(GSocket *socket, GIOCondition cond, gpointer user_data)
{
    MailMcpIpcClient *c = user_data;
    if (cond & (G_IO_HUP | G_IO_ERR)) {
        emit_disconnected(c);
        return G_SOURCE_REMOVE;
    }

    char buf[4096];
    GError *err = NULL;
    gssize n = g_socket_receive(socket, buf, sizeof buf, NULL, &err);
    if (n <= 0) {
        if (err) g_error_free(err);
        emit_disconnected(c);
        return G_SOURCE_REMOVE;
    }

    g_string_append_len(c->read_buf, buf, n);
    char *nl;
    while ((nl = memchr(c->read_buf->str, '\n', c->read_buf->len)) != NULL) {
        gsize line_len = nl - c->read_buf->str;
        g_autofree char *frame = g_strndup(c->read_buf->str, line_len);
        g_string_erase(c->read_buf, 0, line_len + 1);
        if (frame[0]) handle_frame(c, frame);
    }
    return G_SOURCE_CONTINUE;
}

static void emit_disconnected(MailMcpIpcClient *c)
{
    if (c->disconnected) return;
    c->disconnected = TRUE;
    g_hash_table_foreach(c->pending, fail_pending, NULL);
    g_hash_table_remove_all(c->pending);
}

/* Routes one parsed JSON-RPC frame to either a pending request or the
 * subscriber table. Frames that match neither are dropped silently — the
 * daemon shouldn't send them. */
static void handle_frame(MailMcpIpcClient *c, const char *frame)
{
    g_autoptr(JsonParser) parser = json_parser_new();
    if (!json_parser_load_from_data(parser, frame, -1, NULL)) return;
    JsonNode *root = json_parser_get_root(parser);
    if (!root || JSON_NODE_TYPE(root) != JSON_NODE_OBJECT) return;
    JsonObject *obj = json_node_get_object(root);

    /* Response: has numeric `id` matching a pending request. */
    if (json_object_has_member(obj, "id")) {
        JsonNode *idn = json_object_get_member(obj, "id");
        if (json_node_get_value_type(idn) == G_TYPE_INT64) {
            gint64 id = json_node_get_int(idn);
            PendingRequest *p = g_hash_table_lookup(c->pending, &id);
            if (p) {
                if (json_object_has_member(obj, "error")) {
                    JsonNode *errn = json_object_get_member(obj, "error");
                    const char *msg = "rpc error";
                    int code = -32000;
                    if (JSON_NODE_TYPE(errn) == JSON_NODE_OBJECT) {
                        JsonObject *eo = json_node_get_object(errn);
                        if (json_object_has_member(eo, "message"))
                            msg = json_object_get_string_member(eo, "message");
                        if (json_object_has_member(eo, "code"))
                            code = (int)json_object_get_int_member(eo, "code");
                    }
                    g_autoptr(GError) err = g_error_new(MAILMCP_IPC_ERROR,
                                                        MAILMCP_IPC_ERR_RPC,
                                                        "%s (code %d)", msg, code);
                    p->cb(NULL, err, p->user_data);
                } else if (json_object_has_member(obj, "result")) {
                    JsonNode *res = json_object_get_member(obj, "result");
                    p->cb(res, NULL, p->user_data);
                } else {
                    g_autoptr(GError) err = g_error_new(MAILMCP_IPC_ERROR,
                                                        MAILMCP_IPC_ERR_PROTOCOL,
                                                        "response missing result/error");
                    p->cb(NULL, err, p->user_data);
                }
                g_hash_table_remove(c->pending, &id);
                return;
            }
        }
    }

    /* Notification: dispatch by method to all registered subscribers. */
    if (json_object_has_member(obj, "method")) {
        const char *method = json_object_get_string_member(obj, "method");
        if (!method) return;
        JsonNode *params = json_object_has_member(obj, "params")
                         ? json_object_get_member(obj, "params") : NULL;
        GList *subs = g_hash_table_lookup(c->subscribers, method);
        for (GList *l = subs; l; l = l->next) {
            Subscriber *s = l->data;
            s->cb(method, params, s->user_data);
        }
    }
}

/* --- send path --- */

static gboolean write_frame(MailMcpIpcClient *c, const char *frame, GError **err)
{
    if (!c->socket) {
        g_set_error(err, MAILMCP_IPC_ERROR, MAILMCP_IPC_ERR_DISCONNECTED, "not connected");
        return FALSE;
    }
    gsize remaining = strlen(frame);
    const char *p = frame;
    while (remaining > 0) {
        gssize w = g_socket_send(c->socket, p, remaining, NULL, err);
        if (w < 0) return FALSE;
        p += w;
        remaining -= (gsize)w;
    }
    /* Newline frame terminator. */
    return g_socket_send(c->socket, "\n", 1, NULL, err) >= 0;
}

void mailmcp_ipc_call_async(MailMcpIpcClient          *c,
                            const char                *method,
                            JsonNode                  *params,
                            MailMcpIpcResponseCallback cb,
                            gpointer                   user_data)
{
    g_return_if_fail(c != NULL && method != NULL && cb != NULL);

    if (c->disconnected) {
        g_autoptr(GError) err = g_error_new(MAILMCP_IPC_ERROR,
                                             MAILMCP_IPC_ERR_DISCONNECTED,
                                             "client disconnected");
        cb(NULL, err, user_data);
        return;
    }

    gint64 id = c->next_id++;

    g_autoptr(JsonBuilder) b = json_builder_new();
    json_builder_begin_object(b);
    json_builder_set_member_name(b, "jsonrpc"); json_builder_add_string_value(b, "2.0");
    json_builder_set_member_name(b, "id");      json_builder_add_int_value(b, id);
    json_builder_set_member_name(b, "method");  json_builder_add_string_value(b, method);
    json_builder_set_member_name(b, "params");
    if (params) {
        json_builder_add_value(b, json_node_copy(params));
    } else {
        json_builder_begin_object(b);
        json_builder_end_object(b);
    }
    json_builder_end_object(b);

    g_autoptr(JsonGenerator) gen = json_generator_new();
    g_autoptr(JsonNode) root = json_builder_get_root(b);
    json_generator_set_root(gen, root);
    g_autofree char *frame = json_generator_to_data(gen, NULL);

    GError *err = NULL;
    if (!write_frame(c, frame, &err)) {
        cb(NULL, err, user_data);
        g_error_free(err);
        return;
    }

    PendingRequest *pr = g_new0(PendingRequest, 1);
    pr->cb = cb;
    pr->user_data = user_data;
    gint64 *id_key = g_new(gint64, 1);
    *id_key = id;
    g_hash_table_insert(c->pending, id_key, pr);
}

/* --- subscribe --- */

typedef struct {
    MailMcpIpcClient           *client;
    char                      **event_methods;   /* NULL-terminated, owned */
    MailMcpIpcNotifCallback     notif_cb;
    gpointer                    notif_user_data;
    MailMcpIpcSubscribeCallback subscribed_cb;
    gpointer                    subscribed_user_data;
} SubscribeContext;

static void subscribe_context_free(SubscribeContext *ctx)
{
    if (!ctx) return;
    g_strfreev(ctx->event_methods);
    g_free(ctx);
}

static void on_subscribe_response(JsonNode *result, GError *err, gpointer user_data)
{
    SubscribeContext *ctx = user_data;
    if (err) {
        ctx->subscribed_cb(err, ctx->subscribed_user_data);
        subscribe_context_free(ctx);
        return;
    }
    (void)result;
    /* Ack received — register the per-event subscriber AFTER the ack so the
     * daemon's subscriber set is populated before any future notification is
     * routed (see issue #6). */
    for (guint i = 0; ctx->event_methods[i] != NULL; i++) {
        Subscriber *s = g_new0(Subscriber, 1);
        s->cb = ctx->notif_cb;
        s->user_data = ctx->notif_user_data;
        const char *method = ctx->event_methods[i];
        GList *existing = g_hash_table_lookup(ctx->client->subscribers, method);
        if (existing) {
            existing = g_list_append(existing, s);
            /* GHashTable doesn't update the value when key already exists for
             * insert; replace it so we own the new tail. */
            g_hash_table_steal(ctx->client->subscribers, method);
            g_hash_table_insert(ctx->client->subscribers, g_strdup(method), existing);
        } else {
            g_hash_table_insert(ctx->client->subscribers,
                                g_strdup(method),
                                g_list_append(NULL, s));
        }
    }
    ctx->subscribed_cb(NULL, ctx->subscribed_user_data);
    subscribe_context_free(ctx);
}

void mailmcp_ipc_subscribe_async(MailMcpIpcClient           *c,
                                 const char * const         *event_methods,
                                 MailMcpIpcNotifCallback     notif_cb,
                                 gpointer                    notif_user_data,
                                 MailMcpIpcSubscribeCallback subscribed_cb,
                                 gpointer                    subscribed_user_data)
{
    g_return_if_fail(c != NULL && event_methods != NULL
                     && notif_cb != NULL && subscribed_cb != NULL);

    SubscribeContext *ctx = g_new0(SubscribeContext, 1);
    ctx->client = c;
    ctx->event_methods = g_strdupv((char **)event_methods);
    ctx->notif_cb = notif_cb;
    ctx->notif_user_data = notif_user_data;
    ctx->subscribed_cb = subscribed_cb;
    ctx->subscribed_user_data = subscribed_user_data;

    g_autoptr(JsonBuilder) b = json_builder_new();
    json_builder_begin_object(b);
    json_builder_set_member_name(b, "events");
    json_builder_begin_array(b);
    for (guint i = 0; ctx->event_methods[i] != NULL; i++) {
        json_builder_add_string_value(b, ctx->event_methods[i]);
    }
    json_builder_end_array(b);
    json_builder_end_object(b);
    g_autoptr(JsonNode) params = json_builder_get_root(b);

    mailmcp_ipc_call_async(c, "subscribe", params, on_subscribe_response, ctx);
}
