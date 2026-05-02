using System.Text.Json;
using System.Text.Json.Serialization;

namespace MailMCP.IPC;

// Mirrors of the JSON-RPC types defined in
// crates/mail-mcp-core/src/ipc/messages.rs and
// crates/mail-mcp-core/src/permissions/mod.rs.
// Field names use snake_case via JsonPropertyName to match serde output.

public enum AccountStatus
{
    [JsonStringEnumMemberName("ok")] Ok,
    [JsonStringEnumMemberName("needs_reauth")] NeedsReauth,
    [JsonStringEnumMemberName("network_error")] NetworkError,
}

public sealed record AccountListItem(
    [property: JsonPropertyName("id")] string Id,
    [property: JsonPropertyName("label")] string Label,
    [property: JsonPropertyName("provider")] string Provider,
    [property: JsonPropertyName("email")] string Email,
    [property: JsonPropertyName("status")] AccountStatus Status);

public sealed record AccountAddOAuthInProgress(
    [property: JsonPropertyName("challenge_id")] string ChallengeId,
    [property: JsonPropertyName("auth_url")] string AuthUrl);

public enum Policy
{
    [JsonStringEnumMemberName("allow")] Allow,
    [JsonStringEnumMemberName("confirm")] Confirm,
    [JsonStringEnumMemberName("session")] Session,
    [JsonStringEnumMemberName("draftify")] Draftify,
    [JsonStringEnumMemberName("block")] Block,
}

public enum Category
{
    [JsonStringEnumMemberName("read")] Read,
    [JsonStringEnumMemberName("modify")] Modify,
    [JsonStringEnumMemberName("trash")] Trash,
    [JsonStringEnumMemberName("draft")] Draft,
    [JsonStringEnumMemberName("send")] Send,
}

public sealed record PermissionMap(
    [property: JsonPropertyName("read")] Policy Read,
    [property: JsonPropertyName("modify")] Policy Modify,
    [property: JsonPropertyName("trash")] Policy Trash,
    [property: JsonPropertyName("draft")] Policy Draft,
    [property: JsonPropertyName("send")] Policy Send);

public sealed record McpEndpointInfo(
    [property: JsonPropertyName("url")] string Url,
    [property: JsonPropertyName("bearer_token")] string BearerToken,
    [property: JsonPropertyName("stdio_shim_path")] string? StdioShimPath);

public sealed record DaemonStatus(
    [property: JsonPropertyName("version")] string Version,
    [property: JsonPropertyName("uptime_secs")] ulong UptimeSecs,
    [property: JsonPropertyName("account_count")] uint AccountCount,
    [property: JsonPropertyName("mcp_paused")] bool McpPaused,
    [property: JsonPropertyName("onboarding_complete")] bool OnboardingComplete);

public sealed record PendingApproval(
    [property: JsonPropertyName("id")] string Id,
    [property: JsonPropertyName("account")] string Account,
    [property: JsonPropertyName("category")] Category Category,
    [property: JsonPropertyName("summary")] string Summary,
    [property: JsonPropertyName("details")] JsonElement Details,
    [property: JsonPropertyName("created_at")] string CreatedAt,
    [property: JsonPropertyName("expires_at")] string ExpiresAt);

/// Notifications pushed from daemon → client. Mirrors the tagged Rust enum.
public abstract record DaemonNotification
{
    public sealed record ApprovalRequested(PendingApproval Approval) : DaemonNotification;
    public sealed record ApprovalResolved(string Id, string Decision) : DaemonNotification;
    public sealed record AccountAdded(JsonElement Account) : DaemonNotification;
    public sealed record AccountRemoved(string AccountId) : DaemonNotification;
    public sealed record AccountNeedsReauth(string AccountId) : DaemonNotification;
    public sealed record McpPausedChanged(bool Paused) : DaemonNotification;
    public sealed record Unknown(string Method, JsonElement Params) : DaemonNotification;

    public static DaemonNotification Decode(JsonElement frame)
    {
        var method = frame.GetProperty("method").GetString() ?? "";
        var p = frame.GetProperty("params");
        return method switch
        {
            "approval.requested" => new ApprovalRequested(
                JsonSerializer.Deserialize<PendingApproval>(p)
                    ?? throw new JsonException("malformed approval payload")),
            "approval.resolved" => new ApprovalResolved(
                p.GetProperty("id").GetString() ?? "",
                p.GetProperty("decision").GetString() ?? ""),
            "account.added" => new AccountAdded(p.Clone()),
            "account.removed" => new AccountRemoved(
                p.GetProperty("account_id").GetString() ?? ""),
            "account.needs_reauth" => new AccountNeedsReauth(
                p.GetProperty("account_id").GetString() ?? ""),
            "mcp.paused_changed" => new McpPausedChanged(
                p.GetProperty("paused").GetBoolean()),
            _ => new Unknown(method, p.Clone()),
        };
    }
}
