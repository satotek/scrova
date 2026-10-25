use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration: Option<Duration>,
    /// Player-specific stable ID (e.g. Music.app persistent ID). Only meaningful
    /// within the same `source`.
    pub source_track_id: Option<String>,
}

impl Track {
    /// Last.fm requires both an artist and a track name; streams and radio
    /// sometimes provide neither.
    pub fn is_submittable(&self) -> bool {
        !self.title.trim().is_empty() && !self.artist.trim().is_empty()
    }

    /// Identity used to decide whether two observations refer to the same track.
    /// Falls back to metadata when the player gives no stable ID.
    pub(crate) fn same_track_as(&self, other: &Track) -> bool {
        match (&self.source_track_id, &other.source_track_id) {
            (Some(a), Some(b)) => a == b,
            // Album and duration are left out: players fill metadata in
            // stages (title first, album and length a moment later), and
            // treating each stage as a new track splits one play in several.
            _ => self.title == other.title && self.artist == other.artist,
        }
    }
}

/// A snapshot of a player's state, produced by a native observer.
///
/// Observers may emit these on every notification, on a polling timer, or both;
/// the engine only relies on `observed_at` and `position`, not on how often
/// events arrive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PlaybackEvent {
    /// `None` when the player has nothing loaded.
    pub track: Option<Track>,
    pub state: PlaybackState,
    /// Playhead position at `observed_at`, if the player exposes it.
    pub position: Option<Duration>,
    /// Identifier of the player app, e.g. a bundle ID such as `com.apple.Music`.
    pub source: String,
    pub observed_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum TimestampAccuracy {
    /// Derived from live observation of the playback start.
    Observed,
    /// Reconstructed after the fact (e.g. from a history API without play dates).
    /// Must never be presented or deduplicated as if it were exact.
    Estimated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ScrobbleOrigin {
    LiveObservation,
    HistoryRecovery,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Scrobble {
    pub track: Track,
    pub started_at: SystemTime,
    pub timestamp_accuracy: TimestampAccuracy,
    pub origin: ScrobbleOrigin,
    pub source: String,
    /// Engine-generated ID of the listening session this scrobble came from.
    /// Stable across retries, so it doubles as an idempotency key for the queue.
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum EngineOutput {
    NowPlaying { track: Track, source: String },
    Scrobble { scrobble: Scrobble },
}

/// How far the current listening session is from being scrobbled, for UIs
/// that want to show it. Not needed to drive the engine.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SessionProgress {
    pub track: Track,
    pub state: PlaybackState,
    /// Listening time credited as of `observed_at`.
    pub played: Duration,
    /// `None` when the track will never be scrobbled (too short, or missing
    /// title/artist).
    pub threshold: Option<Duration>,
    pub scrobbled: bool,
    /// Playhead at `observed_at`, if the player reports one.
    pub position: Option<Duration>,
    /// When `played` was last updated; while `state` is `Playing`, callers can
    /// extrapolate from here between observations.
    pub observed_at: SystemTime,
}

/// One song in a native media-library snapshot.
///
/// iOS cannot observe playback in the background, so the native layer compares
/// these snapshots over time and lets Rust reconstruct plays it missed.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct LibraryItem {
    /// `track.source_track_id` must be a stable player-specific ID.
    pub track: Track,
    pub play_count: u64,
    pub last_played_at: Option<SystemTime>,
}
