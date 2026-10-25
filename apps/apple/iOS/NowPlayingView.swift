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
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .navigationTitle("Now playing")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    @ViewBuilder
    private var latest: some View {
        if let record = model.recentScrobbles.first {
            lastScrobbled(record)
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

    private var nowPlaying: some View {
        GeometryReader { proxy in
            // Side by side when the space is clearly wider than tall (iPad in
            // landscape): stacked, the square artwork would leave the text a
            // sliver at the bottom.
            if model.currentTrack != nil, proxy.size.width > proxy.size.height * 1.2 {
                HStack(alignment: .center, spacing: 32) {
                    artwork
                    // The last scrobble follows the meter in the same column,
                    // rather than stretching across the bottom under both.
                    VStack(alignment: .leading, spacing: 18) {
                        trackText
                        meter
                        // An iPhone on its side has no height to spare; there
                        // the card would be cut off, and History is a tap away.
                        if proxy.size.height >= 320 {
                            latest
                                .padding(.top, 8)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                VStack(alignment: .leading, spacing: 18) {
                    artwork
                        .frame(maxWidth: .infinity)
                    trackText
                        .layoutPriority(1)
                    meter
                        .layoutPriority(1)
                    // Lowest priority, so leftover height lands here rather
                    // than around the artwork, keeping it with its text.
                    Spacer(minLength: 0)
                        .layoutPriority(-1)
                    latest
                        .layoutPriority(1)
                }
                .frame(maxHeight: .infinity, alignment: .top)
                // A full-width square on an iPad in portrait would be larger
                // than the text it belongs to warrants.
                .frame(maxWidth: 640)
                .frame(maxWidth: .infinity)
            }
        }
    }

    private var trackText: some View {
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
    }

    @ViewBuilder
    private var meter: some View {
        if let progress = model.progress, model.currentTrack != nil {
            ScrobbleMeter(progress: progress, tint: model.tint)
        }
    }

    private var artwork: some View {
        let isPaused = model.playbackState == .paused && model.currentTrack != nil
        return Color.clear
            .aspectRatio(1, contentMode: .fit)
            .frame(maxWidth: model.currentTrack == nil ? 88 : nil)
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
