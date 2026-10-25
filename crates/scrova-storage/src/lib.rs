//! Durable delivery queue for Scrova.
//!
//! A scrobble is stored once, while each destination (Last.fm today,
//! ListenBrainz later) gets its own delivery state. This keeps retries and
//! failures isolated per service instead of making the whole queue item
//! unusable when one destination is down.
//!
//! SQLite is used instead of a JSON queue because the app and its extensions
//! may touch the queue from different processes. WAL mode and transactions let
//! readers such as a history view run without blocking delivery claims.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, params_from_iter};
use scrova_core::{
    KnownPlay, Scrobble, ScrobbleOrigin, TimestampAccuracy, Track, recent_played_key_parts,
};
use thiserror::Error;

pub use scrova_core::{LibraryState, RecentSnapshot};

/// Destination identifier for Last.fm. New services can be added without a
/// schema migration because destination is part of the delivery key.
pub const LASTFM_DESTINATION: &str = "lastfm";

/// The queue drains oldest listening sessions first. Callers that do not have
/// a specific batch size can use this value with [`Store::claim_batch`].
pub const DEFAULT_CLAIM_LIMIT: usize = 50;

/// Upper bound for retry delays so an offline device eventually retries even
/// after a long failure streak.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);

const SCHEMA_VERSION: i64 = 3;
const BASE_BACKOFF_SECONDS: u64 = 60;
/// SQLite's default parameter limit is 999; keep chunks well below it so a
/// large library snapshot still fits in one query.
const LIBRARY_ID_CHUNK: usize = 500;

/// Errors returned by the storage layer.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than the supported version {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },
    #[error("timestamp is outside the range representable as Unix milliseconds")]
    TimeOutOfRange,
    #[error("at least one destination is required")]
    NoDestinations,
    #[error("database contains an invalid {field} value: {value}")]
    InvalidField { field: &'static str, value: String },
}

/// Convenience result type for this crate.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Identifies one delivery, not the scrobble body. The same scrobble can have
/// several deliveries when more destinations are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(i64);

impl QueueId {
    /// Raw SQLite row identifier.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for QueueId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Delivery progress for a single destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    InFlight,
    Submitted,
    Ignored { code: i64, message: String },
    Failed { permanent: bool, message: String },
}

impl DeliveryState {
    /// Terminal states are useful for history views and for deciding whether a
    /// queue entry still needs attention.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Submitted | Self::Ignored { .. } | Self::Failed { .. }
        )
    }

    fn from_columns(
        state: &str,
        last_error: Option<String>,
        ignored_code: Option<i64>,
        permanent_failure: bool,
    ) -> Result<Self> {
        match state {
            "pending" => Ok(Self::Pending),
            "in_flight" => Ok(Self::InFlight),
            "submitted" => Ok(Self::Submitted),
            "ignored" => Ok(Self::Ignored {
                code: ignored_code.ok_or_else(|| StoreError::InvalidField {
                    field: "ignored_code",
                    value: "<missing>".to_owned(),
                })?,
                message: last_error.unwrap_or_default(),
            }),
            "failed" => Ok(Self::Failed {
                permanent: permanent_failure,
                message: last_error.unwrap_or_default(),
            }),
            other => Err(StoreError::InvalidField {
                field: "state",
                value: other.to_owned(),
            }),
        }
    }
}

/// Result of enqueueing a scrobble when the caller needs to distinguish a new
/// row from an idempotent retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Queued(QueueId),
    AlreadyQueued(QueueId),
}

/// One atomic library-recovery write: the baseline (only on the first scan),
/// updated snapshot states, and recovered scrobbles.
#[derive(Debug, Clone, Copy)]
pub struct LibraryScan<'a> {
    pub baseline: Option<SystemTime>,
    pub states: &'a [(String, LibraryState)],
    pub scrobbles: &'a [Scrobble],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibraryScanResult {
    pub queued: u32,
    pub already_queued: u32,
}

/// A delivery together with the immutable scrobble body it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub id: QueueId,
    pub destination: String,
    pub scrobble: Scrobble,
    /// Row id of the scrobble body. Delivery ids can change when a destination
    /// is added, so history cursors use this stable pair with `started_at`.
    pub scrobble_id: i64,
    pub state: DeliveryState,
    pub attempts: u32,
    pub next_attempt_at: SystemTime,
    pub last_error: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// Opaque keyset cursor for [`Store::history_page`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryCursor {
    pub started_at: SystemTime,
    pub id: i64,
}

/// One page of scrobbles, newest first.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPage {
    pub items: Vec<QueueItem>,
    /// `None` when no older rows exist. The UI only passes this value back.
    pub next: Option<HistoryCursor>,
}

/// SQLite-backed scrobble queue.
pub struct Store {
    // rusqlite connections are Send but not Sync, and an app may call this from
    // different queues. A mutex keeps the API borrow-free while serialising
    // statements on the single connection.
    connection: Mutex<Connection>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not expose the connection or database path in logs.
        formatter.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Opens a file-backed database and runs pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// Opens an in-memory database, mainly for tests and previews.
    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;

        // WAL is persistent in the database file. In-memory databases report
        // "memory" here, which is expected and not an error.
        let _journal_mode: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;

        migrate(&mut connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Stores a scrobble once and creates its default Last.fm delivery.
    ///
    /// The engine can repeat an observation after a restart; `session_id` is
    /// the idempotency key, so a duplicate returns the existing delivery
    /// instead of enqueueing the same listen twice.
    pub fn enqueue(&self, scrobble: &Scrobble) -> Result<QueueId> {
        let ids = self.enqueue_for(scrobble, &[LASTFM_DESTINATION])?;
        Ok(ids[0])
    }

    /// Like [`Self::enqueue`], but reports whether the session was already
    /// queued. Recovery uses this to keep reruns idempotent without silently
    /// counting a duplicate as new work.
    pub fn enqueue_reported(&self, scrobble: &Scrobble) -> Result<EnqueueOutcome> {
        let (ids, inserted) = self.enqueue_for_impl(scrobble, &[LASTFM_DESTINATION])?;
        let id = ids[0];
        Ok(if inserted {
            EnqueueOutcome::Queued(id)
        } else {
            EnqueueOutcome::AlreadyQueued(id)
        })
    }

    /// Enqueues one scrobble for multiple destinations.
    ///
    /// This exists so adding ListenBrainz later does not require a second
    /// scrobble row or a schema change.
    pub fn enqueue_for(&self, scrobble: &Scrobble, destinations: &[&str]) -> Result<Vec<QueueId>> {
        let (ids, _) = self.enqueue_for_impl(scrobble, destinations)?;
        Ok(ids)
    }

    fn enqueue_for_impl(
        &self,
        scrobble: &Scrobble,
        destinations: &[&str],
    ) -> Result<(Vec<QueueId>, bool)> {
        if destinations.is_empty() {
            return Err(StoreError::NoDestinations);
        }

        let now_ms = now_millis()?;
        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = insert_scrobble(&transaction, scrobble, destinations, now_ms)?;
        transaction.commit()?;
        Ok(result)
    }

    /// Returns the first scan time for `source`, or `None` if the library has
    /// never been baselined.
    pub fn library_baseline(&self, source: &str) -> Result<Option<SystemTime>> {
        let scanned_at_ms: Option<i64> = self
            .connection()
            .query_row(
                "SELECT scanned_at_ms FROM library_baseline WHERE source = ?1",
                params![source],
                |row| row.get(0),
            )
            .optional()?;
        scanned_at_ms.map(system_time_from_millis).transpose()
    }

    /// Reads stored state for the requested track IDs, chunked to stay under
    /// SQLite's bind-parameter limit for large library snapshots.
    pub fn library_states<T: AsRef<str>>(
        &self,
        source: &str,
        track_ids: &[T],
    ) -> Result<HashMap<String, LibraryState>> {
        if track_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let connection = self.connection();
        let mut states = HashMap::new();
        for chunk in track_ids.chunks(LIBRARY_ID_CHUNK) {
            let placeholders = (0..chunk.len())
                .map(|index| format!("?{}", index + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT track_id, play_count, last_played_at_ms
                 FROM library_state
                 WHERE source = ?1 AND track_id IN ({placeholders})"
            );
            let mut statement = connection.prepare(&sql)?;

            let mut values = Vec::with_capacity(chunk.len() + 1);
            values.push(source.to_owned());
            values.extend(chunk.iter().map(|track_id| track_id.as_ref().to_owned()));

            let rows = statement.query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?;
            for row in rows {
                let (track_id, play_count, last_played_at_ms) = row?;
                let play_count =
                    u64::try_from(play_count).map_err(|_| StoreError::InvalidField {
                        field: "play_count",
                        value: play_count.to_string(),
                    })?;
                let last_played_at = last_played_at_ms.map(system_time_from_millis).transpose()?;
                states.insert(
                    track_id,
                    LibraryState {
                        play_count,
                        last_played_at,
                    },
                );
            }
        }
        Ok(states)
    }

    /// Stores snapshot state for a set of tracks in one transaction.
    pub fn upsert_library_states(
        &self,
        source: &str,
        states: &[(String, LibraryState)],
    ) -> Result<()> {
        if states.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (track_id, state) in states {
            upsert_library_state(&transaction, source, track_id, *state)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Records the first scan time. Later calls are no-ops so a baseline can
    /// never move forward and accidentally hide old plays.
    pub fn set_library_baseline(&self, source: &str, scanned_at: SystemTime) -> Result<()> {
        let scanned_at_ms = system_time_to_millis(scanned_at)?;
        self.connection().execute(
            "INSERT INTO library_baseline (source, scanned_at_ms)
             VALUES (?1, ?2)
             ON CONFLICT(source) DO NOTHING",
            params![source, scanned_at_ms],
        )?;
        Ok(())
    }

    /// Lists scrobble starts for one track within an inclusive time range.
    pub fn existing_scrobble_starts(
        &self,
        track_source_id: &str,
        from: SystemTime,
        to: SystemTime,
    ) -> Result<Vec<SystemTime>> {
        let from_ms = system_time_to_millis(from)?;
        let to_ms = system_time_to_millis(to)?;

        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT started_at_ms
             FROM scrobbles
             WHERE track_source_id = ?1
               AND started_at_ms >= ?2
               AND started_at_ms <= ?3
             ORDER BY started_at_ms ASC",
        )?;
        let rows = statement.query_map(params![track_source_id, from_ms, to_ms], |row| {
            row.get::<_, i64>(0)
        })?;

        let mut starts = Vec::new();
        for row in rows {
            starts.push(system_time_from_millis(row?)?);
        }
        Ok(starts)
    }

    /// Like [`Self::existing_scrobble_starts`], for players that expose no
    /// track ID (many Android apps): matches on title and artist instead.
    pub fn existing_scrobble_starts_by_metadata(
        &self,
        title: &str,
        artist: &str,
        from: SystemTime,
        to: SystemTime,
    ) -> Result<Vec<SystemTime>> {
        let from_ms = system_time_to_millis(from)?;
        let to_ms = system_time_to_millis(to)?;

        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT started_at_ms
             FROM scrobbles
             WHERE track_title = ?1
               AND track_artist = ?2
               AND started_at_ms >= ?3
               AND started_at_ms <= ?4
             ORDER BY started_at_ms ASC",
        )?;
        let rows = statement.query_map(params![title, artist, from_ms, to_ms], |row| {
            row.get::<_, i64>(0)
        })?;

        let mut starts = Vec::new();
        for row in rows {
            starts.push(system_time_from_millis(row?)?);
        }
        Ok(starts)
    }

    /// Applies a recovery scan in one transaction: optional baseline, snapshot
    /// states, and queued scrobbles either all commit or all roll back.
    pub fn apply_library_scan(
        &self,
        source: &str,
        scan: LibraryScan<'_>,
    ) -> Result<LibraryScanResult> {
        let now_ms = now_millis()?;
        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if let Some(baseline) = scan.baseline {
            let scanned_at_ms = system_time_to_millis(baseline)?;
            transaction.execute(
                "INSERT INTO library_baseline (source, scanned_at_ms)
                 VALUES (?1, ?2)
                 ON CONFLICT(source) DO NOTHING",
                params![source, scanned_at_ms],
            )?;
        }

        for (track_id, state) in scan.states {
            upsert_library_state(&transaction, source, track_id, *state)?;
        }

        let mut queued = 0_usize;
        let mut already_queued = 0_usize;
        for scrobble in scan.scrobbles {
            let (_, inserted) =
                insert_scrobble(&transaction, scrobble, &[LASTFM_DESTINATION], now_ms)?;
            if inserted {
                queued += 1;
            } else {
                already_queued += 1;
            }
        }

        transaction.commit()?;
        Ok(LibraryScanResult {
            queued: u32::try_from(queued).unwrap_or(u32::MAX),
            already_queued: u32::try_from(already_queued).unwrap_or(u32::MAX),
        })
    }

    /// Reads the last MusicKit recently-played snapshot for a source.
    pub fn recent_snapshot(&self, source: &str) -> Result<Option<RecentSnapshot>> {
        let row: Option<(String, i64)> = self
            .connection()
            .query_row(
                "SELECT keys_json, taken_at_ms
                 FROM recent_played_snapshot
                 WHERE source = ?1",
                params![source],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let Some((keys_json, taken_at_ms)) = row else {
            return Ok(None);
        };
        let keys = serde_json::from_str(&keys_json).map_err(|error| StoreError::InvalidField {
            field: "recent_played_snapshot.keys_json",
            value: error.to_string(),
        })?;

        Ok(Some(RecentSnapshot {
            keys,
            taken_at: system_time_from_millis(taken_at_ms)?,
        }))
    }

    /// Writes only the snapshot. Recovery uses [`Self::apply_recent_played`] so
    /// a failed enqueue cannot advance the baseline past unqueued plays.
    pub fn put_recent_snapshot(&self, source: &str, snapshot: &RecentSnapshot) -> Result<()> {
        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        upsert_recent_snapshot(&transaction, source, snapshot)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persisted plays for a source, used to suppress recovery duplicates.
    ///
    /// Keys are normalized the same way as the recent-played planner so a
    /// catalog ID and a library persistent ID still compare equal by metadata.
    pub fn known_plays(&self, source: &str, since: SystemTime) -> Result<Vec<KnownPlay>> {
        let since_ms = system_time_to_millis(since)?;
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT track_artist, track_title, started_at_ms
             FROM scrobbles
             WHERE source = ?1 AND started_at_ms >= ?2
             ORDER BY started_at_ms ASC",
        )?;
        let rows = statement.query_map(params![source, since_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        let mut plays = Vec::new();
        for row in rows {
            let (artist, title, started_at_ms) = row?;
            plays.push(KnownPlay {
                key: recent_played_key_parts(&artist, &title),
                started_at: system_time_from_millis(started_at_ms)?,
            });
        }
        Ok(plays)
    }

    /// Applies a recently-played scan atomically: the snapshot and all planned
    /// scrobbles commit together, mirroring library recovery.
    pub fn apply_recent_played(
        &self,
        source: &str,
        snapshot: &RecentSnapshot,
        scrobbles: &[Scrobble],
    ) -> Result<LibraryScanResult> {
        let now_ms = now_millis()?;
        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

        upsert_recent_snapshot(&transaction, source, snapshot)?;

        let mut queued = 0_usize;
        let mut already_queued = 0_usize;
        for scrobble in scrobbles {
            let (_, inserted) =
                insert_scrobble(&transaction, scrobble, &[LASTFM_DESTINATION], now_ms)?;
            if inserted {
                queued += 1;
            } else {
                already_queued += 1;
            }
        }

        transaction.commit()?;
        Ok(LibraryScanResult {
            queued: u32::try_from(queued).unwrap_or(u32::MAX),
            already_queued: u32::try_from(already_queued).unwrap_or(u32::MAX),
        })
    }

    /// Claims pending deliveries for one destination.
    ///
    /// The transaction moves rows to `in_flight` before returning them so two
    /// workers cannot send the same scrobble. Ordering by the scrobble's
    /// `started_at` drains the oldest listening sessions first.
    pub fn claim_batch(
        &self,
        destination: &str,
        now: SystemTime,
        max: usize,
    ) -> Result<Vec<QueueItem>> {
        if max == 0 {
            return Ok(Vec::new());
        }

        let now_ms = system_time_to_millis(now)?;
        let limit = i64::try_from(max).unwrap_or(i64::MAX);

        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let rows = {
            let sql = format!(
                "{QUEUE_SELECT}
                 WHERE d.destination = ?1
                   AND d.state = 'pending'
                   AND d.next_attempt_at_ms <= ?2
                 ORDER BY s.started_at_ms ASC, s.id ASC
                 LIMIT ?3"
            );
            let mut statement = transaction.prepare(&sql)?;
            let mapped = statement.query_map(params![destination, now_ms, limit], raw_from_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let stored_now = system_time_from_millis(now_ms)?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let mut item = QueueItem::try_from(row)?;
            transaction.execute(
                "UPDATE deliveries
                 SET state = 'in_flight', updated_at_ms = ?1
                 WHERE id = ?2 AND state = 'pending'",
                params![now_ms, item.id.get()],
            )?;
            item.state = DeliveryState::InFlight;
            item.updated_at = stored_now;
            items.push(item);
        }

        transaction.commit()?;
        Ok(items)
    }

    /// Marks deliveries as accepted by their destination.
    pub fn mark_submitted(&self, ids: impl AsRef<[QueueId]>) -> Result<()> {
        self.update_many(ids.as_ref(), |transaction, id, now_ms| {
            transaction.execute(
                "UPDATE deliveries
                 SET state = 'submitted',
                     last_error = NULL,
                     ignored_code = NULL,
                     permanent_failure = 0,
                     updated_at_ms = ?1
                 WHERE id = ?2",
                params![now_ms, id.get()],
            )?;
            Ok(())
        })
    }

    /// Marks one delivery as ignored by the destination.
    pub fn mark_ignored(&self, id: QueueId, code: i64, message: &str) -> Result<()> {
        let now_ms = now_millis()?;
        self.connection().execute(
            "UPDATE deliveries
             SET state = 'ignored',
                 ignored_code = ?1,
                 last_error = ?2,
                 permanent_failure = 0,
                 updated_at_ms = ?3
             WHERE id = ?4",
            params![code, message, now_ms, id.get()],
        )?;
        Ok(())
    }

    /// Records a transient failure and schedules the next attempt.
    pub fn mark_retry(
        &self,
        ids: impl AsRef<[QueueId]>,
        next_attempt_at: SystemTime,
        error: &str,
    ) -> Result<()> {
        let next_attempt_ms = system_time_to_millis(next_attempt_at)?;
        self.update_many(ids.as_ref(), |transaction, id, now_ms| {
            transaction.execute(
                "UPDATE deliveries
                 SET state = 'pending',
                     attempts = attempts + 1,
                     next_attempt_at_ms = ?1,
                     last_error = ?2,
                     ignored_code = NULL,
                     permanent_failure = 0,
                     updated_at_ms = ?3
                 WHERE id = ?4",
                params![next_attempt_ms, error, now_ms, id.get()],
            )?;
            Ok(())
        })
    }

    /// Marks deliveries as permanently failed.
    pub fn mark_failed(&self, ids: impl AsRef<[QueueId]>, message: &str) -> Result<()> {
        self.update_many(ids.as_ref(), |transaction, id, now_ms| {
            transaction.execute(
                "UPDATE deliveries
                 SET state = 'failed',
                     last_error = ?1,
                     ignored_code = NULL,
                     permanent_failure = 1,
                     updated_at_ms = ?2
                 WHERE id = ?3",
                params![message, now_ms, id.get()],
            )?;
            Ok(())
        })
    }

    /// Returns in-flight deliveries to pending after a crash.
    ///
    /// The previous request may already have reached the service, so the exact
    /// outcome is unknown. The item is retried with `uncertain` in
    /// `last_error`; losing a scrobble is worse than a possible duplicate that
    /// the destination can reject.
    pub fn recover_in_flight(&self) -> Result<usize> {
        let now_ms = now_millis()?;
        let changed = self.connection().execute(
            "UPDATE deliveries
             SET state = 'pending',
                 next_attempt_at_ms = ?1,
                 last_error = 'uncertain',
                 ignored_code = NULL,
                 permanent_failure = 0,
                 updated_at_ms = ?1
             WHERE state = 'in_flight'",
            params![now_ms],
        )?;
        Ok(changed)
    }

    /// Number of deliveries still waiting to be claimed for a destination.
    pub fn pending_count(&self, destination: &str) -> Result<u64> {
        let count: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM deliveries
             WHERE destination = ?1 AND state = 'pending'",
            params![destination],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    /// Most recently enqueued deliveries, including terminal states.
    pub fn recent(&self, limit: usize) -> Result<Vec<QueueItem>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let sql = format!(
            "{QUEUE_SELECT}
             ORDER BY d.created_at_ms DESC, d.id DESC
             LIMIT ?1"
        );

        let connection = self.connection();
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params![limit], raw_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter().map(QueueItem::try_from).collect()
    }

    /// Keyset-paged History query, newest first.
    ///
    /// `(started_at_ms, id)` is both the sort key and the cursor. Comparing the
    /// pair avoids skipping or repeating rows that share a timestamp, which is
    /// common for recovered plays. The existing `idx_scrobbles_started` index
    /// covers this order, so no extra migration is needed.
    pub fn history_page(&self, limit: usize, before: Option<HistoryCursor>) -> Result<HistoryPage> {
        if limit == 0 {
            return Ok(HistoryPage {
                items: Vec::new(),
                next: None,
            });
        }

        let fetch_limit = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let mut values = vec![Value::Text(LASTFM_DESTINATION.to_owned())];
        let sql = if let Some(cursor) = before {
            values.push(Value::Integer(system_time_to_millis(cursor.started_at)?));
            values.push(Value::Integer(cursor.id));
            values.push(Value::Integer(fetch_limit));
            format!(
                "{QUEUE_SELECT}
                 WHERE d.destination = ?1
                   AND (s.started_at_ms, s.id) < (?2, ?3)
                 ORDER BY s.started_at_ms DESC, s.id DESC
                 LIMIT ?4"
            )
        } else {
            values.push(Value::Integer(fetch_limit));
            format!(
                "{QUEUE_SELECT}
                 WHERE d.destination = ?1
                 ORDER BY s.started_at_ms DESC, s.id DESC
                 LIMIT ?2"
            )
        };

        let connection = self.connection();
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values), raw_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut items = rows
            .into_iter()
            .map(QueueItem::try_from)
            .collect::<Result<Vec<_>>>()?;

        // Fetch one extra row so `next` is only set when something is actually
        // older, avoiding an empty extra page at an exact boundary.
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next = if has_more {
            items.last().map(|item| HistoryCursor {
                started_at: item.scrobble.started_at,
                id: item.scrobble_id,
            })
        } else {
            None
        };

        Ok(HistoryPage { items, next })
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        // A panic in another caller must not make the queue permanently
        // unusable; SQLite will have rolled back the interrupted transaction.
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn update_many(
        &self,
        ids: &[QueueId],
        update: impl Fn(&rusqlite::Transaction<'_>, QueueId, i64) -> rusqlite::Result<()>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let now_ms = now_millis()?;
        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for id in ids {
            update(&transaction, *id, now_ms)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

/// Inserts a scrobble and its deliveries inside an existing transaction.
///
/// Returns whether the scrobble row was newly inserted; a duplicate
/// `session_id` still gets any missing deliveries but is not new work.
fn insert_scrobble(
    transaction: &rusqlite::Transaction<'_>,
    scrobble: &Scrobble,
    destinations: &[&str],
    now_ms: i64,
) -> Result<(Vec<QueueId>, bool)> {
    let started_at_ms = system_time_to_millis(scrobble.started_at)?;
    let duration_ms = scrobble
        .track
        .duration
        .map(duration_to_millis)
        .transpose()?;

    let inserted = transaction.execute(
        "INSERT INTO scrobbles (
            session_id,
            track_title,
            track_artist,
            track_album,
            track_album_artist,
            track_duration_ms,
            track_source_id,
            started_at_ms,
            timestamp_accuracy,
            origin,
            source,
            created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(session_id) DO NOTHING",
        params![
            &scrobble.session_id,
            &scrobble.track.title,
            &scrobble.track.artist,
            scrobble.track.album.as_deref(),
            scrobble.track.album_artist.as_deref(),
            duration_ms,
            scrobble.track.source_track_id.as_deref(),
            started_at_ms,
            accuracy_as_str(scrobble.timestamp_accuracy),
            origin_as_str(scrobble.origin),
            &scrobble.source,
            now_ms,
        ],
    )? == 1;

    let scrobble_id: i64 = transaction.query_row(
        "SELECT id FROM scrobbles WHERE session_id = ?1",
        params![&scrobble.session_id],
        |row| row.get(0),
    )?;

    let mut ids = Vec::with_capacity(destinations.len());
    for destination in destinations {
        transaction.execute(
            "INSERT INTO deliveries (
                scrobble_id,
                destination,
                state,
                attempts,
                next_attempt_at_ms,
                last_error,
                ignored_code,
                permanent_failure,
                created_at_ms,
                updated_at_ms
             ) VALUES (?1, ?2, 'pending', 0, 0, NULL, NULL, 0, ?3, ?3)
             ON CONFLICT(scrobble_id, destination) DO NOTHING",
            params![scrobble_id, destination, now_ms],
        )?;

        let id: i64 = transaction.query_row(
            "SELECT id FROM deliveries WHERE scrobble_id = ?1 AND destination = ?2",
            params![scrobble_id, destination],
            |row| row.get(0),
        )?;
        ids.push(QueueId(id));
    }

    Ok((ids, inserted))
}

fn upsert_library_state(
    transaction: &rusqlite::Transaction<'_>,
    source: &str,
    track_id: &str,
    state: LibraryState,
) -> Result<()> {
    let play_count = i64::try_from(state.play_count).map_err(|_| StoreError::InvalidField {
        field: "play_count",
        value: state.play_count.to_string(),
    })?;
    let last_played_at_ms = state
        .last_played_at
        .map(system_time_to_millis)
        .transpose()?;

    transaction.execute(
        "INSERT INTO library_state (source, track_id, play_count, last_played_at_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source, track_id) DO UPDATE SET
             play_count = excluded.play_count,
             last_played_at_ms = excluded.last_played_at_ms",
        params![source, track_id, play_count, last_played_at_ms],
    )?;
    Ok(())
}

fn upsert_recent_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    source: &str,
    snapshot: &RecentSnapshot,
) -> Result<()> {
    let keys_json =
        serde_json::to_string(&snapshot.keys).map_err(|error| StoreError::InvalidField {
            field: "recent_played_snapshot.keys_json",
            value: error.to_string(),
        })?;
    let taken_at_ms = system_time_to_millis(snapshot.taken_at)?;

    transaction.execute(
        "INSERT INTO recent_played_snapshot (source, keys_json, taken_at_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(source) DO UPDATE SET
             keys_json = excluded.keys_json,
             taken_at_ms = excluded.taken_at_ms",
        params![source, keys_json, taken_at_ms],
    )?;
    Ok(())
}

/// Deterministic exponential backoff for a failed attempt count.
///
/// `attempts` is the number of failures already recorded, so the first retry
/// waits one minute and the delay doubles until the one-hour cap. Jitter is
/// intentionally left to the caller: keeping this function pure makes retry
/// timing testable, while the caller can add randomness without changing the
/// contract.
pub fn backoff(attempts: u32) -> Duration {
    let exponent = attempts.saturating_sub(1).min(6);
    let seconds = BASE_BACKOFF_SECONDS.saturating_mul(1_u64 << exponent);
    Duration::from_secs(seconds).min(MAX_BACKOFF)
}

fn migrate(connection: &mut Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version > SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    // Each migration runs in a transaction with its user_version bump. A
    // process killed mid-migration therefore leaves the old version intact.
    if version < 1 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SCHEMA_V1)?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
    }

    if version < 2 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SCHEMA_V2)?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
    }

    if version < 3 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SCHEMA_V3)?;
        transaction.pragma_update(None, "user_version", 3)?;
        transaction.commit()?;
    }

    Ok(())
}

fn now_millis() -> Result<i64> {
    system_time_to_millis(SystemTime::now())
}

fn system_time_to_millis(time: SystemTime) -> Result<i64> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).map_err(|_| StoreError::TimeOutOfRange),
        Err(error) => {
            let millis = i64::try_from(error.duration().as_millis())
                .map_err(|_| StoreError::TimeOutOfRange)?;
            Ok(-millis)
        }
    }
}

fn system_time_from_millis(millis: i64) -> Result<SystemTime> {
    let duration = Duration::from_millis(millis.unsigned_abs());
    if millis >= 0 {
        UNIX_EPOCH
            .checked_add(duration)
            .ok_or(StoreError::TimeOutOfRange)
    } else {
        UNIX_EPOCH
            .checked_sub(duration)
            .ok_or(StoreError::TimeOutOfRange)
    }
}

fn duration_to_millis(duration: Duration) -> Result<i64> {
    i64::try_from(duration.as_millis()).map_err(|_| StoreError::TimeOutOfRange)
}

fn duration_from_millis(millis: i64) -> Result<Duration> {
    let millis = u64::try_from(millis).map_err(|_| StoreError::InvalidField {
        field: "track_duration_ms",
        value: millis.to_string(),
    })?;
    Ok(Duration::from_millis(millis))
}

fn accuracy_as_str(accuracy: TimestampAccuracy) -> &'static str {
    match accuracy {
        TimestampAccuracy::Observed => "observed",
        TimestampAccuracy::Estimated => "estimated",
    }
}

fn accuracy_from_str(value: &str) -> Result<TimestampAccuracy> {
    match value {
        "observed" => Ok(TimestampAccuracy::Observed),
        "estimated" => Ok(TimestampAccuracy::Estimated),
        other => Err(StoreError::InvalidField {
            field: "timestamp_accuracy",
            value: other.to_owned(),
        }),
    }
}

fn origin_as_str(origin: ScrobbleOrigin) -> &'static str {
    match origin {
        ScrobbleOrigin::LiveObservation => "live_observation",
        ScrobbleOrigin::HistoryRecovery => "history_recovery",
    }
}

fn origin_from_str(value: &str) -> Result<ScrobbleOrigin> {
    match value {
        "live_observation" => Ok(ScrobbleOrigin::LiveObservation),
        "history_recovery" => Ok(ScrobbleOrigin::HistoryRecovery),
        other => Err(StoreError::InvalidField {
            field: "origin",
            value: other.to_owned(),
        }),
    }
}

struct RawQueueItem {
    id: i64,
    destination: String,
    state: String,
    attempts: i64,
    next_attempt_at_ms: i64,
    last_error: Option<String>,
    ignored_code: Option<i64>,
    permanent_failure: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    session_id: String,
    track_title: String,
    track_artist: String,
    track_album: Option<String>,
    track_album_artist: Option<String>,
    track_duration_ms: Option<i64>,
    track_source_id: Option<String>,
    started_at_ms: i64,
    timestamp_accuracy: String,
    origin: String,
    source: String,
    scrobble_id: i64,
}

fn raw_from_row(row: &Row<'_>) -> rusqlite::Result<RawQueueItem> {
    Ok(RawQueueItem {
        id: row.get(0)?,
        destination: row.get(1)?,
        state: row.get(2)?,
        attempts: row.get(3)?,
        next_attempt_at_ms: row.get(4)?,
        last_error: row.get(5)?,
        ignored_code: row.get(6)?,
        permanent_failure: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
        session_id: row.get(10)?,
        track_title: row.get(11)?,
        track_artist: row.get(12)?,
        track_album: row.get(13)?,
        track_album_artist: row.get(14)?,
        track_duration_ms: row.get(15)?,
        track_source_id: row.get(16)?,
        started_at_ms: row.get(17)?,
        timestamp_accuracy: row.get(18)?,
        origin: row.get(19)?,
        source: row.get(20)?,
        scrobble_id: row.get(21)?,
    })
}

impl TryFrom<RawQueueItem> for QueueItem {
    type Error = StoreError;

    fn try_from(raw: RawQueueItem) -> Result<Self> {
        let RawQueueItem {
            id,
            destination,
            state,
            attempts,
            next_attempt_at_ms,
            last_error,
            ignored_code,
            permanent_failure,
            created_at_ms,
            updated_at_ms,
            session_id,
            track_title,
            track_artist,
            track_album,
            track_album_artist,
            track_duration_ms,
            track_source_id,
            started_at_ms,
            timestamp_accuracy,
            origin,
            source,
            scrobble_id,
        } = raw;

        let attempts = u32::try_from(attempts).map_err(|_| StoreError::InvalidField {
            field: "attempts",
            value: attempts.to_string(),
        })?;
        let duration = track_duration_ms.map(duration_from_millis).transpose()?;
        let state = DeliveryState::from_columns(
            &state,
            last_error.clone(),
            ignored_code,
            permanent_failure != 0,
        )?;

        Ok(Self {
            id: QueueId(id),
            destination,
            scrobble: Scrobble {
                track: Track {
                    title: track_title,
                    artist: track_artist,
                    album: track_album,
                    album_artist: track_album_artist,
                    duration,
                    source_track_id: track_source_id,
                },
                started_at: system_time_from_millis(started_at_ms)?,
                timestamp_accuracy: accuracy_from_str(&timestamp_accuracy)?,
                origin: origin_from_str(&origin)?,
                source,
                session_id,
            },
            scrobble_id,
            state,
            attempts,
            next_attempt_at: system_time_from_millis(next_attempt_at_ms)?,
            last_error,
            created_at: system_time_from_millis(created_at_ms)?,
            updated_at: system_time_from_millis(updated_at_ms)?,
        })
    }
}

const QUEUE_SELECT: &str = r#"
SELECT
    d.id,
    d.destination,
    d.state,
    d.attempts,
    d.next_attempt_at_ms,
    d.last_error,
    d.ignored_code,
    d.permanent_failure,
    d.created_at_ms,
    d.updated_at_ms,
    s.session_id,
    s.track_title,
    s.track_artist,
    s.track_album,
    s.track_album_artist,
    s.track_duration_ms,
    s.track_source_id,
    s.started_at_ms,
    s.timestamp_accuracy,
    s.origin,
    s.source,
    s.id
FROM deliveries d
JOIN scrobbles s ON s.id = d.scrobble_id
"#;

const SCHEMA_V1: &str = r#"
CREATE TABLE scrobbles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL UNIQUE,
    track_title TEXT NOT NULL,
    track_artist TEXT NOT NULL,
    track_album TEXT,
    track_album_artist TEXT,
    track_duration_ms INTEGER
        CHECK (track_duration_ms IS NULL OR track_duration_ms >= 0),
    track_source_id TEXT,
    started_at_ms INTEGER NOT NULL,
    timestamp_accuracy TEXT NOT NULL
        CHECK (timestamp_accuracy IN ('observed', 'estimated')),
    origin TEXT NOT NULL
        CHECK (origin IN ('live_observation', 'history_recovery')),
    source TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_scrobbles_started
    ON scrobbles (started_at_ms, id);

CREATE TABLE deliveries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scrobble_id INTEGER NOT NULL
        REFERENCES scrobbles (id) ON DELETE CASCADE,
    destination TEXT NOT NULL
        CHECK (length(destination) > 0),
    state TEXT NOT NULL
        CHECK (state IN ('pending', 'in_flight', 'submitted', 'ignored', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0
        CHECK (attempts >= 0),
    next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    ignored_code INTEGER,
    permanent_failure INTEGER NOT NULL DEFAULT 0
        CHECK (permanent_failure IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE (scrobble_id, destination)
);

CREATE INDEX idx_deliveries_claim
    ON deliveries (destination, state, next_attempt_at_ms, id);

CREATE INDEX idx_deliveries_recent
    ON deliveries (created_at_ms DESC, id DESC);
"#;

const SCHEMA_V2: &str = r#"
CREATE TABLE library_state (
    source TEXT NOT NULL,
    track_id TEXT NOT NULL,
    play_count INTEGER NOT NULL CHECK (play_count >= 0),
    last_played_at_ms INTEGER,
    PRIMARY KEY (source, track_id)
);

CREATE TABLE library_baseline (
    source TEXT PRIMARY KEY,
    scanned_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_scrobbles_track_started
    ON scrobbles (track_source_id, started_at_ms);
"#;

const SCHEMA_V3: &str = r#"
CREATE TABLE recent_played_snapshot (
    source TEXT PRIMARY KEY,
    keys_json TEXT NOT NULL,
    taken_at_ms INTEGER NOT NULL
);
"#;
