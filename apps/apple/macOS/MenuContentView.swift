import ScrovaKit
import SwiftUI

struct MenuContentView: View {
    @Bindable var model: AppModel

    /// The menu is a glance, not the history view.
    private static let recentLimit = 10
    private static let recentMaxHeight: CGFloat = 340

    @State private var recentHeight: CGFloat = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if model.automationDenied {
                permissionNotice
                    .padding(16)
                Divider()
            }
            nowPlaying
                .padding(16)
            Divider()
            recent
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
            Divider()
            footer
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
        }
        .frame(width: 320, alignment: .leading)
        .onAppear { model.refreshLoginItem() }
    }

    // MARK: Now playing

    private var permissionNotice: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Scrova can’t see Music yet")
                .font(.headline)
            Text("Allow Scrova to control Music in System Settings so it can read what is playing.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Button("Open Automation Settings", action: model.openAutomationSettings)
        }
    }

    private var nowPlaying: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 12) {
                artwork
                VStack(alignment: .leading, spacing: 2) {
                    if let track = model.currentTrack {
                        Text(track.title)
                            .font(.system(size: 15, weight: .semibold))
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                        if !track.artist.isEmpty {
                            Text(track.artist)
                                .font(.callout)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        if let album = track.album, !album.isEmpty {
                            Text(album)
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                                .lineLimit(1)
                        }
                    } else {
                        Text("Nothing playing")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(.secondary)
                        Text("Play something in Music and it shows up here.")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            if let progress = model.progress, model.currentTrack != nil {
                ScrobbleMeter(progress: progress, tint: model.tint)
            }
            if let readFailure = model.readFailure {
                Text(readFailure)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var artwork: some View {
        let isPaused = model.playbackState == .paused
        return ZStack {
            RoundedRectangle(cornerRadius: 8)
                .fill(.quaternary)
            if let image = model.artwork, model.currentTrack != nil {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            } else {
                Image(systemName: "music.note")
                    .font(.system(size: 22))
                    .foregroundStyle(.tertiary)
            }
        }
        .frame(width: 64, height: 64)
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay {
            // A dimmed cover reads as "paused" at a glance, without another label.
            if isPaused, model.currentTrack != nil {
                RoundedRectangle(cornerRadius: 8)
                    .fill(.black.opacity(0.45))
                Image(systemName: "pause.fill")
                    .font(.system(size: 18))
                    .foregroundStyle(.white)
            }
        }
        .accessibilityLabel(isPaused ? "Paused" : "")
    }

    // MARK: Recent

    private var recent: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text("Recent scrobbles")
                Spacer()
                if model.pendingCount > 0 {
                    Text("\(model.pendingCount) waiting to send")
                        .monospacedDigit()
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)

            if model.recentScrobbles.isEmpty {
                Text("Tracks appear here once you’ve listened to half of them, or four minutes.")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                // Wrapped titles make rows tall; the menu must not outgrow the
                // screen. A ScrollView has no height of its own inside the
                // self-sizing menu window, so it is sized from its content.
                ScrollView {
                    VStack(spacing: 6) {
                        ForEach(Array(model.recentScrobbles.prefix(Self.recentLimit).enumerated()), id: \.offset) { _, record in
                            RecentScrobbleRow(record: record)
                                .padding(10)
                                .background(.quinary, in: RoundedRectangle(cornerRadius: 10))
                        }
                    }
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { recentHeight = $0 }
                }
                .scrollBounceBehavior(.basedOnSize)
                .frame(height: min(recentHeight, Self.recentMaxHeight))
            }
        }
    }

    // MARK: Account and settings

    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(notices, id: \.self) { notice in
                Text(notice)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 8) {
                account
                Spacer(minLength: 8)
                settingsMenu
            }
        }
    }

    @ViewBuilder
    private var account: some View {
        switch model.account {
        case .signedOut:
            Button("Sign in to Last.fm", action: model.signIn)
                .buttonStyle(.borderedProminent)
                .disabled(!ScrovaSetup.hasLastfmCredentials)
        case .awaitingApproval:
            ProgressView()
                .controlSize(.small)
            Text("Approve Scrova in your browser")
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        case .signedIn(let username):
            Image(systemName: "person.crop.circle")
                .foregroundStyle(.secondary)
            Text(username)
                .font(.callout)
                .lineLimit(1)
            if model.isSending {
                ProgressView()
                    .controlSize(.mini)
            }
        }
    }

    private var settingsMenu: some View {
        Menu {
            switch model.account {
            case .signedOut:
                EmptyView()
            case .awaitingApproval:
                Button("Open Approval Page Again", action: model.reopenApprovalPage)
                Button("Cancel Sign-In", action: model.cancelSignIn)
                Divider()
            case .signedIn(let username):
                Button("Send Now", action: model.flush)
                    .disabled(model.isSending || model.pendingCount == 0)
                if let profile = URL(string: "https://www.last.fm/user/\(username)") {
                    Link("Open Last.fm Profile", destination: profile)
                }
                Button("Sign Out", action: model.signOut)
                Divider()
            }
            Toggle(
                "Launch at Login",
                isOn: Binding(
                    get: { model.launchesAtLogin },
                    set: { model.setLaunchesAtLogin($0) }
                )
            )
            Divider()
            Button("Quit Scrova", action: model.quit)
                .keyboardShortcut("q")
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.system(size: 15))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .accessibilityLabel("Settings")
    }

    private var notices: [String] {
        var notices: [String] = []
        if let setupFailure = model.setupFailure { notices.append(setupFailure) }
        if case .signedOut = model.account, !ScrovaSetup.hasLastfmCredentials {
            notices.append(ScrovaSetup.missingCredentialsNotice)
        }
        if let accountNotice = model.accountNotice { notices.append(accountNotice) }
        if let loginItemNotice = model.loginItemNotice, model.loginItemRequested || model.launchesAtLogin {
            notices.append(loginItemNotice)
        }
        return notices
    }
}
