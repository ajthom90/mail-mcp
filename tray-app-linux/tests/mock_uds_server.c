#include "mock_uds_server.h"
#include <glib/gstdio.h>
#include <json-glib/json-glib.h>
#include <string.h>
#include <unistd.h>

struct _MailMcpMockUds {
    char         *socket_path;
    GSocket      *listen_sock;
    GSocket      *client_sock;     /* once accepted */
    GThread      *thread;
    GMainContext *ctx;
    GMainLoop    *loop;
    GMutex        mu;
    GCond         client_cv;
    GHashTable   *responses;       /* char* method -> char* template */
    GString      *read_buf;
};

/* --- helpers running on the server thread --- */

static gboolean on_listen_readable(GSocket *socket, GIOCondition cond, gpointer user_data);
static gboolean on_client_readable(GSocket *socket, GIOCondition cond, gpointer user_data);

static void send_line(MailMcpMockUds *m, const char *frame)
{
    if (!m->client_sock) return;
    g_socket_send(m->client_sock, frame, strlen(frame), NULL, NULL);
    g_socket_send(m->client_sock, "\n", 1, NULL, NULL);
}

static void handle_request(MailMcpMockUds *m, const char *frame)
{
    g_autoptr(JsonParser) p = json_parser_new();
    if (!json_parser_load_from_data(p, frame, -1, NULL)) return;
    JsonNode *root = json_parser_get_root(p);
    if (!root || JSON_NODE_TYPE(root) != JSON_NODE_OBJECT) return;
    JsonObject *obj = json_node_get_object(root);

    if (!json_object_has_member(obj, "method") || !json_object_has_member(obj, "id"))
        return;
    const char *method = json_object_get_string_member(obj, "method");
    gint64 id = json_object_get_int_member(obj, "id");

    g_mutex_lock(&m->mu);
    char *templ = g_hash_table_lookup(m->responses, method);
    g_autofree char *response = templ ? g_strdup(templ) : NULL;
    g_mutex_unlock(&m->mu);

    if (!response) {
        /* Default ack: result {} so unknown methods don't deadlock the client. */
        g_autofree char *def = g_strdup_printf(
            "{\"jsonrpc\":\"2.0\",\"id\":%lld,\"result\":{}}", (long long)id);
        send_line(m, def);
        return;
    }

    /* Substitute $ID with the actual id. */
    g_autofree char *id_str = g_strdup_printf("%lld", (long long)id);
    g_auto(GStrv) parts = g_strsplit(response, "$ID", -1);
    g_autofree char *expanded = g_strjoinv(id_str, parts);
    send_line(m, expanded);
}

static gboolean on_client_readable(GSocket *socket, GIOCondition cond, gpointer user_data)
{
    MailMcpMockUds *m = user_data;
    if (cond & (G_IO_HUP | G_IO_ERR)) return G_SOURCE_REMOVE;
    char buf[4096];
    gssize n = g_socket_receive(socket, buf, sizeof buf, NULL, NULL);
    if (n <= 0) return G_SOURCE_REMOVE;
    g_string_append_len(m->read_buf, buf, n);
    char *nl;
    while ((nl = memchr(m->read_buf->str, '\n', m->read_buf->len)) != NULL) {
        gsize line_len = nl - m->read_buf->str;
        g_autofree char *frame = g_strndup(m->read_buf->str, line_len);
        g_string_erase(m->read_buf, 0, line_len + 1);
        if (frame[0]) handle_request(m, frame);
    }
    return G_SOURCE_CONTINUE;
}

static gboolean on_listen_readable(GSocket *socket, GIOCondition cond, gpointer user_data)
{
    (void)cond;
    MailMcpMockUds *m = user_data;
    GSocket *cli = g_socket_accept(socket, NULL, NULL);
    if (!cli) return G_SOURCE_CONTINUE;
    g_socket_set_blocking(cli, FALSE);

    g_mutex_lock(&m->mu);
    m->client_sock = cli;
    g_cond_broadcast(&m->client_cv);
    g_mutex_unlock(&m->mu);

    GSource *src = g_socket_create_source(cli, G_IO_IN | G_IO_HUP | G_IO_ERR, NULL);
    g_source_set_callback(src, G_SOURCE_FUNC(on_client_readable), m, NULL);
    g_source_attach(src, m->ctx);
    g_source_unref(src);
    return G_SOURCE_CONTINUE;
}

static gpointer server_thread_func(gpointer data)
{
    MailMcpMockUds *m = data;
    g_main_context_push_thread_default(m->ctx);
    g_main_loop_run(m->loop);
    g_main_context_pop_thread_default(m->ctx);
    return NULL;
}

/* --- public API --- */

MailMcpMockUds *mailmcp_mock_uds_new(void)
{
    MailMcpMockUds *m = g_new0(MailMcpMockUds, 1);
    g_mutex_init(&m->mu);
    g_cond_init(&m->client_cv);
    m->ctx = g_main_context_new();
    m->loop = g_main_loop_new(m->ctx, FALSE);
    m->responses = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, g_free);
    m->read_buf = g_string_new(NULL);

    /* Pick a unique socket path under /tmp. */
    g_autofree char *tmpdir = g_dir_make_tmp("mailmcp-mock-XXXXXX", NULL);
    g_assert(tmpdir);
    m->socket_path = g_build_filename(tmpdir, "ipc.sock", NULL);

    m->listen_sock = g_socket_new(G_SOCKET_FAMILY_UNIX,
                                   G_SOCKET_TYPE_STREAM,
                                   G_SOCKET_PROTOCOL_DEFAULT,
                                   NULL);
    g_assert(m->listen_sock);
    g_socket_set_blocking(m->listen_sock, FALSE);

    g_autoptr(GSocketAddress) addr = g_unix_socket_address_new(m->socket_path);
    g_socket_bind(m->listen_sock, addr, TRUE, NULL);
    g_socket_listen(m->listen_sock, NULL);

    GSource *src = g_socket_create_source(m->listen_sock, G_IO_IN, NULL);
    g_source_set_callback(src, G_SOURCE_FUNC(on_listen_readable), m, NULL);
    g_source_attach(src, m->ctx);
    g_source_unref(src);

    m->thread = g_thread_new("mailmcp-mock-uds", server_thread_func, m);
    return m;
}

void mailmcp_mock_uds_free(MailMcpMockUds *m)
{
    if (!m) return;
    g_main_loop_quit(m->loop);
    g_thread_join(m->thread);
    g_main_loop_unref(m->loop);
    g_main_context_unref(m->ctx);

    if (m->client_sock) {
        g_socket_close(m->client_sock, NULL);
        g_object_unref(m->client_sock);
    }
    g_socket_close(m->listen_sock, NULL);
    g_object_unref(m->listen_sock);

    g_unlink(m->socket_path);
    g_autofree char *parent = g_path_get_dirname(m->socket_path);
    g_rmdir(parent);
    g_free(m->socket_path);

    g_hash_table_destroy(m->responses);
    g_string_free(m->read_buf, TRUE);
    g_mutex_clear(&m->mu);
    g_cond_clear(&m->client_cv);
    g_free(m);
}

const char *mailmcp_mock_uds_socket_path(MailMcpMockUds *m) { return m->socket_path; }

void mailmcp_mock_uds_set_response(MailMcpMockUds *m, const char *method, const char *response)
{
    g_mutex_lock(&m->mu);
    g_hash_table_insert(m->responses, g_strdup(method), g_strdup(response));
    g_mutex_unlock(&m->mu);
}

/* Pushed from a foreign thread; we hop onto the server thread's main context
 * to actually emit, so the send happens serially with handle_request. */
typedef struct { MailMcpMockUds *m; char *frame; } PushCtx;

static gboolean do_push(gpointer data)
{
    PushCtx *ctx = data;
    send_line(ctx->m, ctx->frame);
    g_free(ctx->frame);
    g_free(ctx);
    return G_SOURCE_REMOVE;
}

void mailmcp_mock_uds_push(MailMcpMockUds *m, const char *frame)
{
    PushCtx *ctx = g_new0(PushCtx, 1);
    ctx->m = m;
    ctx->frame = g_strdup(frame);
    GSource *src = g_idle_source_new();
    g_source_set_callback(src, do_push, ctx, NULL);
    g_source_attach(src, m->ctx);
    g_source_unref(src);
}

gboolean mailmcp_mock_uds_wait_for_client(MailMcpMockUds *m, guint timeout_ms)
{
    g_mutex_lock(&m->mu);
    gint64 deadline = g_get_monotonic_time() + (gint64)timeout_ms * 1000;
    while (!m->client_sock) {
        if (!g_cond_wait_until(&m->client_cv, &m->mu, deadline)) {
            g_mutex_unlock(&m->mu);
            return FALSE;
        }
    }
    g_mutex_unlock(&m->mu);
    return TRUE;
}
