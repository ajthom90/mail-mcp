import AppKit
import SwiftUI

struct OAuthView: View {
    @Bindable var state: WizardState
    let client: IpcClient
    @State private var status: String = "Preparing…"
    @State private var hasStarted = false
    @State private var task: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Sign in to \(providerDisplayName)")
                .font(.title).bold()
            Text(status).foregroundStyle(.secondary)
            ProgressView()
            Spacer()
            HStack {
                Button("Cancel") { cancel() }
                Spacer()
            }
        }
        .task {
            guard !hasStarted else { return }
            hasStarted = true
            await begin()
        }
    }

    private var providerDisplayName: String {
        switch state.selectedProvider {
        case "m365": return "Microsoft 365"
        default: return "Gmail"
        }
    }

    private func begin() async {
        status = "Asking the daemon to start sign-in…"
        do {
            let progress: AccountAddOAuthInProgress = try await client.call(
                "accounts.add_oauth",
                params: ["provider": .string(state.selectedProvider)]
            )
            state.pendingChallengeId = progress.challengeId
            state.pendingAuthURL = URL(string: progress.authUrl)
            if let url = state.pendingAuthURL {
                status = "Opening your browser. Complete sign-in there."
                NSWorkspace.shared.open(url)
            }

            // Subscribe BEFORE calling complete_oauth so we don't miss the event.
            let stream = try await client.subscribe(events: ["account.added"])
            // complete_oauth blocks server-side until the OAuth callback resolves.
            // It returns the new Account record. Run it concurrently with the
            // notification listener; whichever signals first wins.
            try await withThrowingTaskGroup(of: Void.self) { group in
                group.addTask {
                    let _: AnyJSON = try await client.call(
                        "accounts.complete_oauth",
                        params: [
                            "challenge_id": .string(progress.challengeId),
                            "label": .string(""),
                        ]
                    )
                }
                group.addTask {
                    for await note in stream {
                        if case .accountAdded = note {
                            return
                        }
                    }
                }
                try await group.next()
                group.cancelAll()
            }
            status = "Signed in. Continuing…"
            state.advance()
        } catch {
            status = "Sign-in failed: \(error.localizedDescription)"
            state.error = error.localizedDescription
        }
    }

    private func cancel() {
        if let cid = state.pendingChallengeId {
            Task {
                let _: Empty? = try? await client.call(
                    "accounts.cancel_oauth",
                    params: ["challenge_id": .string(cid)]
                )
            }
        }
        NSApp.keyWindow?.close()
    }
}
