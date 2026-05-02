import AppKit
import Foundation

/// Subscribes to approval.requested notifications and presents a modal NSAlert
/// for each. v0.1b uses modal-only; non-modal notifications can come later.
@MainActor
public final class ApprovalCoordinator {
    private let client: IpcClient
    private var task: Task<Void, Never>?

    public init(client: IpcClient) { self.client = client }

    public func start() {
        task = Task { [weak self] in await self?.run() }
    }

    public func stop() { task?.cancel() }

    private func run() async {
        do {
            let stream = try await client.subscribe(events: ["approval.requested"])
            for await note in stream {
                if case .approvalRequested(let p) = note {
                    await present(p)
                }
            }
        } catch {
            NSLog("ApprovalCoordinator subscription failed: \(error)")
        }
    }

    private func present(_ p: PendingApproval) async {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "\(p.summary) on \(p.account.rawValue)"
        alert.informativeText = informativeText(for: p)
        alert.addButton(withTitle: "Approve")
        alert.addButton(withTitle: "Reject")
        let resp = alert.runModal()
        let decision = resp == .alertFirstButtonReturn ? "approve" : "reject"
        do {
            let _: Empty = try await client.call(
                "approvals.decide",
                params: [
                    "id": .string(p.id),
                    "decision": .string(decision),
                ]
            )
        } catch {
            // The daemon may have already auto-resolved; harmless.
            NSLog("approvals.decide failed: \(error)")
        }
    }

    private func informativeText(for p: PendingApproval) -> String {
        switch p.category {
        case .send:
            if case .object(let obj) = p.details {
                let to = renderArray(obj["to"]) ?? "(none)"
                let subject = renderString(obj["subject"]) ?? "(no subject)"
                return "To: \(to)\nSubject: \(subject)"
            }
            return "Send approval requested."
        case .trash:
            if case .object(let obj) = p.details, case .array(let ids) = obj["message_ids"] {
                return "Move \(ids.count) message(s) to Trash."
            }
            return "Trash approval requested."
        default:
            return "Action requires your approval."
        }
    }

    private func renderArray(_ j: AnyJSON?) -> String? {
        guard case .array(let xs) = j else { return nil }
        return xs.compactMap { renderString($0) }.joined(separator: ", ")
    }

    private func renderString(_ j: AnyJSON?) -> String? {
        if case .string(let s) = j { return s }
        return nil
    }
}
