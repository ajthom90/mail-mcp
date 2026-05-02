import Foundation

// Mirrors of the JSON-RPC types defined in
// crates/mail-mcp-core/src/ipc/messages.rs and
// crates/mail-mcp-core/src/permissions/mod.rs.
// Field names use snake_case via CodingKeys to match serde output.

public struct AccountID: Codable, Hashable, Sendable, RawRepresentable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public init(from decoder: Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        try c.encode(rawValue)
    }
}

public enum AccountStatus: String, Codable, Sendable {
    case ok
    case needsReauth = "needs_reauth"
    case networkError = "network_error"
}

public struct AccountListItem: Codable, Identifiable, Hashable, Sendable {
    public let id: AccountID
    public let label: String
    public let provider: String
    public let email: String
    public let status: AccountStatus
}

public struct AccountAddOAuthInProgress: Codable, Sendable {
    public let challengeId: String
    public let authUrl: String
    enum CodingKeys: String, CodingKey {
        case challengeId = "challenge_id"
        case authUrl = "auth_url"
    }
}

public enum Policy: String, Codable, Sendable {
    case allow
    case confirm
    case session
    case draftify
    case block
}

public enum Category: String, Codable, Sendable {
    case read, modify, trash, draft, send
}

public struct PermissionMap: Codable, Sendable {
    public var read: Policy
    public var modify: Policy
    public var trash: Policy
    public var draft: Policy
    public var send: Policy
}

public struct McpEndpointInfo: Codable, Sendable {
    public let url: String
    public let bearerToken: String
    public let stdioShimPath: String?
    enum CodingKeys: String, CodingKey {
        case url
        case bearerToken = "bearer_token"
        case stdioShimPath = "stdio_shim_path"
    }
}

public struct DaemonStatus: Codable, Sendable {
    public let version: String
    public let uptimeSecs: UInt64
    public let accountCount: UInt32
    public let mcpPaused: Bool
    public let onboardingComplete: Bool
    enum CodingKeys: String, CodingKey {
        case version
        case uptimeSecs = "uptime_secs"
        case accountCount = "account_count"
        case mcpPaused = "mcp_paused"
        case onboardingComplete = "onboarding_complete"
    }
}

public struct PendingApproval: Codable, Identifiable, Sendable {
    public let id: String
    public let account: AccountID
    public let category: Category
    public let summary: String
    public let details: AnyJSON
    public let createdAt: String
    public let expiresAt: String
    enum CodingKeys: String, CodingKey {
        case id, account, category, summary, details
        case createdAt = "created_at"
        case expiresAt = "expires_at"
    }
}

/// Notifications pushed from daemon → client. Mirrors the tagged Rust enum.
public enum DaemonNotification: Sendable {
    case approvalRequested(PendingApproval)
    case approvalResolved(id: String, decision: String)
    case accountAdded(AnyJSON)        // full Account record; structure not needed in Phase A
    case accountRemoved(AccountID)
    case accountNeedsReauth(AccountID)
    case mcpPausedChanged(Bool)
    case unknown(method: String, params: AnyJSON)
}

extension DaemonNotification: Decodable {
    private enum CodingKeys: String, CodingKey { case method, params }
    private enum ResolvedKeys: String, CodingKey { case id, decision }
    private enum AccountIdKeys: String, CodingKey { case accountId = "account_id" }
    private enum PausedKeys: String, CodingKey { case paused }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let method = try c.decode(String.self, forKey: .method)
        switch method {
        case "approval.requested":
            self = .approvalRequested(try c.decode(PendingApproval.self, forKey: .params))
        case "approval.resolved":
            let p = try c.nestedContainer(keyedBy: ResolvedKeys.self, forKey: .params)
            self = .approvalResolved(
                id: try p.decode(String.self, forKey: .id),
                decision: try p.decode(String.self, forKey: .decision)
            )
        case "account.added":
            self = .accountAdded(try c.decode(AnyJSON.self, forKey: .params))
        case "account.removed":
            let p = try c.nestedContainer(keyedBy: AccountIdKeys.self, forKey: .params)
            self = .accountRemoved(try p.decode(AccountID.self, forKey: .accountId))
        case "account.needs_reauth":
            let p = try c.nestedContainer(keyedBy: AccountIdKeys.self, forKey: .params)
            self = .accountNeedsReauth(try p.decode(AccountID.self, forKey: .accountId))
        case "mcp.paused_changed":
            let p = try c.nestedContainer(keyedBy: PausedKeys.self, forKey: .params)
            self = .mcpPausedChanged(try p.decode(Bool.self, forKey: .paused))
        default:
            self = .unknown(
                method: method,
                params: (try? c.decode(AnyJSON.self, forKey: .params)) ?? .null
            )
        }
    }
}

/// Type-erased JSON value, for fields where we don't need a strong type.
public enum AnyJSON: Codable, Sendable {
    case null
    case bool(Bool)
    case int(Int64)
    case double(Double)
    case string(String)
    case array([AnyJSON])
    case object([String: AnyJSON])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null; return }
        if let b = try? c.decode(Bool.self) { self = .bool(b); return }
        if let i = try? c.decode(Int64.self) { self = .int(i); return }
        if let d = try? c.decode(Double.self) { self = .double(d); return }
        if let s = try? c.decode(String.self) { self = .string(s); return }
        if let a = try? c.decode([AnyJSON].self) { self = .array(a); return }
        if let o = try? c.decode([String: AnyJSON].self) { self = .object(o); return }
        throw DecodingError.dataCorruptedError(in: c, debugDescription: "unknown JSON shape")
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .int(let i): try c.encode(i)
        case .double(let d): try c.encode(d)
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }
}
