import SwiftUI

/// Wires the Settings scene panes. Phase A includes only Accounts; Phase B
/// adds Permissions, General, About.
public struct SettingsRoot: View {
    let accountsVM: AccountsViewModel
    let onAddAccount: () -> Void

    public var body: some View {
        TabView {
            AccountsPane(vm: accountsVM, onAddAccount: onAddAccount)
                .tabItem { Label("Accounts", systemImage: "person.crop.circle") }
                .frame(minWidth: 480, minHeight: 300)
        }
        .padding()
    }
}
