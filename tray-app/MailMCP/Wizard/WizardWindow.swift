import AppKit
import SwiftUI
import Observation

@MainActor
@Observable
public final class WizardState {
    public enum Step: Int, CaseIterable {
        case welcome, addAccount, oauth, permissions, autostart, configClient
    }
    public var step: Step = .welcome
    public var error: String?
    public var pendingChallengeId: String?
    public var pendingAuthURL: URL?
    public var addedAccountLabel: String?
    /// Provider chosen on the AddAccount step. One of "gmail" or "m365".
    /// Default is "gmail" so users who muscle-memory hit Continue still work.
    public var selectedProvider: String = "gmail"

    public func advance() {
        if let next = Step(rawValue: step.rawValue + 1) { step = next }
    }
    public func back() {
        if let prev = Step(rawValue: step.rawValue - 1) { step = prev }
    }
}

/// Owns the wizard NSWindow. Showing it is `WizardController.show()`.
@MainActor
public final class WizardController {
    private let client: IpcClient
    private var window: NSWindow?
    private var state = WizardState()

    public init(client: IpcClient) { self.client = client }

    public func show() {
        if let window {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let view = WizardRootView(state: state, client: client) { [weak self] in
            self?.close()
        }
        let hosting = NSHostingController(rootView: view)
        let window = NSWindow(contentViewController: hosting)
        window.title = "Welcome to MailMCP"
        window.styleMask = [.titled, .closable]
        window.setContentSize(NSSize(width: 520, height: 420))
        window.center()
        self.window = window
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    public func close() {
        window?.close()
        window = nil
    }
}

private struct WizardRootView: View {
    @Bindable var state: WizardState
    let client: IpcClient
    let onClose: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            stepProgress
            Divider()
            stepContent
                .padding(24)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder
    private var stepProgress: some View {
        HStack(spacing: 4) {
            ForEach(WizardState.Step.allCases, id: \.rawValue) { s in
                Circle()
                    .fill(s.rawValue <= state.step.rawValue ? Color.accentColor : Color.gray.opacity(0.3))
                    .frame(width: 8, height: 8)
            }
        }
        .padding(.vertical, 12)
    }

    @ViewBuilder
    private var stepContent: some View {
        switch state.step {
        case .welcome:        WelcomeView(state: state)
        case .addAccount:     AddAccountView(state: state)
        case .oauth:          OAuthView(state: state, client: client)
        case .permissions:    WizardPermissionsView(state: state)
        case .autostart:      AutostartView(state: state, client: client)
        case .configClient:   ConfigClientView(state: state, onClose: onClose, client: client)
        }
    }
}
