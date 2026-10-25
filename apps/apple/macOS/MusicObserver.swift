import AppKit
import ScrovaKit

/// Watches Music.app and reports snapshots as `PlaybackEvent`s.
///
/// Apple Events are the source of truth. The `com.apple.Music.playerInfo`
/// distributed notification is undocumented and may be dropped, so it only
/// triggers an early read; a timer covers everything else. Snapshots are sent
/// on every read because the engine is indifferent to duplicates.
@MainActor
final class MusicObserver {
    static let bundleID = "com.apple.Music"

    enum Failure: Equatable {
        /// The user declined the Automation prompt (errAEEventNotPermitted).
        case notPermitted
        case scriptError(code: Int, message: String)
    }

    var onEvent: (@MainActor (PlaybackEvent) -> Void)?
    var onFailure: (@MainActor (Failure) -> Void)?
    /// Called when the track changes; `nil` when the track has no artwork.
    var onArtwork: (@MainActor (NSImage?) -> Void)?

    private let pollInterval: TimeInterval
    private var timer: Timer?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []
    private var lastTrack: Track?
    private var lastPosition: TimeInterval?
    private var reportedStopped = false
    /// Artwork can be hundreds of kilobytes, so it is read once per track
    /// rather than on every poll.
    private var artworkKey: String?

    // Compiled once; compiling on every poll is the expensive part.
    private lazy var script = Self.compile(Self.source)
    private lazy var artworkScript = Self.compile(Self.artworkSource)

    init(pollInterval: TimeInterval = 3) {
        // Must stay below the engine's repeat_start_window (10s) so a repeated
        // track is seen near its start.
        self.pollInterval = pollInterval
    }

    func start() {
        guard timer == nil else { return }

        let distributed = DistributedNotificationCenter.default()
        observe(distributed, "com.apple.Music.playerInfo") { $0.poll() }

        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.willSleepNotification.rawValue) { $0.reportSleep() }
        observe(workspace, NSWorkspace.didWakeNotification.rawValue) { $0.poll() }
        observe(workspace, NSWorkspace.didTerminateApplicationNotification.rawValue) { observer in
            if !observer.isMusicRunning { observer.reportStopped() }
        }

        timer = Timer.scheduledTimer(withTimeInterval: pollInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.poll() }
        }
        poll()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        for (center, token) in observers { center.removeObserver(token) }
        observers.removeAll()
    }

    func poll() {
        // `tell application "Music"` launches Music if it is not running.
        guard isMusicRunning else {
            reportStopped()
            return
        }
        guard let script else { return }

        var error: NSDictionary?
        let result = script.executeAndReturnError(&error)
        if let error {
            let code = error[NSAppleScript.errorNumber] as? Int ?? 0
            if code == -1743 {
                onFailure?(.notPermitted)
            } else if code != -600 {
                // -600: Music quit between the running check and the event.
                let message = error[NSAppleScript.errorMessage] as? String ?? ""
                onFailure?(.scriptError(code: code, message: message))
            }
            return
        }
        emit(Self.parse(result))
    }

    private var isMusicRunning: Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).isEmpty
    }

    private func emit(_ snapshot: Snapshot) {
        let event = PlaybackEvent(
            track: snapshot.track,
            state: snapshot.state,
            position: snapshot.position,
            source: Self.bundleID,
            observedAt: Date()
        )
        lastTrack = snapshot.track
        lastPosition = snapshot.position
        reportedStopped = snapshot.state == .stopped
        onEvent?(event)
        updateArtwork(for: snapshot.track)
    }

    private func updateArtwork(for track: Track?) {
        let key = track.map { $0.sourceTrackId ?? "\($0.artist)\u{1F}\($0.title)" }
        guard key != artworkKey else { return }
        artworkKey = key
        guard track != nil, let artworkScript else {
            onArtwork?(nil)
            return
        }
        var error: NSDictionary?
        let result = artworkScript.executeAndReturnError(&error)
        // Failures and tracks without artwork look the same to the user.
        onArtwork?(error == nil ? NSImage(data: result.data) : nil)
    }

    /// The machine may sleep without Music reporting a pause; stop the clock
    /// explicitly so sleep time is never counted as listening.
    private func reportSleep() {
        guard let lastTrack, !reportedStopped else { return }
        emit(Snapshot(track: lastTrack, state: .paused, position: lastPosition))
    }

    private func reportStopped() {
        guard !reportedStopped else { return }
        emit(Snapshot(track: nil, state: .stopped, position: nil))
    }

    private func observe(_ center: NotificationCenter, _ name: String, _ handler: @escaping @MainActor (MusicObserver) -> Void) {
        let token = center.addObserver(forName: Notification.Name(name), object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                if let self { handler(self) }
            }
        }
        observers.append((center, token))
    }
}

// MARK: - Script

extension MusicObserver {
    struct Snapshot {
        var track: Track?
        var state: PlaybackState
        var position: TimeInterval?
    }

    // Each property is read in its own `try` because streams and some cloud
    // tracks lack album, duration, etc. Units are seconds (see Music.sdef).
    // Properties are read through `current track` every time: storing it in a
    // variable resolves it to a library/playlist reference, and the sandbox
    // rejects that with a privilege violation (-10004).
    // The result is a flat list so parsing does not depend on record keys.
    static let source = """
    tell application id "com.apple.Music"
        set currentState to player state
        if currentState is playing then
            set stateText to "playing"
        else if currentState is paused then
            set stateText to "paused"
        else if currentState is fast forwarding then
            set stateText to "fast forwarding"
        else if currentState is rewinding then
            set stateText to "rewinding"
        else
            return {"stopped"}
        end if
        set trackID to ""
        set trackName to ""
        set trackArtist to ""
        set trackAlbum to ""
        set trackAlbumArtist to ""
        set trackDuration to -1
        set playerPosition to -1
        try
            set trackID to persistent ID of current track
        end try
        try
            set trackName to name of current track
        end try
        try
            set trackArtist to artist of current track
        end try
        try
            set trackAlbum to album of current track
        end try
        try
            set trackAlbumArtist to album artist of current track
        end try
        try
            set trackDuration to duration of current track
        end try
        if trackName is "" then
            try
                set trackName to current stream title
            end try
        end if
        try
            set playerPosition to player position
        end try
        return {stateText, trackID, trackName, trackArtist, trackAlbum, trackAlbumArtist, trackDuration, playerPosition}
    end tell
    """

    // `raw data` keeps the original JPEG/PNG bytes; `data` would convert to a
    // legacy PICT. Read through `current track` for the same sandbox reason
    // as above.
    static let artworkSource = """
    tell application id "com.apple.Music"
        return raw data of artwork 1 of current track
    end tell
    """

    static func compile(_ source: String) -> NSAppleScript? {
        let script = NSAppleScript(source: source)
        var error: NSDictionary?
        script?.compileAndReturnError(&error)
        return script
    }

    static func parse(_ list: NSAppleEventDescriptor) -> Snapshot {
        func string(_ i: Int) -> String? {
            guard list.numberOfItems >= i, let s = list.atIndex(i)?.stringValue, !s.isEmpty else { return nil }
            return s
        }
        func seconds(_ i: Int) -> TimeInterval? {
            guard list.numberOfItems >= i, let d = list.atIndex(i)?.doubleValue, d >= 0 else { return nil }
            return d
        }

        let state: PlaybackState = switch string(1) {
        case "playing": .playing
        // Scrubbing is not listening.
        case "paused", "fast forwarding", "rewinding": .paused
        default: .stopped
        }

        guard state != .stopped, let title = string(3) else {
            return Snapshot(track: nil, state: .stopped, position: nil)
        }
        let track = Track(
            title: title,
            artist: string(4) ?? "",
            album: string(5),
            albumArtist: string(6),
            duration: seconds(7),
            sourceTrackId: string(2)
        )
        return Snapshot(track: track, state: state, position: seconds(8))
    }
}
