import SwiftUI

struct WizardPermissionsView: View {
    @Bindable var state: WizardState

    private static let rows: [(name: String, policy: String, blurb: String)] = [
        ("Read & search",        "Allow",     "AI can list and read messages."),
        ("Modify (label, archive, mark read)", "Allow", "Reversible triage."),
        ("Move to trash",        "Confirm",   "You'll be asked each time."),
        ("Create drafts",        "Allow",     "Drafts stay in your drafts folder until you send."),
        ("Send",                 "Convert to draft", "AI never sends without you. Default."),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Default permissions")
                .font(.title).bold()
            Text("These are the safe defaults for your new account. You can change them later in Settings → Permissions.")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Self.rows.indices, id: \.self) { i in
                    HStack(alignment: .top) {
                        VStack(alignment: .leading) {
                            Text(Self.rows[i].name).bold()
                            Text(Self.rows[i].blurb).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(Self.rows[i].policy)
                            .font(.body.monospaced())
                            .padding(.horizontal, 8).padding(.vertical, 4)
                            .background(RoundedRectangle(cornerRadius: 4).fill(.quaternary))
                    }
                }
            }
            Spacer()
            HStack {
                Button("Back") { state.back() }
                Spacer()
                Button("Continue") { state.advance() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}
