use std::time::SystemTime;

use scrova_core::{ScrobbleOrigin, TimestampAccuracy, Track};
use scrova_lastfm::ScrobbleRequest;
use scrova_storage::{DeliveryState, QueueItem};

#[derive(Debug, Clone, uniffi::Record)]
pub struct ScrovaConfig {
    pub database_path: String,
    pub lastfm_api_key: String,
    pub lastfm_shared_secret: String,
    /// Overrides the API root; only meant for tests against a mock server.
    #[uniffi(default = None)]
    pub lastfm_base_url: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AuthRequest {
    pub token: String,
    pub url: String,
}

/// Returned once so the native side can put the key in secure storage.
#[derive(Clone, uniffi::Record)]
pub struct LastfmAccount {
    pub username: String,
    pub session_key: String,
    pub subscriber: bool,
}

impl std::fmt::Debug for LastfmAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LastfmAccount")
            .field("username", &self.username)
            .field("session_key", &"[REDACTED]")
            .field("subscriber", &self.subscriber)
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct FlushReport {
    pub submitted: u32,
    pub ignored: u32,
    pub failed: u32,
    pub retry_scheduled: u32,
    pub remaining: u64,
}

/// Result of comparing one media-library snapshot with the stored baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct RecoveryReport {
    pub baseline_created: bool,
    pub queued: u32,
    pub already_covered: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum DeliveryStatus {
    Pending,
    Sending,
    Submitted,
    Ignored { code: i64, message: String },
    Failed { message: String },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ScrobbleRecord {
    pub track: Track,
    pub started_at: SystemTime,
    pub timestamp_accuracy: TimestampAccuracy,
    pub origin: ScrobbleOrigin,
    pub status: DeliveryStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Opaque page cursor. The UI only stores `HistoryPage.next` and sends it back.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HistoryCursor {
    pub started_at: SystemTime,
    pub id: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HistoryPage {
    pub items: Vec<ScrobbleRecord>,
    /// `None` when there are no older scrobbles.
    pub next: Option<HistoryCursor>,
}

impl From<QueueItem> for ScrobbleRecord {
    fn from(item: QueueItem) -> Self {
        let status = match item.state {
            DeliveryState::Pending => DeliveryStatus::Pending,
            DeliveryState::InFlight => DeliveryStatus::Sending,
            DeliveryState::Submitted => DeliveryStatus::Submitted,
            DeliveryState::Ignored { code, message } => DeliveryStatus::Ignored { code, message },
            DeliveryState::Failed { message, .. } => DeliveryStatus::Failed { message },
        };
        let timestamp_accuracy = item.scrobble.timestamp_accuracy;
        let origin = item.scrobble.origin;
        Self {
            track: item.scrobble.track,
            started_at: item.scrobble.started_at,
            timestamp_accuracy,
            origin,
            status,
            attempts: item.attempts,
            last_error: item.last_error,
        }
    }
}

pub(crate) fn scrobble_request(item: &QueueItem) -> ScrobbleRequest {
    let track = &item.scrobble.track;
    ScrobbleRequest {
        artist: track.artist.clone(),
        track: track.title.clone(),
        timestamp: item.scrobble.started_at,
        album: track.album.clone(),
        album_artist: track.album_artist.clone(),
        duration: track.duration,
        track_number: None,
    }
}
