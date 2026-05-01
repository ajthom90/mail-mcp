import XCTest
@testable import MailMCP

final class IpcClientTests: XCTestCase {
    private var server: MockIpcServer!
    private var client: IpcClient!

    override func setUp() async throws {
        server = MockIpcServer()
        try server.start()
        client = IpcClient(socketPath: server.socketURL)
    }

    override func tearDown() async throws {
        await client.disconnect()
        server.stop()
    }

    func testStatusCallParsesResult() async throws {
        server.responses["status"] = """
        {"jsonrpc":"2.0","id":$ID,"result":{
            "version":"0.1.0","uptime_secs":42,
            "account_count":1,"mcp_paused":false,"onboarding_complete":true
        }}
        """
        let s: DaemonStatus = try await client.call("status")
        XCTAssertEqual(s.version, "0.1.0")
        XCTAssertEqual(s.uptimeSecs, 42)
        XCTAssertEqual(s.accountCount, 1)
        XCTAssertTrue(s.onboardingComplete)
    }

    func testRpcErrorIsThrown() async throws {
        server.responses["bad"] = """
        {"jsonrpc":"2.0","id":$ID,"error":{"code":-32601,"message":"not found"}}
        """
        do {
            let _: Empty = try await client.call("bad")
            XCTFail("expected throw")
        } catch IpcClient.IpcError.rpcError(let code, let message) {
            XCTAssertEqual(code, -32601)
            XCTAssertEqual(message, "not found")
        }
    }

    func testSubscribeReceivesNotification() async throws {
        // No canned response for subscribe; default-handler in MockIpcServer
        // returns subscribed:[] so the await unblocks.
        let stream = try await client.subscribe(events: ["mcp.paused_changed"])
        let frame = """
        {"jsonrpc":"2.0","method":"mcp.paused_changed","params":{"paused":true}}
        """
        server.push(notification: frame)
        guard let n = try await Self.withTimeout(seconds: 2, {
            await Self.firstNotification(from: stream)
        }) else {
            XCTFail("notification never arrived")
            return
        }
        guard case .mcpPausedChanged(let paused) = n else {
            XCTFail("wrong notification")
            return
        }
        XCTAssertTrue(paused)
    }

    /// Reads the first element of an AsyncStream without leaking a `var` iterator
    /// across concurrency boundaries.
    private static func firstNotification(
        from stream: AsyncStream<DaemonNotification>
    ) async -> DaemonNotification? {
        for await n in stream { return n }
        return nil
    }

    /// Helper — fail the test if the body doesn't return within `seconds`.
    private static func withTimeout<R: Sendable>(
        seconds: TimeInterval,
        _ body: @Sendable @escaping () async -> R?
    ) async throws -> R? {
        try await withThrowingTaskGroup(of: R?.self) { group in
            group.addTask { await body() }
            group.addTask {
                try await Task.sleep(for: .seconds(seconds))
                return nil
            }
            let first = try await group.next()
            group.cancelAll()
            return first ?? nil
        }
    }
}
