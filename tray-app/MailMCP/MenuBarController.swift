import AppKit
import SwiftUI

/// Owns the NSStatusItem (menu-bar icon) and routes menu-driven actions.
/// In Phase A this exposes a minimal menu; richer status rendering ships in Task 13.
@MainActor
final class MenuBarController {
    private let statusItem = NSStatusBar.system.statusItem(
        withLength: NSStatusItem.variableLength
    )

    func start() {
        statusItem.button?.image = NSImage(
            systemSymbolName: "envelope.fill",
            accessibilityDescription: "MailMCP"
        )
        statusItem.button?.image?.isTemplate = true   // Renders monochrome in light/dark.
        rebuildMenu()
    }

    private func rebuildMenu() {
        let menu = NSMenu()
        menu.addItem(NSMenuItem(
            title: "MailMCP — starting…",
            action: nil,
            keyEquivalent: ""
        ))
        menu.addItem(NSMenuItem.separator())
        menu.addItem(NSMenuItem(
            title: "Quit",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        ))
        statusItem.menu = menu
    }
}
