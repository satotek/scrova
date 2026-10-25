import ScrovaKit
import SwiftUI

struct ContentView: View {
    @Bindable var model: AppModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationStack {
            List {
                Section {
                    nowPlaying
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets(top: 8, leading: 20, bottom: 16, trailing: 20))
                }
                recent
                account
            }
            .navigationTitle("Scrova")
            .navigationBarTitleDisplayMode(.inline)
        }
        .onAppear { model.activate() }
        .onChange(of: scenePhase) { _, phase in
            // `.inactive` is still in the foreground (control center, app switcher).
            // Only a real background transition stops the engine clock.
            model.setForeground(phase != .background)
        }
    }

    // MARK: Now playing

    @ViewBuilder
    private var nowPlaying: some View {
        if model.libraryDenied {
            VStack(alignment: .leading, spacing: 8) {
                Text("Scrova can’t see Music yet")
                    .font(.headline)
                Text("Allow access to Apple Music and your media library in Settings so Scrova can read what is playing.")
                    .foregroundStyle(.secondary)
            }
        } else {
            VStack(alignment: .leading, spacing: 18) {
                artwork
                VStack(alignment: .leading, spacing: 4) {
                    if let track = model.currentTrack {
                        Text(track.title)
                            .font(.title2.weight(.semibold))
                            .lineLimit(2)
                        if !track.artist.isEmpty {
                            Text(track.artist)
                                .font(.body)
                                .foregroundStyle(.secondary)
                        }
                        if let album = track.album, !album.isEmpty {
                            Text(album)
                                .font(.subheadline)
                                .foregroundStyle(.tertiary)
                        }
                    } else {
                        Text("Nothing playing")
                            .font(.title2.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text("Play something in Music, then come back. Scrova only listens while it is open.")
                            .font(.subheadline)
                            .foregroundStyle(.tertiary)
                    }
                }
                if let progress = model.progress, model.currentTrack != nil {
                    ScrobbleMeter(progress: progress, tint: model.tint)
                }
            }
        }
    }

    private var artwork: some View {
        let isPaused = model.playbackState == .paused && model.currentTrack != nil
        // Full width when there is a cover to show; an empty square that size
        // would be the loudest thing on screen while saying nothing.
        return Color.clear
            .aspectRatio(1, contentMode: .fit)
            .frame(maxWidth: model.currentTrack == nil ? 88 : .infinity)
            .background {
                ZStack {
                    Rectangle().fill(.quaternary)
                    if let image = model.artwork, model.currentTrack != nil {
                        Image(uiImage: image)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    } else {
                        Image(systemName: "music.note")
                            .font(.system(size: 28))
                            .foregroundStyle(.tertiary)
                    }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                if isPaused {
                    RoundedRectangle(cornerRadius: 12)
                        .fill(.black.opacity(0.4))
                    Image(systemName: "pause.fill")
                        .font(.system(size: 36))
                        .foregroundStyle(.white)
                }
            }
            .accessibilityLabel(isPaused ? "Paused" : "")
    }

    // MARK: Recent

    private var recent: some View {
        Section {
            if model.recentScrobbles.isEmpty {
                Text("Tracks appear here once you’ve listened to half of them, or four minutes.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(Array(model.recentScrobbles.enumerated()), id: \.offset) { _, record in
                    RecentScrobbleRow(record: record, artworkSize: 56, titleFont: .body.weight(.medium), detailFont: .subheadline)
                        .padding(.vertical, 4)
                }
            }
        } header: {
            HStack {
                Text("Recent scrobbles")
                Spacer()
                if model.pendingCount > 0 {
                    Text("\(model.pendingCount) waiting to send")
                        .monospacedDigit()
                }
            }
        }
    }

    // MARK: Account

    private var account: some View {
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
                Button("Open Approval Page Again", action: model.reopenApprovalPage)
                Button("Cancel Sign-In", role: .cancel, action: model.cancelSignIn)
            case .signedIn(let username):
                LabeledContent {
                    if model.isSending { ProgressView() }
                } label: {
                    Label(username, systemImage: "person.crop.circle")
                }
                Button("Send Now", action: model.flush)
                    .disabled(model.isSending || model.pendingCount == 0)
                Button("Sign Out", role: .destructive, action: model.signOut)
            }
            if let notice = model.accountNotice ?? model.setupFailure {
                Text(notice)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
