use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_core::{Scrobble, ScrobbleOrigin, TimestampAccuracy, Track};
use scrova_storage::{
    DEFAULT_CLAIM_LIMIT, DeliveryState, EnqueueOutcome, LASTFM_DESTINATION, LibraryScan,
    LibraryScanResult, LibraryState, MAX_BACKOFF, QueueId, QueueItem, Store, backoff,
};

fn at(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis)
}

fn sample(session_id: &str, started_at_ms: u64) -> Scrobble {
    Scrobble {
        track: Track {
            title: format!("Song {session_id}"),
            artist: "Example Artist".to_owned(),
            album: Some("Example Album".to_owned()),
            album_artist: Some("Example Album Artist".to_owned()),
            duration: Some(Duration::from_millis(245_500)),
            source_track_id: Some(format!("track-{session_id}")),
        },
        started_at: at(started_at_ms),
        timestamp_accuracy: TimestampAccuracy::Observed,
        origin: ScrobbleOrigin::LiveObservation,
        source: "com.apple.Music".to_owned(),
        session_id: session_id.to_owned(),
    }
}

fn minimal_sample(session_id: &str, started_at_ms: u64) -> Scrobble {
    Scrobble {
        track: Track {
            title: format!("Song {session_id}"),
            artist: "Example Artist".to_owned(),
            album: None,
            album_artist: None,
            duration: None,
            source_track_id: None,
        },
        started_at: at(started_at_ms),
        timestamp_accuracy: TimestampAccuracy::Estimated,
        origin: ScrobbleOrigin::HistoryRecovery,
        source: "history".to_owned(),
        session_id: session_id.to_owned(),
    }
}

fn item_for<'a>(items: &'a [QueueItem], session_id: &str) -> &'a QueueItem {
    items
        .iter()
        .find(|item| item.scrobble.session_id == session_id)
        .unwrap_or_else(|| panic!("missing queue item for {session_id}"))
}

#[test]
fn enqueue_is_idempotent_for_a_session() {
    let store = Store::open_in_memory().unwrap();
    let scrobble = sample("session-1", 1_000);

    let first = store.enqueue(&scrobble).unwrap();
    let second = store.enqueue(&scrobble).unwrap();

    assert_eq!(first, second);
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);

    let mut conflicting = sample("session-1", 2_000);
    conflicting.track.title = "A different title".to_owned();
    let third = store.enqueue(&conflicting).unwrap();

    assert_eq!(first, third);
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);
    let recent = store.recent(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].scrobble, scrobble);
    assert_eq!(recent[0].state, DeliveryState::Pending);
}

#[test]
fn claim_returns_oldest_sessions_first_and_respects_limit() {
    let store = Store::open_in_memory().unwrap();
    for (session, started_at_ms) in [("newest", 3_000), ("oldest", 1_000), ("middle", 2_000)] {
        store.enqueue(&sample(session, started_at_ms)).unwrap();
    }

    let now = at(10_000);
    let first = store.claim_batch(LASTFM_DESTINATION, now, 2).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].scrobble.session_id, "oldest");
    assert_eq!(first[1].scrobble.session_id, "middle");
    assert!(
        first
            .iter()
            .all(|item| item.state == DeliveryState::InFlight)
    );
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);

    let second = store
        .claim_batch(LASTFM_DESTINATION, now, DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].scrobble.session_id, "newest");

    assert!(
        store
            .claim_batch(LASTFM_DESTINATION, now, DEFAULT_CLAIM_LIMIT)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn claim_with_zero_limit_does_not_change_state() {
    let store = Store::open_in_memory().unwrap();
    store.enqueue(&sample("waiting", 1_000)).unwrap();

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(10_000), 0)
        .unwrap();
    assert!(claimed.is_empty());
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(10_000), DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(claimed.len(), 1);
}

#[test]
fn claim_excludes_items_before_their_next_attempt() {
    let store = Store::open_in_memory().unwrap();
    let waiting = store.enqueue(&sample("waiting", 1_000)).unwrap();
    let ready = store.enqueue(&sample("ready", 2_000)).unwrap();

    store
        .mark_retry([waiting], at(15_000), "temporary outage")
        .unwrap();

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(10_000), DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, ready);

    assert!(
        store
            .claim_batch(LASTFM_DESTINATION, at(14_999), DEFAULT_CLAIM_LIMIT)
            .unwrap()
            .is_empty()
    );

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(15_000), DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, waiting);
    assert_eq!(claimed[0].attempts, 1);
}

#[test]
fn delivery_states_transition_independently() {
    let store = Store::open_in_memory().unwrap();
    for session in ["submitted", "ignored", "failed", "retry"] {
        store.enqueue(&sample(session, 1_000)).unwrap();
    }

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(10_000), DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(claimed.len(), 4);

    let submitted = item_for(&claimed, "submitted").id;
    let ignored = item_for(&claimed, "ignored").id;
    let failed = item_for(&claimed, "failed").id;
    let retry = item_for(&claimed, "retry").id;

    store.mark_submitted([submitted]).unwrap();
    store
        .mark_ignored(ignored, 9, "the service ignored this scrobble")
        .unwrap();
    store
        .mark_failed([failed], "metadata is permanently invalid")
        .unwrap();
    store
        .mark_retry([retry], at(20_000), "rate limited")
        .unwrap();
    store
        .mark_retry([retry], at(25_000), "still rate limited")
        .unwrap();

    let items = store.recent(10).unwrap();

    let submitted_item = item_for(&items, "submitted");
    assert_eq!(submitted_item.state, DeliveryState::Submitted);
    assert_eq!(submitted_item.last_error, None);

    let ignored_item = item_for(&items, "ignored");
    assert_eq!(
        ignored_item.state,
        DeliveryState::Ignored {
            code: 9,
            message: "the service ignored this scrobble".to_owned(),
        }
    );
    assert_eq!(
        ignored_item.last_error.as_deref(),
        Some("the service ignored this scrobble")
    );

    let failed_item = item_for(&items, "failed");
    assert_eq!(
        failed_item.state,
        DeliveryState::Failed {
            permanent: true,
            message: "metadata is permanently invalid".to_owned(),
        }
    );

    let retry_item = item_for(&items, "retry");
    assert_eq!(retry_item.state, DeliveryState::Pending);
    assert_eq!(retry_item.attempts, 2);
    assert_eq!(retry_item.next_attempt_at, at(25_000));

    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);
}

#[test]
fn recover_in_flight_marks_outcome_as_uncertain() {
    let store = Store::open_in_memory().unwrap();
    store.enqueue(&sample("one", 1_000)).unwrap();
    store.enqueue(&sample("two", 2_000)).unwrap();

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(10_000), DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(claimed.len(), 2);

    assert_eq!(store.recover_in_flight().unwrap(), 2);
    assert_eq!(store.recover_in_flight().unwrap(), 0);
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 2);

    let items = store.recent(10).unwrap();
    for item in items {
        assert_eq!(item.state, DeliveryState::Pending);
        assert_eq!(item.last_error.as_deref(), Some("uncertain"));
        assert_eq!(item.attempts, 0);
    }
}

#[test]
fn destinations_keep_independent_delivery_state() {
    let store = Store::open_in_memory().unwrap();
    let scrobble = sample("shared", 1_000);

    let ids = store
        .enqueue_for(&scrobble, &[LASTFM_DESTINATION, "listenbrainz"])
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);
    assert_eq!(store.pending_count("listenbrainz").unwrap(), 1);

    let claimed = store
        .claim_batch(LASTFM_DESTINATION, at(10_000), DEFAULT_CLAIM_LIMIT)
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, ids[0]);
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 0);
    assert_eq!(store.pending_count("listenbrainz").unwrap(), 1);

    store.mark_submitted([claimed[0].id]).unwrap();
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 0);
    assert_eq!(store.pending_count("listenbrainz").unwrap(), 1);
}

#[test]
fn queue_survives_reopen_and_preserves_optional_track_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scrova-queue.sqlite3");

    let full = sample("full", 1_000);
    let minimal = minimal_sample("minimal", 2_000);

    {
        let store = Store::open(&path).unwrap();
        store.enqueue(&full).unwrap();
        store.enqueue(&minimal).unwrap();

        let claimed = store
            .claim_batch(LASTFM_DESTINATION, at(10_000), 1)
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].scrobble.session_id, "full");
    }

    {
        let store = Store::open(&path).unwrap();
        let items = store.recent(10).unwrap();
        assert_eq!(items.len(), 2);

        let full_item = item_for(&items, "full");
        assert_eq!(full_item.scrobble, full);
        assert_eq!(full_item.state, DeliveryState::InFlight);

        let minimal_item = item_for(&items, "minimal");
        assert_eq!(minimal_item.scrobble, minimal);
        assert_eq!(minimal_item.state, DeliveryState::Pending);
        assert_eq!(minimal_item.scrobble.track.album, None);
        assert_eq!(minimal_item.scrobble.track.album_artist, None);
        assert_eq!(minimal_item.scrobble.track.duration, None);
        assert_eq!(minimal_item.scrobble.track.source_track_id, None);

        assert_eq!(store.recover_in_flight().unwrap(), 1);
        assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 2);
    }
}

#[test]
fn recent_respects_limit_and_returns_deterministic_ids() {
    let store = Store::open_in_memory().unwrap();
    for session in ["one", "two", "three"] {
        store.enqueue(&sample(session, 1_000)).unwrap();
    }

    let items = store.recent(2).unwrap();
    assert_eq!(items.len(), 2);
    assert_ne!(items[0].id, items[1].id);
    assert!(store.recent(0).unwrap().is_empty());
}

#[test]
fn backoff_is_exponential_and_capped() {
    assert_eq!(backoff(0), Duration::from_secs(60));
    assert_eq!(backoff(1), Duration::from_secs(60));
    assert_eq!(backoff(2), Duration::from_secs(120));
    assert_eq!(backoff(3), Duration::from_secs(240));
    assert_eq!(backoff(6), Duration::from_secs(1_920));
    assert_eq!(backoff(7), MAX_BACKOFF);
    assert_eq!(backoff(100), MAX_BACKOFF);
}

#[test]
fn enqueue_for_rejects_an_empty_destination_list() {
    let store = Store::open_in_memory().unwrap();
    let error = store.enqueue_for(&sample("no-destination", 1_000), &[]);
    assert!(error.is_err());
}

#[test]
fn open_sets_wal_mode_and_schema_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scrova-queue.sqlite3");

    {
        let _store = Store::open(&path).unwrap();
    }

    let connection = rusqlite::Connection::open(&path).unwrap();
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");

    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
}

#[test]
fn newer_schema_versions_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scrova-queue.sqlite3");

    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.pragma_update(None, "user_version", 3).unwrap();
    }

    let error = Store::open(&path).unwrap_err();
    assert!(matches!(
        error,
        scrova_storage::StoreError::UnsupportedSchema {
            found: 3,
            supported: 2
        }
    ));
}

#[test]
fn queue_id_can_be_read_as_an_integer() {
    let store = Store::open_in_memory().unwrap();
    let id: QueueId = store.enqueue(&sample("integer", 1_000)).unwrap();
    assert!(id.get() > 0);
    assert_eq!(id.to_string(), id.get().to_string());
}

#[test]
fn enqueue_reported_distinguishes_a_duplicate_session() {
    let store = Store::open_in_memory().unwrap();
    let scrobble = sample("same-session", 1_000);

    let first = store.enqueue_reported(&scrobble).unwrap();
    let second = store.enqueue_reported(&scrobble).unwrap();

    let EnqueueOutcome::Queued(first_id) = first else {
        panic!("first enqueue should be new");
    };
    let EnqueueOutcome::AlreadyQueued(second_id) = second else {
        panic!("second enqueue should be a duplicate");
    };
    assert_eq!(first_id, second_id);
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);
}

#[test]
fn library_baseline_is_immutable_and_states_round_trip() {
    let store = Store::open_in_memory().unwrap();

    assert_eq!(store.library_baseline("com.apple.Music").unwrap(), None);
    store
        .set_library_baseline("com.apple.Music", at(1_000))
        .unwrap();
    store
        .set_library_baseline("com.apple.Music", at(2_000))
        .unwrap();
    assert_eq!(
        store.library_baseline("com.apple.Music").unwrap(),
        Some(at(1_000))
    );

    let states = vec![
        (
            "track-a".to_owned(),
            LibraryState {
                play_count: 3,
                last_played_at: Some(at(500)),
            },
        ),
        (
            "track-b".to_owned(),
            LibraryState {
                play_count: 0,
                last_played_at: None,
            },
        ),
    ];
    store
        .upsert_library_states("com.apple.Music", &states)
        .unwrap();

    let read = store
        .library_states(
            "com.apple.Music",
            &[
                "track-a".to_owned(),
                "track-b".to_owned(),
                "missing".to_owned(),
            ],
        )
        .unwrap();
    assert_eq!(read.len(), 2);
    assert_eq!(
        read["track-a"],
        LibraryState {
            play_count: 3,
            last_played_at: Some(at(500)),
        }
    );
    assert_eq!(
        read["track-b"],
        LibraryState {
            play_count: 0,
            last_played_at: None,
        }
    );

    let updated = vec![(
        "track-a".to_owned(),
        LibraryState {
            play_count: 4,
            last_played_at: Some(at(600)),
        },
    )];
    store
        .upsert_library_states("com.apple.Music", &updated)
        .unwrap();
    let read = store
        .library_states("com.apple.Music", &["track-a".to_owned()])
        .unwrap();
    assert_eq!(
        read["track-a"],
        LibraryState {
            play_count: 4,
            last_played_at: Some(at(600)),
        }
    );
}

#[test]
fn existing_scrobble_starts_is_inclusive_and_track_scoped() {
    let store = Store::open_in_memory().unwrap();

    let mut one = sample("one", 1_000);
    one.track.source_track_id = Some("track-1".to_owned());
    let mut two = sample("two", 2_000);
    two.track.source_track_id = Some("track-1".to_owned());
    let mut other = sample("other", 1_500);
    other.track.source_track_id = Some("track-2".to_owned());
    for scrobble in [&one, &two, &other] {
        store.enqueue(scrobble).unwrap();
    }

    assert_eq!(
        store
            .existing_scrobble_starts("track-1", at(1_000), at(2_000))
            .unwrap(),
        vec![at(1_000), at(2_000)]
    );
    assert!(
        store
            .existing_scrobble_starts("track-1", at(1_001), at(1_999))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn apply_library_scan_commits_baseline_states_and_scrobbles_together() {
    let store = Store::open_in_memory().unwrap();
    let mut recovered = minimal_sample("recovery", 5_000);
    recovered.track.source_track_id = Some("track-1".to_owned());
    let states = vec![(
        "track-1".to_owned(),
        LibraryState {
            play_count: 1,
            last_played_at: Some(at(5_000)),
        },
    )];

    let result = store
        .apply_library_scan(
            "com.apple.Music",
            LibraryScan {
                baseline: Some(at(1_000)),
                states: &states,
                scrobbles: std::slice::from_ref(&recovered),
            },
        )
        .unwrap();
    assert_eq!(
        result,
        LibraryScanResult {
            queued: 1,
            already_queued: 0,
        }
    );
    assert_eq!(
        store.library_baseline("com.apple.Music").unwrap(),
        Some(at(1_000))
    );
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);

    let result = store
        .apply_library_scan(
            "com.apple.Music",
            LibraryScan {
                baseline: None,
                states: &states,
                scrobbles: std::slice::from_ref(&recovered),
            },
        )
        .unwrap();
    assert_eq!(
        result,
        LibraryScanResult {
            queued: 0,
            already_queued: 1,
        }
    );
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);

    // A bad state rolls back the whole scan, including a newly attempted
    // baseline, instead of leaving half a snapshot behind.
    let bad_states = vec![(
        "track-2".to_owned(),
        LibraryState {
            play_count: u64::MAX,
            last_played_at: None,
        },
    )];
    assert!(
        store
            .apply_library_scan(
                "com.apple.Music",
                LibraryScan {
                    baseline: Some(at(9_000)),
                    states: &bad_states,
                    scrobbles: &[],
                },
            )
            .is_err()
    );
    assert_eq!(
        store.library_baseline("com.apple.Music").unwrap(),
        Some(at(1_000))
    );
}

#[test]
fn version_one_databases_are_migrated_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("v1.sqlite3");

    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE scrobbles (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL UNIQUE,
                    track_title TEXT NOT NULL,
                    track_artist TEXT NOT NULL,
                    track_album TEXT,
                    track_album_artist TEXT,
                    track_duration_ms INTEGER,
                    track_source_id TEXT,
                    started_at_ms INTEGER NOT NULL,
                    timestamp_accuracy TEXT NOT NULL,
                    origin TEXT NOT NULL,
                    source TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL
                );
                CREATE INDEX idx_scrobbles_started ON scrobbles (started_at_ms, id);
                CREATE TABLE deliveries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    scrobble_id INTEGER NOT NULL REFERENCES scrobbles (id) ON DELETE CASCADE,
                    destination TEXT NOT NULL,
                    state TEXT NOT NULL,
                    attempts INTEGER NOT NULL DEFAULT 0,
                    next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
                    last_error TEXT,
                    ignored_code INTEGER,
                    permanent_failure INTEGER NOT NULL DEFAULT 0,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE (scrobble_id, destination)
                );
                INSERT INTO scrobbles (
                    session_id, track_title, track_artist, track_album,
                    track_album_artist, track_duration_ms, track_source_id,
                    started_at_ms, timestamp_accuracy, origin, source, created_at_ms
                ) VALUES (
                    'old-session', 'Old Song', 'Old Artist', NULL,
                    NULL, NULL, 'old-track', 1000, 'observed',
                    'live_observation', 'com.apple.Music', 1000
                );
                INSERT INTO deliveries (
                    scrobble_id, destination, state, attempts, next_attempt_at_ms,
                    last_error, ignored_code, permanent_failure, created_at_ms, updated_at_ms
                ) VALUES (1, 'lastfm', 'pending', 0, 0, NULL, NULL, 0, 1000, 1000);
                PRAGMA user_version = 1;
                "#,
            )
            .unwrap();
    }

    let store = Store::open(&path).unwrap();
    let items = store.recent(10).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].scrobble.session_id, "old-session");
    assert_eq!(store.library_baseline("com.apple.Music").unwrap(), None);
    assert!(
        store
            .library_states("com.apple.Music", &["old-track".to_owned()])
            .unwrap()
            .is_empty()
    );

    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
}
