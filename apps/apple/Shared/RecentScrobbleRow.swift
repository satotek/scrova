import ScrovaKit
import SwiftUI

/// One entry of the local scrobble log, shown in full: long titles wrap
/// instead of truncating, since the log is where users check what counted.
/// Delivery status only appears when it needs attention.
struct RecentScrobbleRow: View {
    let record: ScrobbleRecord
    var artworkSize: CGFloat = 44
    var titleFont: Font = .callout.weight(.medium)
    var detailFont: Font = .caption
    /// Set where the row has to fit a fixed space (Now playing); the album is
    /// dropped and text truncates there, while the history keeps full text.
    var lineLimit: Int? = nil

    @State private var artwork: CGImage?

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            cover
            VStack(alignment: .leading, spacing: 2) {
                Text(record.track.title)
                    .font(titleFont)
                    .lineLimit(lineLimit)
                    .fixedSize(horizontal: false, vertical: lineLimit == nil)
                if !record.track.artist.isEmpty {
                    Text(record.track.artist)
                        .font(detailFont)
                        .foregroundStyle(.secondary)
                        .lineLimit(lineLimit)
                    .fixedSize(horizontal: false, vertical: lineLimit == nil)
                }
                if lineLimit == nil, let album = record.track.album, !album.isEmpty {
                    Text(album)
                        .font(detailFont)
                        .foregroundStyle(.tertiary)
                        .lineLimit(lineLimit)
                    .fixedSize(horizontal: false, vertical: lineLimit == nil)
                }
                HStack(spacing: 6) {
                    Text(record.startedAt, format: .relative(presentation: .numeric, unitsStyle: .abbreviated))
                        .foregroundStyle(.tertiary)
                    if record.timestampAccuracy == .estimated {
                        Text("Time estimated")
                            .foregroundStyle(.tertiary)
                    }
                    if let status {
                        Text(status.text)
                            .foregroundStyle(status.color)
                    }
                }
                .font(detailFont)
                .padding(.top, 2)
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
        .task(id: record.track) {
            artwork = ArtworkCache.image(for: record.track)
        }
    }

    private var cover: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6).fill(.quaternary)
            if let artwork {
                Image(decorative: artwork, scale: 1)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            } else {
                Image(systemName: "music.note")
                    .foregroundStyle(.tertiary)
            }
        }
        .frame(width: artworkSize, height: artworkSize)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private var status: (text: String, color: Color)? {
        switch record.status {
        case .pending, .sending: ("Waiting to send", .secondary)
        case .submitted: nil
        case .ignored: ("Ignored by Last.fm", .orange)
        case .failed: ("Not sent", .red)
        }
    }
}
