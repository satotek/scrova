import Foundation
import MediaPlayer
import UIKit
import ScrovaKit

/// Reads the system Music player. Apple Events are not available to an iOS app,
/// so `MPMusicPlayerController.systemMusicPlayer` is the documented source.
@MainActor
final class NowPlayingObserver {
    static let source = "com.apple.Music"

    var onEvent: ((PlaybackEvent) -> Void)?
    /// Called when the item changes; `nil` when it has no artwork.
    var onArtwork: ((UIImage?) -> Void)?

    private let player = MPMusicPlayerController.systemMusicPlayer
    private var timer: Timer?
    private var tokens: [NSObjectProtocol] = []
    /// A background launch (the refresh task) must not sample playback. The
    /// process is about to be suspended again, and a playing snapshot with no
    /// following pause would bill that gap as listening.
    private var foreground = false
    /// What was playing when we were backgrounded, held back until we know
    /// whether the same track is still playing on return.
    private var leftWhilePlaying: PlaybackEvent?
    private var artworkItemID: MPMediaEntityPersistentID?
    private var artworkChangedAt = Date.distantPast
    private static let artworkSettleTime: TimeInterval = 15

    var authorizationStatus: MPMediaLibraryAuthorizationStatus {
        MPMediaLibrary.authorizationStatus()
    }

    func start() {
        guard tokens.isEmpty else { return }

        let center = NotificationCenter.default
        // The notification object is the player that changed. beginGenerating
        // is required or these are never posted.
        observe(center, .MPMusicPlayerControllerNowPlayingItemDidChange)
        observe(center, .MPMusicPlayerControllerPlaybackStateDidChange)
        player.beginGeneratingPlaybackNotifications()
    }

    func stop() {
        stopPolling()
        let center = NotificationCenter.default
        for token in tokens {
            center.removeObserver(token)
        }
        tokens.removeAll()
        player.endGeneratingPlaybackNotifications()
    }

    /// Shows the media-library prompt when the status is still not determined.
    /// `NSAppleMusicUsageDescription` is what the system puts in that dialog.
    func requestAuthorizationIfNeeded() async -> MPMediaLibraryAuthorizationStatus {
        let current = MPMediaLibrary.authorizationStatus()
        guard current == .notDetermined else { return current }
        return await withCheckedContinuation { continuation in
            MPMediaLibrary.requestAuthorization { status in
                continuation.resume(returning: status)
            }
        }
    }

    func setForeground(_ isForeground: Bool) {
        let wasForeground = foreground
        foreground = isForeground
        if isForeground {
            if !wasForeground { reconcileTimeAway() }
            startPolling()
            poll()
        } else if wasForeground {
            stopPolling()
            leftWhilePlaying = makeEvent(forcingPause: true)
        }
    }

    /// Music keeps playing after this process backgrounds, typically because
    /// the user went to the Music app. We see nothing until we return, so the
    /// gap is settled then, from the playhead:
    ///
    /// - Same track: send nothing extra. The engine credits the gap as
    ///   `min(playhead advance, wall time)`, so time spent paused in Music
    ///   (playhead still) is not counted.
    /// - Different track or nothing playing: we cannot tell whether the old
    ///   track finished or was skipped, so it is closed as paused at the moment
    ///   we left. Library history recovery picks it up if it was played through.
    private func reconcileTimeAway() {
        defer { leftWhilePlaying = nil }
        guard let left = leftWhilePlaying, let leftTrack = left.track else { return }
        let current = Self.track(from: player.nowPlayingItem)
        let sameTrack = current.map { Self.isSameTrack($0, leftTrack) } ?? false
        if !sameTrack || player.playbackState == .stopped {
            onEvent?(left)
        }
    }

    private static func isSameTrack(_ a: Track, _ b: Track) -> Bool {
        if let idA = a.sourceTrackId, let idB = b.sourceTrackId { return idA == idB }
        return a.title == b.title && a.artist == b.artist
    }

    func poll() {
        guard foreground else { return }
        onEvent?(makeEvent(forcingPause: false))
        updateArtwork()
    }

    /// Rendering artwork is not free, so it settles once per item. Streamed
    /// items load their artwork after the track changes, and until then Music
    /// can hand back the previous track's image, so it is read again for a
    /// short while after each change.
    private func updateArtwork() {
        let item = player.nowPlayingItem
        let now = Date()
        if item?.persistentID != artworkItemID {
            artworkItemID = item?.persistentID
            artworkChangedAt = now
        } else if now.timeIntervalSince(artworkChangedAt) > Self.artworkSettleTime {
            return
        }
        // 600pt covers the largest size the app draws at 3x without upscaling.
        onArtwork?(item?.artwork?.image(at: CGSize(width: 600, height: 600)))
    }

    private func makeEvent(forcingPause: Bool) -> PlaybackEvent {
        let item = player.nowPlayingItem
        let state = mappedState(forcingPause: forcingPause)
        let track = state == .stopped ? nil : Self.track(from: item)
        let position = track == nil ? nil : positionSeconds()
        return PlaybackEvent(
            track: track,
            state: track == nil && state != .paused ? .stopped : state,
            position: position,
            source: Self.source,
            observedAt: Date()
        )
    }

    private func mappedState(forcingPause: Bool) -> PlaybackState {
        if forcingPause, player.playbackState != .stopped, player.nowPlayingItem != nil {
            return .paused
        }
        switch player.playbackState {
        case .playing:
            return .playing
        case .paused, .interrupted, .seekingForward, .seekingBackward:
            // Seeking and interruptions are not listening, same as scrubbing on macOS.
            return .paused
        case .stopped:
            return .stopped
        @unknown default:
            return .stopped
        }
    }

    /// Shared with library history recovery so both produce identical tracks.
    nonisolated static func track(from item: MPMediaItem?) -> Track? {
        guard let item else { return nil }
        let title = item.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !title.isEmpty else { return nil }
        let artist = item.artist?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let album = item.albumTitle?.trimmingCharacters(in: .whitespacesAndNewlines)
        let albumArtist = item.albumArtist?.trimmingCharacters(in: .whitespacesAndNewlines)
        let duration = item.playbackDuration > 0 ? item.playbackDuration : nil
        return Track(
            title: title,
            artist: artist,
            album: album?.isEmpty == false ? album : nil,
            albumArtist: albumArtist?.isEmpty == false ? albumArtist : nil,
            duration: duration,
            sourceTrackId: Self.hexID(item.persistentID)
        )
    }

    /// AppleScript's persistent ID is a hex string. Match that so the same
    /// library track has one `sourceTrackId` on both platforms.
    nonisolated private static func hexID(_ id: MPMediaEntityPersistentID) -> String? {
        guard id != 0 else { return nil }
        return String(format: "%016llX", id)
    }

    /// `currentPlaybackTime` lives on `MPMediaPlayback`, which the music player adopts.
    /// Negative means nothing is loaded.
    private func positionSeconds() -> TimeInterval? {
        let position = player.currentPlaybackTime
        guard position >= 0 else { return nil }
        return position
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name) {
        let token = center.addObserver(forName: name, object: player, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.poll() }
        }
        tokens.append(token)
    }

    private func startPolling() {
        guard timer == nil else { return }
        // Same 3s cap as macOS: under the engine's 10s repeat window.
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.poll() }
        }
    }

    private func stopPolling() {
        timer?.invalidate()
        timer = nil
    }
}
