import SwiftUI

/// Wires the Settings scene panes. Phase A includes only Accounts; Phase B
/// adds Permissions, General, About.
public struct SettingsRoot: View {
    let accountsVM: AccountsViewModel
    let permissionsVM: PermissionsViewModel
    let statusVM: StatusViewModel
    let client: IpcClient
    let paths: MailMCPPaths
    let onAddAccount: () -> Void
    let onRunSetup: () -> Void

    public var body: some View {
        TabView {
            AccountsPane(vm: accountsVM, onAddAccount: onAddAccount)
                .tabItem { Label("Accounts", systemImage: "person.crop.circle") }
            PermissionsPane(accounts: accountsVM, vm: permissionsVM)
                .tabItem { Label("Permissions", systemImage: "lock.shield") }
            GeneralPane(statusVM: statusVM, client: client, paths: paths, onRunSetup: onRunSetup)
                .tabItem { Label("General", systemImage: "gear") }
            AboutPane(statusVM: statusVM, paths: paths)
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(minWidth: 600, minHeight: 420)
        .padding()
    }
}
