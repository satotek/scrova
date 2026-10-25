import Foundation
import Testing
@testable import ScrovaKit

/// Smoke tests for the Swift ⇄ Rust boundary. Scrobble rules themselves are
/// covered by the Rust tests; these only check that values survive the trip.
struct EngineBridgeTests {
    let start = Date(timeIntervalSince1970: 1_700_000_000)
    let track = Track(
        title: "Title",
        artist: "Artist",
        album: "Album",
        albumArtist: nil,
        duration: 200,
        sourceTrackId: "ABCDEF0123456789"
    )

    func event(position: TimeInterval) -> PlaybackEvent {
        PlaybackEvent(
            track: track,
            state: .playing,
            position: position,
            source: "com.apple.Music",
            observedAt: start.addingTimeInterval(position)
        )
    }

    @Test func coreVersionIsExposed() {
        #expect(!coreVersion().isEmpty)
    }

    func makeScrova() throws -> Scrova {
        let database = FileManager.default.temporaryDirectory
            .appending(path: "scrova-test-\(UUID().uuidString).sqlite")
        return try Scrova(config: ScrovaConfig(
            databasePath: database.path(percentEncoded: false),
            lastfmApiKey: "key",
            lastfmSharedSecret: "secret"
        ))
    }

    @Test func playbackProducesNowPlayingThenScrobble() throws {
        let scrova = try makeScrova()

        let first = try scrova.handlePlaybackEvent(event: event(position: 0))
        guard case .nowPlaying(let playing, let source) = try #require(first.first) else {
            Issue.record("expected NowPlaying, got \(first)")
            return
        }
        #expect(playing == track)
        #expect(source == "com.apple.Music")

        let outputs = try stride(from: 5.0, through: 100, by: 5).flatMap {
            try scrova.handlePlaybackEvent(event: event(position: $0))
        }
        let scrobbles = outputs.compactMap { output -> Scrobble? in
            if case .scrobble(let scrobble) = output { scrobble } else { nil }
        }
        #expect(scrobbles.count == 1)
        #expect(scrobbles.first?.startedAt == start)
        #expect(scrobbles.first?.timestampAccuracy == TimestampAccuracy.observed)

        // The scrobble was persisted before handlePlaybackEvent returned.
        #expect(try scrova.pendingCount() == 1)
        #expect(try scrova.recentScrobbles(limit: 1).first?.status == .pending)
    }

    @Test func flushWithoutSessionThrowsNotSignedIn() async throws {
        let scrova = try makeScrova()
        await #expect(throws: ScrovaError.NotSignedIn) {
            try await scrova.flush()
        }
    }
}
