import AppKit
import Combine
import SwiftUI
import Observation

@MainActor
final class MenuBarController {
    private let statusItem = NSStatusBar.system.statusItem(
        withLength: NSStatusItem.variableLength
    )
    private let paths: MailMCPPaths
    private let launcher: DaemonLauncher
    private let client: IpcClient
    private let statusVM: StatusViewModel
    private var refreshTimer: Timer?
    private var observation: AnyCancellable?
    private lazy var wizard = WizardController(client: client)
    private lazy var accountsVM = AccountsViewModel(client: client)
    private var settingsWindow: NSWindow?

    init(paths: MailMCPPaths = .defaultForUser()) {
        self.paths = paths
        self.launcher = DaemonLauncher(paths: paths)
        self.client = IpcClient(socketPath: paths.ipcSocket)
        self.statusVM = StatusViewModel(client: client)
    }

    func start() {
        statusItem.button?.image = NSImage(
            systemSymbolName: "envelope.fill",
            accessibilityDescription: "MailMCP"
        )
        statusItem.button?.image?.isTemplate = true
        rebuildMenu()

        Task {
            do {
                try await launcher.ensureRunning()
                statusVM.start()
                accountsVM.start()
                await maybeOpenWizard()
                // Repaint the menu every time the VM's `status` changes.
                withObservationTracking {
                    _ = statusVM.status
                    _ = statusVM.pendingApprovalCount
                    _ = statusVM.lastError
                } onChange: { [weak self] in
                    Task { @MainActor in self?.rebuildMenu() }
                }
            } catch {
                showLaunchError(error)
            }
        }
    }

    private func maybeOpenWizard() async {
        // Wait briefly for the first status to land, then check.
        for _ in 0..<10 {
            await statusVM.refresh()
            if statusVM.status != nil { break }
            try? await Task.sleep(for: .milliseconds(200))
        }
        if let s = statusVM.status, s.accountCount == 0 && !s.onboardingComplete {
            wizard.show()
        }
    }

    private func rebuildMenu() {
        let menu = NSMenu()
        if let s = statusVM.status {
            let line = "Status: \(s.mcpPaused ? "Paused" : "Connected") — \(s.accountCount) account\(s.accountCount == 1 ? "" : "s")"
            menu.addItem(NSMenuItem(title: line, action: nil, keyEquivalent: ""))
        } else if let err = statusVM.lastError {
            menu.addItem(NSMenuItem(title: "Error: \(err)", action: nil, keyEquivalent: ""))
        } else {
            menu.addItem(NSMenuItem(title: "Connecting…", action: nil, keyEquivalent: ""))
        }
        if statusVM.pendingApprovalCount > 0 {
            menu.addItem(NSMenuItem(
                title: "Pending approvals: \(statusVM.pendingApprovalCount)",
                action: nil,
                keyEquivalent: ""
            ))
        }
        menu.addItem(NSMenuItem.separator())

        let pauseTitle = (statusVM.status?.mcpPaused ?? false) ? "Resume MCP" : "Pause MCP"
        let pauseItem = NSMenuItem(
            title: pauseTitle,
            action: #selector(togglePause),
            keyEquivalent: ""
        )
        pauseItem.target = self
        menu.addItem(pauseItem)

        let settings = NSMenuItem(
            title: "Open Settings…",
            action: #selector(openSettings),
            keyEquivalent: ","
        )
        settings.target = self
        menu.addItem(settings)
        menu.addItem(NSMenuItem.separator())

        let setup = NSMenuItem(
            title: "Run Setup Again…",
            action: #selector(openWizard),
            keyEquivalent: ""
        )
        setup.target = self
        menu.addItem(setup)
        menu.addItem(NSMenuItem.separator())
        menu.addItem(NSMenuItem(
            title: "Quit MailMCP",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        ))
        statusItem.menu = menu
    }

    @objc private func openWizard() { wizard.show() }

    @objc private func openSettings() {
        if let w = settingsWindow {
            w.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let view = SettingsRoot(accountsVM: accountsVM) { [weak self] in
            self?.wizard.show()
        }
        let host = NSHostingController(rootView: view)
        let w = NSWindow(contentViewController: host)
        w.title = "MailMCP Settings"
        w.styleMask = [.titled, .closable, .resizable]
        w.setContentSize(NSSize(width: 560, height: 360))
        w.center()
        settingsWindow = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func togglePause() {
        let newPaused = !(statusVM.status?.mcpPaused ?? false)
        Task {
            do {
                let _: Empty = try await client.call(
                    "mcp.pause",
                    params: ["paused": .bool(newPaused)]
                )
                await statusVM.refresh()
            } catch {
                NSAlert(error: error).runModal()
            }
        }
    }

    private func showLaunchError(_ error: Error) {
        let alert = NSAlert()
        alert.messageText = "MailMCP couldn't start the daemon"
        alert.informativeText = (error as? LocalizedError)?.errorDescription ?? "\(error)"
        alert.alertStyle = .critical
        alert.addButton(withTitle: "Show Logs")
        alert.addButton(withTitle: "Quit")
        let resp = alert.runModal()
        if resp == .alertFirstButtonReturn {
            NSWorkspace.shared.open(paths.logsDir)
        }
        NSApp.terminate(nil)
    }
}
