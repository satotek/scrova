use std::fmt;
use std::time::{Duration, SystemTime};

/// Application credentials issued by Last.fm.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub api_key: String,
    pub shared_secret: String,
}

impl Credentials {
    #[must_use]
    pub fn new(api_key: impl Into<String>, shared_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            shared_secret: shared_secret.into(),
        }
    }
}

// The shared secret is a signing key, not a user-facing value. Keep it out of
// logs even when a caller derives Debug on a containing type.
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("api_key", &self.api_key)
            .field("shared_secret", &"[REDACTED]")
            .finish()
    }
}

/// A web-service session returned by `auth.getSession`.
#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub key: String,
    pub subscriber: bool,
}

impl Session {
    #[must_use]
    pub fn new(name: impl Into<String>, key: impl Into<String>, subscriber: bool) -> Self {
        Self {
            name: name.into(),
            key: key.into(),
            subscriber,
        }
    }
}

// Session keys grant write access to a user's account until revoked.
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("name", &self.name)
            .field("key", &"[REDACTED]")
            .field("subscriber", &self.subscriber)
            .finish()
    }
}

/// The currently playing track, sent by `track.updateNowPlaying`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NowPlaying {
    pub artist: String,
    pub track: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration: Option<Duration>,
    pub track_number: Option<u32>,
}

impl NowPlaying {
    #[must_use]
    pub fn new(artist: impl Into<String>, track: impl Into<String>) -> Self {
        Self {
            artist: artist.into(),
            track: track.into(),
            ..Self::default()
        }
    }
}

/// A single scrobble to submit in a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrobbleRequest {
    pub artist: String,
    pub track: String,
    /// The time playback started, not the time the scrobble is sent.
    pub timestamp: SystemTime,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration: Option<Duration>,
    pub track_number: Option<u32>,
}

impl ScrobbleRequest {
    #[must_use]
    pub fn new(artist: impl Into<String>, track: impl Into<String>, timestamp: SystemTime) -> Self {
        Self {
            artist: artist.into(),
            track: track.into(),
            timestamp,
            album: None,
            album_artist: None,
            duration: None,
            track_number: None,
        }
    }
}

/// Query parameters for `user.getRecentTracks`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentTracksQuery {
    /// One-based page number. Last.fm defaults to the first page.
    pub page: Option<u32>,
    /// Page size. Last.fm defaults to 50 and caps this at 200.
    pub limit: Option<u32>,
    /// Exclusive lower bound on the scrobble timestamp.
    pub from: Option<SystemTime>,
    /// Exclusive upper bound on the scrobble timestamp.
    pub to: Option<SystemTime>,
}

/// One page of a user's recent tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentTracksPage {
    pub user: String,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
    pub tracks: Vec<RecentTrack>,
}

/// A track returned by `user.getRecentTracks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentTrack {
    pub artist: String,
    pub artist_mbid: Option<String>,
    pub track: String,
    pub album: Option<String>,
    pub album_mbid: Option<String>,
    pub mbid: Option<String>,
    pub url: Option<String>,
    pub loved: bool,
    pub now_playing: bool,
    pub played_at: Option<SystemTime>,
}

/// Last.fm's automatic spelling corrections for an accepted scrobble.
///
/// A `Some` value means Last.fm replaced the submitted value. Callers should
/// not apply these automatically; the API docs require explicit user approval.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Corrections {
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
}

impl Corrections {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.track.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.album_artist.is_none()
    }
}

/// Per-scrobble result returned by `track.scrobble`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrobbleOutcome {
    Accepted { corrected: Corrections },
    Ignored { code: u32, message: String },
}
