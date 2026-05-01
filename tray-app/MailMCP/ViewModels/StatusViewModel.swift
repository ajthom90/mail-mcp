import Foundation
import Observation

@MainActor
@Observable
public final class StatusViewModel {
    public private(set) var status: DaemonStatus?
    public private(set) var pendingApprovalCount: Int = 0
    public private(set) var lastError: String?

    private let client: IpcClient
    private var pollTask: Task<Void, Never>?
    private var notifTask: Task<Void, Never>?

    public init(client: IpcClient) {
        self.client = client
    }

    public func start() {
        pollTask = Task { [weak self] in await self?.pollLoop() }
        notifTask = Task { [weak self] in await self?.notificationLoop() }
    }

    public func stop() {
        pollTask?.cancel()
        notifTask?.cancel()
    }

    public func refresh() async {
        do {
            status = try await client.call("status")
            let approvals: [PendingApproval] = try await client.call("approvals.list")
            pendingApprovalCount = approvals.count
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    private func pollLoop() async {
        while !Task.isCancelled {
            await refresh()
            try? await Task.sleep(for: .seconds(30))
        }
    }

    private func notificationLoop() async {
        do {
            let stream = try await client.subscribe(events: [
                "approval.requested",
                "approval.resolved",
                "account.added",
                "account.removed",
                "mcp.paused_changed",
            ])
            for await note in stream {
                switch note {
                case .approvalRequested:
                    pendingApprovalCount += 1
                case .approvalResolved:
                    pendingApprovalCount = max(0, pendingApprovalCount - 1)
                case .accountAdded, .accountRemoved, .mcpPausedChanged:
                    await refresh()
                default:
                    break
                }
            }
        } catch {
            lastError = error.localizedDescription
        }
    }
}
