import SwiftUI

struct AddAccountView: View {
    @Bindable var state: WizardState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Add an account")
                .font(.title).bold()
            Text("MailMCP supports Gmail and Microsoft 365. IMAP is coming soon.")
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
            providerCard(name: "Gmail", systemImage: "envelope.fill", providerId: "gmail")
            providerCard(name: "Microsoft 365", systemImage: "building.2.fill", providerId: "m365")
            providerCard(name: "IMAP", systemImage: "server.rack", providerId: nil)
        }
    }

    private func providerCard(name: String, systemImage: String, providerId: String?) -> some View {
        let enabled = providerId != nil
        let selected = providerId != nil && state.selectedProvider == providerId
        return Button {
            if let providerId {
                state.selectedProvider = providerId
            }
        } label: {
            VStack(spacing: 8) {
                Image(systemName: systemImage).font(.system(size: 32))
                Text(name).font(.headline)
                if !enabled {
                    Text("Coming soon").font(.caption).foregroundStyle(.secondary)
                }
            }
            .frame(width: 140, height: 110)
            .background(
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(selected ? Color.accentColor : Color.secondary.opacity(0.3),
                                  lineWidth: selected ? 2 : 1)
            )
            .opacity(enabled ? 1.0 : 0.5)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
    }
}
