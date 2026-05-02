import SwiftUI

struct WelcomeView: View {
    @Bindable var state: WizardState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Welcome to MailMCP")
                .font(.largeTitle).bold()
            Text("MailMCP lets your local AI assistant (Claude Desktop and other MCP clients) read, triage, and compose email through your existing accounts. Everything stays on your machine.")
                .font(.body)
            Text("Let's connect your first account.")
                .font(.body)
            Spacer()
            HStack {
                Spacer()
                Button("Skip Setup") { NSApp.keyWindow?.close() }
                Button("Continue") { state.advance() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}
