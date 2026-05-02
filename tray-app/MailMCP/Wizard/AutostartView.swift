import SwiftUI
import ServiceManagement

struct AutostartView: View {
    @Bindable var state: WizardState
    let client: IpcClient
    @State private var enabled: Bool = true

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Run at login?")
                .font(.title).bold()
            Text("MailMCP can start automatically when you log in. The daemon stays running in the background; the menu-bar app launches on demand.")
                .foregroundStyle(.secondary)

            VStack(alignment: .leading) {
                Toggle("Run at login (recommended)", isOn: $enabled)
            }
            Spacer()
            HStack {
                Button("Back") { state.back() }
                Spacer()
                Button("Continue") {
                    Task { await commit() }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
    }

    private func commit() async {
        // Persist to the daemon (status reflects user choice; SMAppService is a Phase B
        // wiring detail and won't block onboarding if it fails).
        let _: Empty? = try? await client.call(
            "settings.set_autostart",
            params: ["enabled": .bool(enabled)]
        )
        let _: Empty? = try? await client.call(
            "settings.set_onboarding_complete",
            params: ["complete": .bool(true)]
        )
        state.advance()
    }
}
