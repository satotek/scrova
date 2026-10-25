import ScrovaKit
import SwiftUI
import UIKit

struct NowPlayingView: View {
    @Bindable var model: AppModel
    let openSettings: () -> Void
    let openHistory: () -> Void

    @Environment(\.openURL) private var openURL

    var body: some View {
        NavigationStack {
            // A fixed layout: this is the screen people glance at while
            // listening, so everything fits at once and the artwork gives way
            // to the text instead of pushing it off screen.
            VStack(alignment: .leading, spacing: 16) {
                if model.libraryDenied {
                    permissionCard
                } else {
                    if case .signedOut = model.account {
                        signedOutNotice
                    }
                    nowPlaying
                }
                // Lowest priority, so leftover height lands here rather than
                // around the artwork, keeping the artwork and its text together.
                Spacer(minLength: 0)
                    .layoutPriority(-1)
                if let latest = model.recentScrobbles.first {
                    lastScrobbled(latest)
                }
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .navigationTitle("Now playing")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    private func lastScrobbled(_ record: ScrobbleRecord) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Last scrobbled")
                .font(.footnote)
                .foregroundStyle(.secondary)
            Button(action: openHistory) {
                RecentScrobbleRow(
                    record: record,
                    artworkSize: 40,
                    titleFont: .body.weight(.medium),
                    detailFont: .caption,
                    lineLimit: 1
                )
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
            }
            .buttonStyle(.plain)
        }
    }

    private var permissionCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Scrova can’t see Music yet")
                .font(.headline)
            Text("Allow access to Apple Music and your media library in Settings so Scrova can read what is playing.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Button("Open Settings", action: openAppSettings)
        }
    }

    private var signedOutNotice: some View {
        Button(action: openSettings) {
            Text("Sign in to Last.fm in Settings to send scrobbles.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(.plain)
    }

    // MARK: Now playing

    @ViewBuilder
    private var nowPlaying: some View {
        VStack(alignment: .leading, spacing: 18) {
            artwork
                .frame(maxWidth: .infinity)
            VStack(alignment: .leading, spacing: 4) {
                if let track = model.currentTrack {
                    Text(track.title)
                        .font(.title2.weight(.semibold))
                        .lineLimit(2)
                    if !track.artist.isEmpty {
                        Text(track.artist)
                            .font(.body)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    if let album = track.album, !album.isEmpty {
                        Text(album)
                            .font(.subheadline)
                            .foregroundStyle(.tertiary)
                            .lineLimit(1)
                    }
                } else {
                    Text("Nothing playing")
                        .font(.title2.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text("Play something in Music, then come back. Scrova only counts while it is open.")
                        .font(.subheadline)
                        .foregroundStyle(.tertiary)
                }
            }
            .layoutPriority(1)
            if let progress = model.progress, model.currentTrack != nil {
                ScrobbleMeter(progress: progress, tint: model.tint)
                    .layoutPriority(1)
            }
        }
    }

    private var artwork: some View {
        let isPaused = model.playbackState == .paused && model.currentTrack != nil
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

    private func openAppSettings() {
        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
        openURL(url)
    }
}
