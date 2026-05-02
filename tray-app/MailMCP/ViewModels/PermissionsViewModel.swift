import Foundation
import Observation

@MainActor
@Observable
public final class PermissionsViewModel {
    public private(set) var permissions: PermissionMap?
    public private(set) var lastError: String?
    public var selectedAccountId: AccountID? {
        didSet { Task { await refresh() } }
    }

    private let client: IpcClient
    public init(client: IpcClient) { self.client = client }

    public func refresh() async {
        guard let id = selectedAccountId else { permissions = nil; return }
        do {
            permissions = try await client.call(
                "permissions.get",
                params: ["account_id": .string(id.rawValue)]
            )
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    public func set(category: Category, policy: Policy) async {
        guard let id = selectedAccountId else { return }
        do {
            let _: Empty = try await client.call(
                "permissions.set",
                params: [
                    "account_id": .string(id.rawValue),
                    "category": .string(category.rawValue),
                    "policy": .string(policy.rawValue),
                ]
            )
            await refresh()
        } catch {
            lastError = error.localizedDescription
        }
    }
}
