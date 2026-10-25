import Foundation
import MediaPlayer
import ScrovaKit

/// Reads play counts from the media library so Rust can queue the plays that
/// happened while Scrova was not running. Only library songs carry them;
/// streamed tracks outside the library cannot be recovered this way.
enum LibraryHistory {
    /// Plays older than this cannot be scrobbled anyway (Last.fm rejects them),
    /// so after the baseline there is no point in sending those songs.
    private static let lookback: TimeInterval = 15 * 24 * 60 * 60

    /// The first scan sends every song to record the baseline; later scans only
    /// send songs played since then, which keeps large libraries cheap.
    static func items(baselineAt: Date?, now: Date = Date()) -> [ScrovaKit.LibraryItem] {
        let since = baselineAt.map { max($0, now.addingTimeInterval(-lookback)) }
        let songs = MPMediaQuery.songs().items ?? []
        return songs.compactMap { item in
            if let since {
                guard let lastPlayed = item.lastPlayedDate, lastPlayed >= since else { return nil }
            }
            guard let track = NowPlayingObserver.track(from: item), track.sourceTrackId != nil else {
                return nil
            }
            return ScrovaKit.LibraryItem(track: track, playCount: UInt64(item.playCount), lastPlayedAt: item.lastPlayedDate)
        }
    }
}
