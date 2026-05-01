import XCTest
@testable import MailMCP

final class ModelsTests: XCTestCase {
    func testAccountListItemDecodesSnakeCase() throws {
        let json = #"""
        [{
            "id": "01H123",
            "label": "Personal",
            "provider": "gmail",
            "email": "alice@example.com",
            "status": "ok"
        }]
        """#
        let decoded = try JSONDecoder().decode(
            [AccountListItem].self,
            from: Data(json.utf8)
        )
        XCTAssertEqual(decoded.first?.email, "alice@example.com")
        XCTAssertEqual(decoded.first?.status, .ok)
    }

    func testAccountStatusNeedsReauth() throws {
        let json = "\"needs_reauth\""
        let s = try JSONDecoder().decode(AccountStatus.self, from: Data(json.utf8))
        XCTAssertEqual(s, .needsReauth)
    }

    func testNotificationApprovalRequestedDecodes() throws {
        let json = #"""
        {
            "method": "approval.requested",
            "params": {
                "id": "01H4567",
                "account": "01H123",
                "category": "send",
                "summary": "send_message",
                "details": {"to":["a@b.com"]},
                "created_at": "2026-05-01T00:00:00Z",
                "expires_at": "2026-05-01T00:05:00Z"
            }
        }
        """#
        let n = try JSONDecoder().decode(
            DaemonNotification.self,
            from: Data(json.utf8)
        )
        guard case .approvalRequested(let p) = n else {
            XCTFail("expected approvalRequested, got \(n)")
            return
        }
        XCTAssertEqual(p.id, "01H4567")
        XCTAssertEqual(p.category, .send)
    }

    func testNotificationMcpPausedChangedDecodes() throws {
        let json = #"""
        {"method":"mcp.paused_changed","params":{"paused":true}}
        """#
        let n = try JSONDecoder().decode(
            DaemonNotification.self,
            from: Data(json.utf8)
        )
        guard case .mcpPausedChanged(let paused) = n else {
            XCTFail("expected mcpPausedChanged, got \(n)")
            return
        }
        XCTAssertTrue(paused)
    }

    func testUnknownNotificationFallsBack() throws {
        let json = #"""
        {"method":"future.event","params":{"x":1}}
        """#
        let n = try JSONDecoder().decode(
            DaemonNotification.self,
            from: Data(json.utf8)
        )
        guard case .unknown(let method, _) = n else {
            XCTFail("expected unknown, got \(n)")
            return
        }
        XCTAssertEqual(method, "future.event")
    }
}
