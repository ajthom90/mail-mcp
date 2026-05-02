import XCTest
@testable import MailMCP

final class PathsTests: XCTestCase {
    func testWithRootSetsAllPaths() {
        let root = URL(fileURLWithPath: "/tmp/mail-mcp-test")
        let p = MailMCPPaths.withRoot(root)
        XCTAssertEqual(p.dataDir.path, "/tmp/mail-mcp-test/data")
        XCTAssertEqual(p.logsDir.path, "/tmp/mail-mcp-test/logs")
        XCTAssertEqual(p.cacheDir.path, "/tmp/mail-mcp-test/cache")
        XCTAssertEqual(p.runtimeDir.path, "/tmp/mail-mcp-test/run")
        XCTAssertEqual(p.ipcSocket.lastPathComponent, "ipc.sock")
        XCTAssertEqual(p.endpointJSON.lastPathComponent, "endpoint.json")
    }

    func testDefaultPathsAreAbsolute() {
        let p = MailMCPPaths.defaultForUser()
        XCTAssertTrue(p.dataDir.path.hasPrefix("/"))
        XCTAssertTrue(p.runtimeDir.path.contains("mail-mcp-"))
    }
}
