import SwiftUI

struct PermissionsPane: View {
    @Bindable var accounts: AccountsViewModel
    @Bindable var vm: PermissionsViewModel

    var body: some View {
        VStack(alignment: .leading) {
            Text("Permissions").font(.title2).bold()
            Picker("Account", selection: $vm.selectedAccountId) {
                Text("Pick an account…").tag(AccountID?.none)
                ForEach(accounts.accounts) { a in
                    Text("\(a.label) (\(a.email))").tag(AccountID?.some(a.id))
                }
            }
            .pickerStyle(.menu)
            .padding(.bottom, 8)

            if let p = vm.permissions {
                VStack(spacing: 8) {
                    permissionRow(
                        title: "Read & search",
                        explanation: "AI can list messages, threads, labels.",
                        category: .read,
                        current: p.read,
                        choices: [.allow, .confirm, .session, .block]
                    )
                    permissionRow(
                        title: "Modify",
                        explanation: "Mark read, label, archive.",
                        category: .modify,
                        current: p.modify,
                        choices: [.allow, .confirm, .session, .block]
                    )
                    permissionRow(
                        title: "Trash",
                        explanation: "Move to trash (semi-reversible).",
                        category: .trash,
                        current: p.trash,
                        choices: [.allow, .confirm, .session, .block]
                    )
                    permissionRow(
                        title: "Drafts",
                        explanation: "Create or update drafts.",
                        category: .draft,
                        current: p.draft,
                        choices: [.allow, .confirm, .session, .block]
                    )
                    permissionRow(
                        title: "Send",
                        explanation: "Default policy converts sends into drafts.",
                        category: .send,
                        current: p.send,
                        choices: [.allow, .confirm, .session, .draftify, .block]
                    )
                }
            } else if vm.selectedAccountId == nil {
                ContentUnavailableView(
                    "Pick an account",
                    systemImage: "lock.shield",
                    description: Text("Permissions are per-account.")
                )
            } else {
                ProgressView()
            }
            Spacer()
        }
        .padding()
    }

    private func permissionRow(
        title: String,
        explanation: String,
        category: Category,
        current: Policy,
        choices: [Policy]
    ) -> some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading) {
                Text(title).bold()
                Text(explanation).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Picker("", selection: Binding(
                get: { current },
                set: { newValue in Task { await vm.set(category: category, policy: newValue) } }
            )) {
                ForEach(choices, id: \.rawValue) { Text($0.rawValue.capitalized).tag($0) }
            }
            .labelsHidden()
            .frame(width: 180)
        }
    }
}
