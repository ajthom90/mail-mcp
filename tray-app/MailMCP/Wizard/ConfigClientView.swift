import AppKit
import SwiftUI

struct ConfigClientView: View {
    @Bindable var state: WizardState
    let onClose: () -> Void
    let client: IpcClient
    @State private var endpoint: McpEndpointInfo?

    private var snippet: String {
        let path = endpoint?.stdioShimPath ??
            (Bundle.main.url(forAuxiliaryExecutable: "mail-mcp-stdio")?.path
                ?? "/path/to/mail-mcp-stdio")
        return """
        {
          "mcpServers": {
            "mail-mcp": {
              "command": "\(path)"
            }
          }
        }
        """
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Configure your AI client").font(.title).bold()
            Text("Add this snippet to Claude Desktop's config to give it access to MailMCP:")
            ScrollView {
                Text(snippet)
                    .font(.system(.body, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .frame(height: 120)
            .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary))

            HStack {
                Button("Copy snippet") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(snippet, forType: .string)
                }
                Button("Open Claude Desktop config") {
                    let url = FileManager.default.homeDirectoryForCurrentUser
                        .appendingPathComponent(
                            "Library/Application Support/Claude/claude_desktop_config.json"
                        )
                    NSWorkspace.shared.open(url.deletingLastPathComponent())
                }
            }

            Spacer()
            HStack {
                Spacer()
                Button("Done") { onClose() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .task { await loadEndpoint() }
    }

    private func loadEndpoint() async {
        endpoint = try? await client.call("mcp.endpoint")
    }
}
