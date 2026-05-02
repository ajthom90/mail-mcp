import AppKit
import SwiftUI

struct AboutPane: View {
    @Bindable var statusVM: StatusViewModel
    let paths: MailMCPPaths

    @State private var logTail: String = "(loading…)"

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("About MailMCP").font(.title2).bold()
            LabeledContent("App version", value: appVersion)
            LabeledContent("Daemon version", value: statusVM.status?.version ?? "—")
            LabeledContent("Daemon uptime", value: statusVM.status.map { formatUptime($0.uptimeSecs) } ?? "—")

            Text("Recent log lines (redacted)").bold().padding(.top, 8)
            ScrollView {
                Text(logTail)
                    .font(.system(.caption, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                    .padding(8)
            }
            .frame(minHeight: 160)
            .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary))

            HStack {
                Button("Refresh") { Task { await loadLogTail() } }
                Button("Copy support bundle") { copySupportBundle() }
                Spacer()
                Link(
                    "GitHub",
                    destination: URL(string: "https://github.com/ajthom90/mail-mcp")!
                )
            }
        }
        .padding()
        .task { await loadLogTail() }
    }

    private func loadLogTail() async {
        let logFile = paths.logsDir.appendingPathComponent("daemon.log")
        guard let data = try? Data(contentsOf: logFile),
              let s = String(data: data, encoding: .utf8) else {
            logTail = "(no log file yet at \(logFile.path))"
            return
        }
        let lines = s.split(separator: "\n", omittingEmptySubsequences: false)
        let tail = lines.suffix(50).joined(separator: "\n")
        logTail = String(tail)
    }

    private func copySupportBundle() {
        var lines: [String] = []
        lines.append("MailMCP \(appVersion)")
        if let s = statusVM.status {
            lines.append("Daemon \(s.version), uptime \(formatUptime(s.uptimeSecs))")
            lines.append("Accounts: \(s.accountCount), MCP paused: \(s.mcpPaused)")
        }
        lines.append("---")
        lines.append(logTail)
        let bundle = lines.joined(separator: "\n")
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(bundle, forType: .string)
    }

    private func formatUptime(_ secs: UInt64) -> String {
        let h = secs / 3600
        let m = (secs % 3600) / 60
        return h > 0 ? "\(h)h \(m)m" : "\(m)m \(secs % 60)s"
    }
}
