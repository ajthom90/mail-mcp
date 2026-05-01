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

        menu.addItem(NSMenuItem.separator())
        menu.addItem(NSMenuItem(
            title: "Quit MailMCP",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        ))
        statusItem.menu = menu
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
