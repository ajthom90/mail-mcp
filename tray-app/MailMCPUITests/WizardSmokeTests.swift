import XCTest

final class WizardSmokeTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    func testWelcomeAndSkipSetupClosesWizard() {
        let app = XCUIApplication()
        // Point the daemon at a fresh tempdir so the wizard auto-opens.
        let tmp = FileManager.default.temporaryDirectory
            .appendingPathComponent("mailmcp-uitest-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
        app.launchEnvironment["MAIL_MCP_ROOT"] = tmp.path
        app.launch()

        // Wait for the Welcome screen.
        let welcome = app.staticTexts["Welcome to MailMCP"]
        XCTAssertTrue(welcome.waitForExistence(timeout: 15))

        // Click Skip Setup.
        let skipButton = app.buttons["Skip Setup"]
        XCTAssertTrue(skipButton.exists)
        skipButton.click()

        // The wizard window should close.
        XCTAssertFalse(welcome.waitForExistence(timeout: 3))
    }
}
