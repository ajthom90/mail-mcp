import SwiftUI

struct AccountsPane: View {
    @Bindable var vm: AccountsViewModel
    let onAddAccount: () -> Void

    var body: some View {
        VStack(alignment: .leading) {
            HStack {
                Text("Accounts").font(.title2).bold()
                Spacer()
                Button(action: onAddAccount) {
                    Label("Add Account", systemImage: "plus")
                }
            }
            .padding(.bottom, 8)

            if vm.accounts.isEmpty && !vm.isLoading {
                ContentUnavailableView(
                    "No accounts yet",
                    systemImage: "envelope.badge",
                    description: Text("Click Add Account to connect Gmail.")
                )
            } else {
                List {
                    ForEach(vm.accounts) { acc in
                        AccountRow(account: acc) {
                            Task { await vm.remove(id: acc.id) }
                        }
                    }
                }
            }
        }
        .padding()
        .task { await vm.refresh() }
    }
}

private struct AccountRow: View {
    let account: AccountListItem
    let onRemove: () -> Void
    @State private var showingConfirm = false

    var body: some View {
        HStack {
            Image(systemName: "envelope.fill")
                .foregroundStyle(.tint)
            VStack(alignment: .leading) {
                Text(account.label).bold()
                Text(account.email).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            statusBadge
            Button("Remove", role: .destructive) {
                showingConfirm = true
            }
            .controlSize(.small)
        }
        .padding(.vertical, 4)
        .alert(
            "Remove \(account.label)?",
            isPresented: $showingConfirm,
            actions: {
                Button("Remove", role: .destructive) { onRemove() }
                Button("Cancel", role: .cancel) {}
            },
            message: {
                Text("MailMCP will forget this account's tokens. You can re-add it any time.")
            }
        )
    }

    @ViewBuilder
    private var statusBadge: some View {
        switch account.status {
        case .ok:
            Text("Connected").font(.caption).foregroundStyle(.green)
        case .needsReauth:
            Text("Re-auth needed").font(.caption).foregroundStyle(.orange)
        case .networkError:
            Text("Offline").font(.caption).foregroundStyle(.red)
        }
    }
}
