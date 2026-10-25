//! UniFFI surface for Swift (Apple) and Kotlin (Android).
//!
//! This layer wires the engine, the queue and the Last.fm client together and
//! owns synchronisation and object lifetimes. Scrobble rules, persistence and
//! the wire protocol stay in their own crates, because Windows will need a
//! separate C ABI layer and nothing here should be the only way to reach them.
//!
//! The native app drives all network activity (`update_now_playing`, `flush`)
//! instead of a background task inside Rust: iOS only grants execution time to
//! the app itself, so the app must decide when sending is possible.

mod error;
mod records;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_core::{
    EngineConfig, EngineOutput, LibraryItem, LibraryState, PlaybackEvent, RecoveryConfig, Scrobble,
    ScrobbleEngine, ScrobbleOrigin, SessionProgress, TimestampAccuracy, Track, plan_recovery,
};
use scrova_lastfm::{
    Credentials, LastfmClient, LastfmError, NowPlaying, RetryClass, ScrobbleOutcome,
};
use scrova_storage::{
    HistoryCursor as StorageHistoryCursor, HistoryPage as StorageHistoryPage, LASTFM_DESTINATION,
    LibraryScan, QueueId, QueueItem, Store,
};

pub use error::ScrovaError;
pub use records::{
    AuthRequest, DeliveryStatus, FlushReport, HistoryCursor, HistoryPage, LastfmAccount,
    RecoveryReport, ScrobbleRecord, ScrovaConfig,
};

uniffi::setup_scaffolding!();

/// Last.fm's documented batch limit for `track.scrobble`.
const BATCH_SIZE: usize = 50;
/// Floor for the delay after a rate-limit response (error 29), on top of the
/// regular exponential backoff.
const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);
/// Retry cadence while the API key is suspended (error 26). Nothing we do will
/// fix it, so only check back occasionally.
const SUSPENDED_BACKOFF: Duration = Duration::from_secs(60 * 60);
/// Start times of one play estimated from two observations differ by polling
/// jitter and rounding, far less than this; two real plays of a track cannot
/// start this close together.
const RESTART_DUPLICATE_WINDOW: Duration = Duration::from_secs(30);

#[derive(uniffi::Object)]
pub struct Scrova {
    // Observers on Apple platforms call in from arbitrary threads/queues.
    engine: Mutex<ScrobbleEngine>,
    store: Store,
    lastfm: LastfmClient,
    // Owned by the native secure storage; Rust only keeps it in memory.
    session_key: Mutex<Option<String>>,
    // Two overlapping flushes would claim disjoint batches and could send them
    // out of chronological order.
    flush_lock: tokio::sync::Mutex<()>,
}

#[uniffi::export(async_runtime = "tokio")]
impl Scrova {
    #[uniffi::constructor]
    pub fn new(config: ScrovaConfig) -> Result<Arc<Self>, ScrovaError> {
        let credentials = Credentials::new(config.lastfm_api_key, config.lastfm_shared_secret);
        let lastfm = match config.lastfm_base_url {
            Some(url) => LastfmClient::with_base_url(credentials, url),
            None => LastfmClient::new(credentials),
        };
        Self::with_parts(Store::open(&config.database_path)?, lastfm)
    }

    pub fn set_session_key(&self, session_key: Option<String>) {
        *lock(&self.session_key) = session_key;
    }

    pub fn is_signed_in(&self) -> bool {
        lock(&self.session_key).is_some()
    }

    /// Feeds one observation to the engine. Scrobbles are persisted before this
    /// returns, so they survive even if the app is killed right after.
    pub fn handle_playback_event(
        &self,
        event: PlaybackEvent,
    ) -> Result<Vec<EngineOutput>, ScrovaError> {
        let outputs = lock(&self.engine).handle(event);
        for output in &outputs {
            if let EngineOutput::Scrobble { scrobble } = output
                && !self.already_scrobbled(scrobble)?
            {
                self.store.enqueue(scrobble)?;
            }
        }
        Ok(outputs)
    }

    pub fn session_progress(&self, source: String) -> Option<SessionProgress> {
        lock(&self.engine).progress(&source)
    }

    /// Compares one media-library snapshot with the stored baseline and queues
    /// plays that happened while the app could not observe playback.
    ///
    /// The first scan only establishes the baseline; it never replays the
    /// user's lifetime of plays.
    pub fn recover_library_history(
        &self,
        source: String,
        items: Vec<LibraryItem>,
    ) -> Result<RecoveryReport, ScrovaError> {
        let now = SystemTime::now();

        // A stable track ID is the only reliable join key across scans.
        let mut by_id: HashMap<String, LibraryItem> = HashMap::new();
        for item in items {
            if let Some(track_id) = item.track.source_track_id.as_deref()
                && !track_id.is_empty()
            {
                by_id.insert(track_id.to_owned(), item);
            }
        }
        let items: Vec<LibraryItem> = by_id.into_values().collect();
        let track_ids: Vec<String> = items
            .iter()
            .filter_map(|item| item.track.source_track_id.clone())
            .collect();

        let Some(baseline_at) = self.store.library_baseline(&source)? else {
            let states = items.iter().filter_map(library_state).collect::<Vec<_>>();
            self.store.apply_library_scan(
                &source,
                LibraryScan {
                    baseline: Some(now),
                    states: &states,
                    scrobbles: &[],
                },
            )?;
            return Ok(RecoveryReport {
                baseline_created: true,
                ..RecoveryReport::default()
            });
        };

        let previous = self.store.library_states(&source, &track_ids)?;
        let config = RecoveryConfig::default();
        let mut states = Vec::with_capacity(items.len());
        let mut recovered = Vec::new();
        let mut already_covered = 0_u32;

        for item in &items {
            let Some(track_id) = item.track.source_track_id.as_deref() else {
                continue;
            };
            let previous_state = previous.get(track_id);

            // Store the new snapshot even when no play can be placed. Otherwise
            // the same delta would be reconsidered on every scan.
            states.push((
                track_id.to_owned(),
                LibraryState {
                    play_count: item.play_count,
                    last_played_at: item.last_played_at,
                },
            ));

            let Some((from, to)) = recovery_query_range(item, previous_state, baseline_at, &config)
            else {
                continue;
            };
            let existing = self.store.existing_scrobble_starts(track_id, from, to)?;
            let plan = plan_recovery(item, previous_state, baseline_at, &existing, now, &config);
            already_covered = already_covered.saturating_add(plan.already_covered);

            for start in plan.starts {
                let Some(start_ms) = start
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|duration| u64::try_from(duration.as_millis()).ok())
                else {
                    continue;
                };
                recovered.push(Scrobble {
                    track: item.track.clone(),
                    started_at: start,
                    timestamp_accuracy: TimestampAccuracy::Estimated,
                    origin: ScrobbleOrigin::HistoryRecovery,
                    source: source.clone(),
                    // Deterministic, so rerunning the same scan hits the
                    // scrobbles.session_id UNIQUE constraint instead of
                    // duplicating a listen.
                    session_id: format!("recovery:{source}:{track_id}:{start_ms}"),
                });
            }
        }

        // Keep the newest work when a long gap produces more than one call can
        // reasonably submit.
        recovered.sort_by_key(|scrobble| std::cmp::Reverse(scrobble.started_at));
        let mut seen = HashSet::new();
        recovered.retain(|scrobble| seen.insert(scrobble.session_id.clone()));
        recovered.truncate(config.max_scrobbles);

        let result = self.store.apply_library_scan(
            &source,
            LibraryScan {
                baseline: None,
                states: &states,
                scrobbles: &recovered,
            },
        )?;

        Ok(RecoveryReport {
            baseline_created: false,
            queued: result.queued,
            already_covered: already_covered.saturating_add(result.already_queued),
        })
    }

    /// Returns the first scan time for a source. The native side uses it to
    /// send only recently played items after the baseline.
    pub fn library_scanned_at(&self, source: String) -> Result<Option<SystemTime>, ScrovaError> {
        Ok(self.store.library_baseline(&source)?)
    }

    /// Step 1 of the desktop auth flow: open `url` in a browser, then call
    /// [`Self::complete_authorization`] with `token` once the user is back.
    pub async fn begin_authorization(&self) -> Result<AuthRequest, ScrovaError> {
        let token = self.lastfm.get_token().await?;
        let url = self.lastfm.authorize_url(&token);
        Ok(AuthRequest { token, url })
    }

    /// Exchanges an approved token for a session. Returns
    /// [`ScrovaError::AuthorizationPending`] while the user has not approved
    /// yet, so callers can simply try again.
    pub async fn complete_authorization(
        &self,
        token: String,
    ) -> Result<LastfmAccount, ScrovaError> {
        let session = self
            .lastfm
            .get_session(&token)
            .await
            .map_err(|error| match error {
                // auth.getSession documents 14 as "token not yet authorized".
                LastfmError::Api { code: 14, .. } => ScrovaError::AuthorizationPending,
                other => other.into(),
            })?;
        self.set_session_key(Some(session.key.clone()));
        Ok(LastfmAccount {
            username: session.name,
            session_key: session.key,
            subscriber: session.subscriber,
        })
    }

    /// Best effort: Last.fm asks clients not to retry failed Now Playing
    /// updates, so the caller should just log the error.
    pub async fn update_now_playing(&self, track: Track) -> Result<(), ScrovaError> {
        let session_key = self.require_session_key()?;
        let now_playing = NowPlaying {
            artist: track.artist,
            track: track.title,
            album: track.album,
            album_artist: track.album_artist,
            duration: track.duration,
            track_number: None,
        };
        let result = self
            .lastfm
            .update_now_playing(&session_key, &now_playing)
            .await;
        self.forget_session_if_invalid(result).map_err(Into::into)
    }

    /// Sends every due scrobble, oldest first, in batches of 50.
    ///
    /// Stops at the first request-level failure and reschedules that batch, so
    /// a flaky connection does not burn through the whole queue's attempts.
    pub async fn flush(&self) -> Result<FlushReport, ScrovaError> {
        let _guard = self.flush_lock.lock().await;
        let session_key = self.require_session_key()?;
        let mut report = FlushReport::default();

        loop {
            let now = SystemTime::now();
            let batch = self
                .store
                .claim_batch(LASTFM_DESTINATION, now, BATCH_SIZE)?;
            if batch.is_empty() {
                break;
            }
            let requests: Vec<_> = batch.iter().map(records::scrobble_request).collect();
            let ids: Vec<QueueId> = batch.iter().map(|item| item.id).collect();

            match self.lastfm.scrobble(&session_key, &requests).await {
                Ok(outcomes) => self.record_outcomes(&batch, outcomes, &mut report)?,
                Err(error) => {
                    let message = error.to_string();
                    let retry_at = |floor: Duration| {
                        now + scrova_storage::backoff(max_attempts(&batch) + 1).max(floor)
                    };
                    match error.retry_class() {
                        RetryClass::Retry => {
                            self.store
                                .mark_retry(&ids, retry_at(Duration::ZERO), &message)?;
                            report.retry_scheduled += ids.len() as u32;
                        }
                        RetryClass::RetryAfterBackoff => {
                            self.store
                                .mark_retry(&ids, retry_at(RATE_LIMIT_BACKOFF), &message)?;
                            report.retry_scheduled += ids.len() as u32;
                        }
                        RetryClass::Reauthenticate => {
                            // Keep the batch due immediately: it is fine once
                            // the user signs in again.
                            self.store.mark_retry(&ids, now, &message)?;
                            self.set_session_key(None);
                            return Err(ScrovaError::SessionInvalid);
                        }
                        RetryClass::Suspended => {
                            self.store
                                .mark_retry(&ids, now + SUSPENDED_BACKOFF, &message)?;
                            return Err(ScrovaError::ApiKeySuspended);
                        }
                        RetryClass::Permanent => {
                            // Kept in the database as failed rather than
                            // deleted, so a fix on our side can resend them.
                            self.store.mark_failed(&ids, &message)?;
                            report.failed += ids.len() as u32;
                        }
                    }
                    break;
                }
            }
        }

        report.remaining = self.store.pending_count(LASTFM_DESTINATION)?;
        Ok(report)
    }

    pub fn pending_count(&self) -> Result<u64, ScrovaError> {
        Ok(self.store.pending_count(LASTFM_DESTINATION)?)
    }

    pub fn recent_scrobbles(&self, limit: u32) -> Result<Vec<ScrobbleRecord>, ScrovaError> {
        Ok(self
            .store
            .recent(limit as usize)?
            .into_iter()
            .map(ScrobbleRecord::from)
            .collect())
    }

    /// Returns one History page ordered by playback time, newest first.
    ///
    /// `before` is the opaque `next` value from the previous page. Passing it
    /// back keeps paging stable even when several plays share a timestamp.
    pub fn scrobble_history(
        &self,
        limit: u32,
        before: Option<HistoryCursor>,
    ) -> Result<HistoryPage, ScrovaError> {
        let before = before.map(|cursor| StorageHistoryCursor {
            started_at: cursor.started_at,
            id: cursor.id,
        });
        let page: StorageHistoryPage = self.store.history_page(limit as usize, before)?;
        Ok(HistoryPage {
            items: page.items.into_iter().map(ScrobbleRecord::from).collect(),
            next: page.next.map(|cursor| HistoryCursor {
                started_at: cursor.started_at,
                id: cursor.id,
            }),
        })
    }
}

impl Scrova {
    fn with_parts(store: Store, lastfm: LastfmClient) -> Result<Arc<Self>, ScrovaError> {
        // A previous process may have died mid-request; see Store::recover_in_flight.
        store.recover_in_flight()?;
        Ok(Arc::new(Self {
            engine: Mutex::new(ScrobbleEngine::new(EngineConfig::default())),
            store,
            lastfm,
            session_key: Mutex::new(None),
            flush_lock: tokio::sync::Mutex::new(()),
        }))
    }

    /// The engine forgets its sessions when the app restarts. Joining the same
    /// play again midway credits the playhead and would scrobble it a second
    /// time; its estimated start lands next to the first one.
    fn already_scrobbled(&self, scrobble: &Scrobble) -> Result<bool, ScrovaError> {
        let from = scrobble.started_at - RESTART_DUPLICATE_WINDOW;
        let to = scrobble.started_at + RESTART_DUPLICATE_WINDOW;
        let track = &scrobble.track;
        let starts = match track.source_track_id.as_deref() {
            Some(track_id) => self.store.existing_scrobble_starts(track_id, from, to)?,
            None => self.store.existing_scrobble_starts_by_metadata(
                &track.title,
                &track.artist,
                from,
                to,
            )?,
        };
        Ok(!starts.is_empty())
    }

    fn require_session_key(&self) -> Result<String, ScrovaError> {
        lock(&self.session_key)
            .clone()
            .ok_or(ScrovaError::NotSignedIn)
    }

    fn forget_session_if_invalid<T>(
        &self,
        result: Result<T, LastfmError>,
    ) -> Result<T, LastfmError> {
        if let Err(error) = &result
            && error.retry_class() == RetryClass::Reauthenticate
        {
            self.set_session_key(None);
        }
        result
    }

    fn record_outcomes(
        &self,
        batch: &[QueueItem],
        outcomes: Vec<ScrobbleOutcome>,
        report: &mut FlushReport,
    ) -> Result<(), ScrovaError> {
        let mut accepted = Vec::new();
        for (item, outcome) in batch.iter().zip(outcomes) {
            match outcome {
                ScrobbleOutcome::Accepted { .. } => accepted.push(item.id),
                ScrobbleOutcome::Ignored { code, message } => {
                    self.store.mark_ignored(item.id, code.into(), &message)?;
                    report.ignored += 1;
                }
            }
        }
        self.store.mark_submitted(&accepted)?;
        report.submitted += accepted.len() as u32;
        Ok(())
    }
}

fn library_state(item: &LibraryItem) -> Option<(String, LibraryState)> {
    let track_id = item.track.source_track_id.as_ref()?;
    if track_id.is_empty() {
        return None;
    }
    Some((
        track_id.clone(),
        LibraryState {
            play_count: item.play_count,
            last_played_at: item.last_played_at,
        },
    ))
}

/// Broad range of existing scrobbles that could affect this item.
///
/// It covers both the tolerance matching around every candidate and the exact
/// `(old.last_played_at - duration, last_played_at]` window used by the count
/// rule, so storage can return one list for the planner.
fn recovery_query_range(
    item: &LibraryItem,
    previous: Option<&LibraryState>,
    baseline_scanned_at: SystemTime,
    config: &RecoveryConfig,
) -> Option<(SystemTime, SystemTime)> {
    let duration = item.track.duration.filter(|duration| !duration.is_zero())?;
    let last_played_at = item.last_played_at?;
    let latest_start = last_played_at.checked_sub(duration)?;

    if config.max_scrobbles == 0 {
        return None;
    }

    let plays = match previous {
        None => {
            if last_played_at <= baseline_scanned_at {
                return None;
            }
            1
        }
        Some(old) => {
            if item.play_count <= old.play_count {
                return None;
            }
            if old
                .last_played_at
                .is_some_and(|old_last| last_played_at <= old_last)
            {
                return None;
            }
            let delta = item.play_count - old.play_count;
            usize::try_from(delta)
                .unwrap_or(usize::MAX)
                .min(config.max_scrobbles)
        }
    };
    if plays == 0 {
        return None;
    }

    let span = duration.checked_mul(u32::try_from(plays - 1).ok()?)?;
    let earliest_start = latest_start.checked_sub(span)?;
    let tolerance = std::cmp::max(duration / 2, Duration::from_secs(30));

    let mut from = earliest_start.checked_sub(tolerance)?;
    if let Some(old_last) = previous.and_then(|state| state.last_played_at)
        && let Some(count_from) = old_last.checked_sub(duration)
        && count_from < from
    {
        from = count_from;
    }

    Some((from, last_played_at.checked_add(tolerance)?))
}

fn max_attempts(batch: &[QueueItem]) -> u32 {
    batch.iter().map(|item| item.attempts).max().unwrap_or(0)
}

/// A panic while holding one of these locks cannot leave the guarded value
/// half-updated in a way that matters more than losing the whole app state.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests;
