import MediaPlayer
import ScrovaKit
import SwiftUI
import UIKit

struct SettingsView: View {
    @Bindable var model: AppModel
    @Environment(\.openURL) private var openURL
    @State private var confirmSignOut = false

    var body: some View {
        NavigationStack {
            List {
                lastfmSection
                sendingSection
                permissionsSection
                Section {
                    NavigationLink("About Scrova") {
                        AboutView()
                    }
                }
            }
            .navigationTitle("Settings")
            .onAppear { model.refreshPermissions() }
            .confirmationDialog(
                "Sign out of Last.fm?",
                isPresented: $confirmSignOut,
                titleVisibility: .visible
            ) {
                Button("Sign out", role: .destructive, action: model.signOut)
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("Scrobbles that are waiting to send stay on this device.")
            }
        }
    }

    // MARK: Last.fm

    private var lastfmSection: some View {
        Section {
            switch model.account {
            case .signedOut:
                Button("Sign in to Last.fm", action: model.signIn)
                    .disabled(!ScrovaSetup.hasLastfmCredentials)
                if !ScrovaSetup.hasLastfmCredentials {
                    Text(ScrovaSetup.missingCredentialsNotice)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            case .awaitingApproval:
                HStack(spacing: 10) {
                    ProgressView()
                    Text("Approve Scrova on Last.fm")
                }
                Button("Open approval page again", action: model.reopenApprovalPage)
                Button("Cancel", role: .cancel, action: model.cancelSignIn)
            case .signedIn(let username):
                if let profile = LastfmLinks.profile(username: username) {
                    Link(destination: profile) {
                        Label(username, systemImage: "person.crop.circle")
                    }
                } else {
                    Label(username, systemImage: "person.crop.circle")
                }
                Button("Sign out", role: .destructive) {
                    confirmSignOut = true
                }
            }
        } header: {
            Text("Last.fm").textCase(nil)
        }
    }

    // MARK: Sending

    private var sendingSection: some View {
        Section {
            LabeledContent("Waiting to send", value: "\(model.pendingCount)")
            Button("Send now", action: model.flush)
                .disabled(model.isSending || model.pendingCount == 0)
            if let notice = model.accountNotice ?? model.setupFailure {
                Text(notice)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        } header: {
            Text("Sending").textCase(nil)
        }
    }

    // MARK: Permissions

    private var permissionsSection: some View {
        Section {
            LabeledContent("Media library") {
                Text(mediaLibraryLabel)
                    .foregroundStyle(.secondary)
            }
            Button("Open Settings", action: openAppSettings)

            LabeledContent("Background App Refresh") {
                Text(backgroundRefreshLabel)
                    .foregroundStyle(.secondary)
            }
            Text("Background App Refresh lets Scrova recover and send plays while it is closed.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        } header: {
            Text("Permissions").textCase(nil)
        }
    }

    private var mediaLibraryLabel: String {
        switch model.libraryAuthorizationStatus {
        case .authorized: "Allowed"
        case .denied: "Denied"
        case .restricted: "Restricted"
        case .notDetermined: "Not asked yet"
        @unknown default: "Unknown"
        }
    }

    private var backgroundRefreshLabel: String {
        switch model.backgroundRefreshStatus {
        case .available: "On"
        case .denied: "Off"
        case .restricted: "Restricted"
        @unknown default: "Unknown"
        }
    }

    private func openAppSettings() {
        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
        openURL(url)
    }
}
