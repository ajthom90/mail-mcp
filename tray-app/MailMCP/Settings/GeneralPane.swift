import AppKit
import SwiftUI
import ServiceManagement

struct GeneralPane: View {
    @Bindable var statusVM: StatusViewModel
    let client: IpcClient
    let paths: MailMCPPaths
    let onRunSetup: () -> Void

    @State private var endpoint: McpEndpointInfo?
    @State private var revealToken = false
    @State private var autostartEnabled: Bool = false

    var body: some View {
        Form {
            Section("Daemon") {
                if let s = statusVM.status {
                    LabeledContent("Status") {
                        Text(s.mcpPaused ? "Paused" : "Running")
                            .foregroundStyle(s.mcpPaused ? .orange : .green)
                    }
                    LabeledContent("Uptime", value: formatUptime(s.uptimeSecs))
                    LabeledContent("Version", value: s.version)
                }
                Toggle("Pause MCP (refuse tool calls)", isOn: Binding(
                    get: { statusVM.status?.mcpPaused ?? false },
                    set: { newValue in
                        Task {
                            let _: Empty? = try? await client.call(
                                "mcp.pause",
                                params: ["paused": .bool(newValue)]
                            )
                            await statusVM.refresh()
                        }
                    }
                ))
            }

            Section("Autostart") {
                Toggle("Run MailMCP at login", isOn: $autostartEnabled)
                    .onChange(of: autostartEnabled) { _, new in
                        Task { await commitAutostart(new) }
                    }
                Text("Registers MailMCP as a login item via SMAppService.mainApp. The tray launches automatically and spawns the daemon if it isn't already running. (Daemon-only autostart via a launchd agent plist is a v0.1c follow-up.)")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("MCP endpoint") {
                if let e = endpoint {
                    LabeledContent("URL") {
                        HStack { Text(e.url).textSelection(.enabled); copyButton(e.url) }
                    }
                    LabeledContent("Bearer token") {
                        HStack {
                            if revealToken {
                                Text(e.bearerToken).textSelection(.enabled).font(.system(.body, design: .monospaced))
                            } else {
                                Text(String(repeating: "•", count: 32))
                            }
                            Button(revealToken ? "Hide" : "Reveal") { revealToken.toggle() }
                                .controlSize(.small)
                            copyButton(e.bearerToken)
                        }
                    }
                    if let shim = e.stdioShimPath {
                        LabeledContent("Stdio shim") {
                            HStack { Text(shim).textSelection(.enabled).font(.caption); copyButton(shim) }
                        }
                    }
                }
            }

            Section("Logs") {
                LabeledContent("Path") {
                    HStack {
                        Text(paths.logsDir.path).textSelection(.enabled).font(.caption)
                        Button("Show in Finder") { NSWorkspace.shared.open(paths.logsDir) }
                            .controlSize(.small)
                    }
                }
            }

            Section {
                Button("Run setup again…", action: onRunSetup)
            }
        }
        .formStyle(.grouped)
        .padding()
        .task {
            endpoint = try? await client.call("mcp.endpoint")
            autostartEnabled = SMAppService.mainApp.status == .enabled
        }
    }

    private func commitAutostart(_ enabled: Bool) async {
        do {
            if enabled { try SMAppService.mainApp.register() }
            else       { try await SMAppService.mainApp.unregister() }
        } catch {
            NSLog("SMAppService toggle failed: \(error)")
        }
        let _: Empty? = try? await client.call(
            "settings.set_autostart",
            params: ["enabled": .bool(enabled)]
        )
    }

    private func formatUptime(_ secs: UInt64) -> String {
        let h = secs / 3600
        let m = (secs % 3600) / 60
        let s = secs % 60
        if h > 0 { return "\(h)h \(m)m" }
        if m > 0 { return "\(m)m \(s)s" }
        return "\(s)s"
    }

    private func copyButton(_ s: String) -> some View {
        Button {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(s, forType: .string)
        } label: {
            Image(systemName: "doc.on.doc")
        }
        .controlSize(.small)
    }
}
