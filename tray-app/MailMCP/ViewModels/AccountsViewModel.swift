import Foundation
import Observation

@MainActor
@Observable
public final class AccountsViewModel {
    public private(set) var accounts: [AccountListItem] = []
    public private(set) var isLoading = false
    public private(set) var lastError: String?

    private let client: IpcClient
    private var notifTask: Task<Void, Never>?

    public init(client: IpcClient) {
        self.client = client
    }

    public func start() {
        Task { await refresh() }
        notifTask = Task { [weak self] in await self?.notificationLoop() }
    }

    public func stop() { notifTask?.cancel() }

    public func refresh() async {
        isLoading = true
        defer { isLoading = false }
        do {
            accounts = try await client.call("accounts.list")
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    public func remove(id: AccountID) async {
        do {
            let _: Empty = try await client.call(
                "accounts.remove",
                params: ["account_id": .string(id.rawValue)]
            )
            await refresh()
        } catch {
            lastError = error.localizedDescription
        }
    }

    private func notificationLoop() async {
        do {
            let stream = try await client.subscribe(events: [
                "account.added", "account.removed", "account.needs_reauth",
            ])
            for await _ in stream { await refresh() }
        } catch {
            lastError = error.localizedDescription
        }
    }
}
