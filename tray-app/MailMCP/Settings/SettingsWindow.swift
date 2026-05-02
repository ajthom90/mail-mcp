import SwiftUI

/// Wires the Settings scene panes. Phase A includes only Accounts; Phase B
/// adds Permissions, General, About.
public struct SettingsRoot: View {
    let accountsVM: AccountsViewModel
    let permissionsVM: PermissionsViewModel
    let onAddAccount: () -> Void

    public var body: some View {
        TabView {
            AccountsPane(vm: accountsVM, onAddAccount: onAddAccount)
                .tabItem { Label("Accounts", systemImage: "person.crop.circle") }
            PermissionsPane(accounts: accountsVM, vm: permissionsVM)
                .tabItem { Label("Permissions", systemImage: "lock.shield") }
        }
        .frame(minWidth: 560, minHeight: 360)
        .padding()
    }
}
