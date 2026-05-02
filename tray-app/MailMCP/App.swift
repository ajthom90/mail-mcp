import AppKit
import SwiftUI

@main
struct MailMCPApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        // No primary window — this is a menu-bar accessory app (LSUIElement=true).
        // The MenuBarController owns the NSStatusItem and any windows we open.
        Settings { EmptyView() }   // placeholder so SwiftUI gives us a Settings menu item
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    var menuBar: MenuBarController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Don't show in Dock or app switcher.
        NSApp.setActivationPolicy(.accessory)
        menuBar = MenuBarController()
        menuBar?.start()
    }
}
