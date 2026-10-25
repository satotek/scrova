import ScrovaKit
import SwiftUI

struct SettingsView: View {
    @Bindable var model: AppModel

    var body: some View {
        TabView {
            GeneralSettingsView(model: model)
                .tabItem { Label("General", systemImage: "gearshape") }

            LastfmSettingsView(model: model)
                .tabItem { Label("Last.fm", systemImage: "person.crop.circle") }

            NavigationStack {
                AboutView()
            }
            .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 520, height: 460)
    }
}

private struct GeneralSettingsView: View {
    @Bindable var model: AppModel

    var body: some View {
        Form {
            Section {
                Toggle(
                    "Launch at login",
                    isOn: Binding(
                        get: { model.launchesAtLogin },
                        set: { model.setLaunchesAtLogin($0) }
                    )
                )
            } footer: {
                Text("Scrova stays in the menu bar and keeps scrobbling while it is running.")
            }

            if let notice = model.loginItemNotice,
               model.loginItemRequested || model.launchesAtLogin {
                Section {
                    Text(notice)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .onAppear { model.refreshLoginItem() }
    }
}

private struct LastfmSettingsView: View {
    @Bindable var model: AppModel
    @State private var confirmSignOut = false

    var body: some View {
        Form {
            Section("Last.fm") {
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
            }

            Section("Sending") {
                LabeledContent("Waiting to send", value: "\(model.pendingCount)")
                Button("Send now", action: model.flush)
                    .disabled(model.isSending || model.pendingCount == 0)
                if let notice = model.accountNotice ?? model.setupFailure {
                    Text(notice)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
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
