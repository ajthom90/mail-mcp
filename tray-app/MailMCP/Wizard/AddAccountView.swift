import SwiftUI

struct AddAccountView: View {
    @Bindable var state: WizardState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Add an account")
                .font(.title).bold()
            Text("MailMCP supports Gmail in this release. Microsoft 365 and IMAP are coming soon.")
                .foregroundStyle(.secondary)
            providerGrid
            Spacer()
            HStack {
                Button("Back") { state.back() }
                Spacer()
                Button("Continue") { state.advance() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    @ViewBuilder
    private var providerGrid: some View {
        HStack(spacing: 16) {
            providerCard(name: "Gmail", systemImage: "envelope.fill", enabled: true)
            providerCard(name: "Microsoft 365", systemImage: "building.2.fill", enabled: false)
            providerCard(name: "IMAP", systemImage: "server.rack", enabled: false)
        }
    }

    private func providerCard(name: String, systemImage: String, enabled: Bool) -> some View {
        VStack(spacing: 8) {
            Image(systemName: systemImage).font(.system(size: 32))
            Text(name).font(.headline)
            if !enabled {
                Text("Coming soon").font(.caption).foregroundStyle(.secondary)
            }
        }
        .frame(width: 140, height: 110)
        .background(RoundedRectangle(cornerRadius: 8).strokeBorder(.tertiary))
        .opacity(enabled ? 1.0 : 0.5)
    }
}
