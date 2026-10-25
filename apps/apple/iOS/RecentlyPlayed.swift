import Foundation
import MediaPlayer
import MusicKit
import ScrovaKit

/// MusicKit "recently played" gives Scrova a timestamp-free list of the songs
/// the user streamed, including songs outside their library. It is intentionally
/// fetched only after MusicKit authorization is granted from Settings.
enum RecentlyPlayed {
    static let limit = 30

    static var authorizationStatus: MusicAuthorization.Status {
        MusicAuthorization.currentStatus
    }

    static func requestAuthorization() async -> MusicAuthorization.Status {
        await MusicAuthorization.request()
    }

    /// Newest first. The server may return fewer than `limit` items.
    static func fetch(limit: Int = limit) async throws -> [ScrovaKit.Track] {
        var request = MusicRecentlyPlayedRequest<Song>()
        request.limit = limit
        let response = try await request.response()
        return response.items.compactMap(track(from:))
    }

    /// Catalog IDs are stable across launches, unlike a library persistent ID.
    /// The recovery planner builds its own normalized key, so this is only for
    /// diagnostics and display.
    static func track(from song: Song) -> ScrovaKit.Track? {
        let title = song.title.trimmingCharacters(in: .whitespacesAndNewlines)
        let artist = song.artistName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty, !artist.isEmpty else { return nil }

        let album = song.albumTitle?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let duration = song.duration.flatMap { $0 > 0 ? $0 : nil }

        return ScrovaKit.Track(
            title: title,
            artist: artist,
            album: album?.isEmpty == false ? album : nil,
            albumArtist: nil,
            duration: duration,
            sourceTrackId: song.id.rawValue
        )
    }

    /// The system player is the same source NowPlayingObserver reads, so the
    /// recovery hint and the live engine agree on title/artist matching.
    @MainActor
    static func nowPlayingHint() -> (track: ScrovaKit.Track?, position: TimeInterval?, isPlaying: Bool) {
        let player = MPMusicPlayerController.systemMusicPlayer
        let position = player.currentPlaybackTime
        return (
            NowPlayingObserver.track(from: player.nowPlayingItem),
            position >= 0 ? position : nil,
            player.playbackState == .playing
        )
    }
}
