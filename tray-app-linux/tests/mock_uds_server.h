#pragma once
#include <gio/gio.h>
#include <gio/gunixsocketaddress.h>

/* In-process UDS server for IpcClient tests. Listens on a per-test temp
 * socket path; accepts one client; responds to each line-delimited JSON-RPC
 * request from a per-method scripted-response table. Pushes notifications
 * on demand. Runs in a thread so the test can block on the GMainContext
 * iteration and still drive the server.
 *
 * The accept + serve loop runs on its own GThread with its own GMainContext.
 * We expose a function to set scripted responses and one to push a
 * notification line.
 */
typedef struct _MailMcpMockUds MailMcpMockUds;

MailMcpMockUds *mailmcp_mock_uds_new(void);
void            mailmcp_mock_uds_free(MailMcpMockUds *);

const char *mailmcp_mock_uds_socket_path(MailMcpMockUds *);

/* Set a canned response template for the given JSON-RPC method. The template
 * is sent verbatim except `$ID` is replaced with the request id. The
 * template MUST be one line (no embedded newlines). */
void mailmcp_mock_uds_set_response(MailMcpMockUds *, const char *method, const char *response);

/* Push a notification frame (sent as-is followed by '\n'). Caller responsible
 * for valid JSON-RPC framing. */
void mailmcp_mock_uds_push(MailMcpMockUds *, const char *frame);

/* Wait for the server to accept one client and the handshake state machine
 * to be ready. Returns FALSE if accept didn't happen within `timeout_ms`. */
gboolean mailmcp_mock_uds_wait_for_client(MailMcpMockUds *, guint timeout_ms);
