//! End-to-end checks of the wiring: engine → queue → mock Last.fm → queue.

use std::time::{Duration, SystemTime};

use scrova_core::{
    LibraryItem, PlaybackEvent, PlaybackState, Scrobble, ScrobbleOrigin, TimestampAccuracy, Track,
};
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const START: u64 = 1_700_000_000;

fn track() -> Track {
    Track {
        title: "Test Track".into(),
        artist: "Test Artist".into(),
        album: None,
        album_artist: None,
        duration: Some(Duration::from_secs(200)),
        source_track_id: Some("ABC".into()),
    }
}

/// Plays `track()` far enough to produce exactly one scrobble.
fn play_one(scrova: &Scrova) {
    for s in (0..=100).step_by(5) {
        scrova
            .handle_playback_event(PlaybackEvent {
                track: Some(track()),
                state: PlaybackState::Playing,
                position: Some(Duration::from_secs(s)),
                source: "com.apple.Music".into(),
                observed_at: SystemTime::UNIX_EPOCH + Duration::from_secs(START + s),
            })
            .unwrap();
    }
}

fn library_track() -> Track {
    Track {
        title: "Library Track".into(),
        artist: "Library Artist".into(),
        album: None,
        album_artist: None,
        duration: Some(Duration::from_secs(200)),
        source_track_id: Some("LIB1".into()),
    }
}

fn library_item(play_count: u64, last_played_at: Option<SystemTime>) -> LibraryItem {
    LibraryItem {
        track: library_track(),
        play_count,
        last_played_at,
    }
}

async fn setup() -> (MockServer, Arc<Scrova>) {
    let server = MockServer::start().await;
    let lastfm = LastfmClient::with_base_url(Credentials::new("key", "secret"), server.uri());
    let scrova = Scrova::with_parts(Store::open_in_memory().unwrap(), lastfm).unwrap();
    (server, scrova)
}

fn lastfm_error(code: u32) -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_string(format!(r#"{{"error":{code},"message":"test"}}"#))
}

const ACCEPTED: &str = r##"{"scrobbles":{"scrobble":{"track":{"corrected":"0","#text":"Test Track"},"artist":{"corrected":"0","#text":"Test Artist"},"album":{"corrected":"0","#text":""},"albumArtist":{"corrected":"0","#text":""},"timestamp":"1700000000","ignoredMessage":{"code":"0","#text":""}},"@attr":{"accepted":"1","ignored":"0"}}}"##;

#[tokio::test]
async fn scrobbles_are_queued_then_flushed() {
    let (server, scrova) = setup().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=track.scrobble"))
        .and(body_string_contains("timestamp%5B0%5D=1700000000"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ACCEPTED))
        .expect(1)
        .mount(&server)
        .await;

    play_one(&scrova);
    assert_eq!(scrova.pending_count().unwrap(), 1);

    scrova.set_session_key(Some("sk".into()));
    let report = scrova.flush().await.unwrap();
    assert_eq!(
        report,
        FlushReport {
            submitted: 1,
            ..FlushReport::default()
        }
    );
    let recent = scrova.recent_scrobbles(10).unwrap();
    assert_eq!(recent[0].status, DeliveryStatus::Submitted);
}

#[tokio::test]
async fn flush_requires_sign_in_and_keeps_the_queue() {
    let (_server, scrova) = setup().await;
    play_one(&scrova);
    assert!(matches!(
        scrova.flush().await,
        Err(ScrovaError::NotSignedIn)
    ));
    assert_eq!(scrova.pending_count().unwrap(), 1);
}

#[tokio::test]
async fn transient_errors_reschedule_with_backoff() {
    let (server, scrova) = setup().await;
    Mock::given(method("POST"))
        .respond_with(lastfm_error(16))
        .mount(&server)
        .await;

    play_one(&scrova);
    scrova.set_session_key(Some("sk".into()));
    let report = scrova.flush().await.unwrap();
    assert_eq!(report.retry_scheduled, 1);
    assert_eq!(report.remaining, 1);

    // Not due yet, so a second flush sends nothing.
    let report = scrova.flush().await.unwrap();
    assert_eq!(report.retry_scheduled, 0);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_session_signs_out_and_keeps_the_batch_due() {
    let (server, scrova) = setup().await;
    Mock::given(method("POST"))
        .respond_with(lastfm_error(9))
        .mount(&server)
        .await;

    play_one(&scrova);
    scrova.set_session_key(Some("sk".into()));
    assert!(matches!(
        scrova.flush().await,
        Err(ScrovaError::SessionInvalid)
    ));
    assert!(!scrova.is_signed_in());

    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ACCEPTED))
        .mount(&server)
        .await;
    scrova.set_session_key(Some("new".into()));
    assert_eq!(scrova.flush().await.unwrap().submitted, 1);
}

#[tokio::test]
async fn malformed_request_errors_mark_items_failed_without_deleting_them() {
    let (server, scrova) = setup().await;
    Mock::given(method("POST"))
        .respond_with(lastfm_error(13))
        .mount(&server)
        .await;

    play_one(&scrova);
    scrova.set_session_key(Some("sk".into()));
    let report = scrova.flush().await.unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(report.remaining, 0);
    assert!(matches!(
        scrova.recent_scrobbles(1).unwrap()[0].status,
        DeliveryStatus::Failed { .. }
    ));
}

#[tokio::test]
async fn first_library_scan_only_establishes_the_baseline() {
    let (_server, scrova) = setup().await;
    let source = "com.apple.Music".to_owned();

    assert!(scrova.library_scanned_at(source.clone()).unwrap().is_none());

    let item = library_item(7, Some(SystemTime::now() - Duration::from_secs(60 * 60)));
    let report = scrova
        .recover_library_history(source.clone(), vec![item])
        .unwrap();

    assert_eq!(
        report,
        RecoveryReport {
            baseline_created: true,
            queued: 0,
            already_covered: 0,
        }
    );
    assert!(scrova.library_scanned_at(source).unwrap().is_some());
    assert_eq!(scrova.pending_count().unwrap(), 0);
}

#[tokio::test]
async fn second_scan_queues_one_estimated_recovery_and_is_idempotent() {
    let (_server, scrova) = setup().await;
    let source = "com.apple.Music".to_owned();

    let first_last_played = SystemTime::now() - Duration::from_secs(60 * 60);
    scrova
        .recover_library_history(
            source.clone(),
            vec![library_item(1, Some(first_last_played))],
        )
        .unwrap();
    let baseline = scrova
        .library_scanned_at(source.clone())
        .unwrap()
        .expect("baseline should exist");

    let last_played = baseline + Duration::from_secs(10);
    let item = library_item(2, Some(last_played));
    let report = scrova
        .recover_library_history(source.clone(), vec![item.clone()])
        .unwrap();

    assert_eq!(
        report,
        RecoveryReport {
            baseline_created: false,
            queued: 1,
            already_covered: 0,
        }
    );
    let records = scrova.recent_scrobbles(10).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].timestamp_accuracy, TimestampAccuracy::Estimated);
    assert_eq!(records[0].origin, ScrobbleOrigin::HistoryRecovery);
    assert_eq!(
        records[0].started_at,
        last_played - Duration::from_secs(200)
    );
    assert_eq!(scrova.pending_count().unwrap(), 1);

    let again = scrova.recover_library_history(source, vec![item]).unwrap();
    assert_eq!(
        again,
        RecoveryReport {
            baseline_created: false,
            queued: 0,
            already_covered: 0,
        }
    );
    assert_eq!(scrova.pending_count().unwrap(), 1);
}

#[tokio::test]
async fn a_live_scrobble_covers_the_recovered_candidate() {
    let (_server, scrova) = setup().await;
    let source = "com.apple.Music".to_owned();

    let first_last_played = SystemTime::now() - Duration::from_secs(60 * 60);
    scrova
        .recover_library_history(
            source.clone(),
            vec![library_item(1, Some(first_last_played))],
        )
        .unwrap();
    let baseline = scrova
        .library_scanned_at(source.clone())
        .unwrap()
        .expect("baseline should exist");

    let last_played = baseline + Duration::from_secs(10);
    let recovered_start = last_played - Duration::from_secs(200);
    scrova
        .store
        .enqueue(&Scrobble {
            track: library_track(),
            started_at: recovered_start,
            timestamp_accuracy: TimestampAccuracy::Observed,
            origin: ScrobbleOrigin::LiveObservation,
            source: source.clone(),
            session_id: "live-session".to_owned(),
        })
        .unwrap();

    let report = scrova
        .recover_library_history(source, vec![library_item(2, Some(last_played))])
        .unwrap();

    assert_eq!(
        report,
        RecoveryReport {
            baseline_created: false,
            queued: 0,
            already_covered: 1,
        }
    );
    assert_eq!(scrova.pending_count().unwrap(), 1);
    let records = scrova.recent_scrobbles(10).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].origin, ScrobbleOrigin::LiveObservation);
    assert_eq!(records[0].timestamp_accuracy, TimestampAccuracy::Observed);
}

#[tokio::test]
async fn unapproved_token_reports_pending_authorization() {
    let (server, scrova) = setup().await;
    Mock::given(method("POST"))
        .respond_with(lastfm_error(14))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(lastfm_error(14))
        .mount(&server)
        .await;

    let result = scrova.complete_authorization("token".into()).await;
    assert!(
        matches!(result, Err(ScrovaError::AuthorizationPending)),
        "{result:?}"
    );
    assert!(!scrova.is_signed_in());
}

#[tokio::test]
async fn restarting_midway_does_not_queue_the_same_play_twice() {
    let (_server, scrova) = setup().await;
    play_one(&scrova);
    assert_eq!(scrova.pending_count().unwrap(), 1);

    // A fresh engine (app restart) joins the same play at 120s, 20s later.
    *lock(&scrova.engine) = ScrobbleEngine::new(EngineConfig::default());
    for s in [120, 125] {
        scrova
            .handle_playback_event(PlaybackEvent {
                track: Some(track()),
                state: PlaybackState::Playing,
                position: Some(Duration::from_secs(s)),
                source: "com.apple.Music".into(),
                observed_at: SystemTime::UNIX_EPOCH + Duration::from_secs(START + s),
            })
            .unwrap();
    }
    assert_eq!(scrova.pending_count().unwrap(), 1);
}

fn history_scrobble(session: &str, title: &str, started_at: SystemTime) -> Scrobble {
    Scrobble {
        track: Track {
            title: title.into(),
            artist: "History Artist".into(),
            album: None,
            album_artist: None,
            duration: Some(Duration::from_secs(200)),
            source_track_id: Some(session.into()),
        },
        started_at,
        timestamp_accuracy: TimestampAccuracy::Observed,
        origin: ScrobbleOrigin::LiveObservation,
        source: "com.apple.Music".into(),
        session_id: session.into(),
    }
}

#[tokio::test]
async fn scrobble_history_pages_newest_first_with_a_deterministic_cursor() {
    let (_server, scrova) = setup().await;
    for (session, title, seconds) in [("a", "A", 1_000), ("b", "B", 2_000), ("c", "C", 2_000)] {
        scrova
            .store
            .enqueue(&history_scrobble(
                session,
                title,
                SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            ))
            .unwrap();
    }

    let first = scrova.scrobble_history(2, None).unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.track.title.as_str())
            .collect::<Vec<_>>(),
        ["C", "B"]
    );
    let next = first.next.expect("one older row remains");

    let second = scrova.scrobble_history(2, Some(next)).unwrap();
    assert_eq!(
        second
            .items
            .iter()
            .map(|item| item.track.title.as_str())
            .collect::<Vec<_>>(),
        ["A"]
    );
    assert!(second.next.is_none());
}

#[tokio::test]
async fn scrobble_history_next_is_none_at_an_exact_boundary() {
    let (_server, scrova) = setup().await;
    for (session, seconds) in [("newer", 2_000), ("older", 1_000)] {
        scrova
            .store
            .enqueue(&history_scrobble(
                session,
                session,
                SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            ))
            .unwrap();
    }

    let page = scrova.scrobble_history(2, None).unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.next.is_none());
}

#[tokio::test]
async fn scrobble_history_handles_zero_limit_and_empty_history() {
    let (_server, scrova) = setup().await;

    let empty = scrova.scrobble_history(10, None).unwrap();
    assert!(empty.items.is_empty());
    assert!(empty.next.is_none());

    scrova
        .store
        .enqueue(&history_scrobble(
            "one",
            "One",
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
        ))
        .unwrap();
    let zero = scrova.scrobble_history(0, None).unwrap();
    assert!(zero.items.is_empty());
    assert!(zero.next.is_none());
}

#[tokio::test]
async fn scrobble_history_reports_delivery_status() {
    let (_server, scrova) = setup().await;
    let id = scrova
        .store
        .enqueue(&history_scrobble(
            "submitted",
            "Submitted",
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
        ))
        .unwrap();
    scrova.store.mark_submitted([id]).unwrap();

    let page = scrova.scrobble_history(10, None).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].status, DeliveryStatus::Submitted);
    assert_eq!(page.items[0].track.title, "Submitted");
}

#[tokio::test]
async fn restart_dedupe_falls_back_to_title_and_artist_without_an_id() {
    let (_server, scrova) = setup().await;
    let mut t = track();
    t.source_track_id = None;
    let event = |s: u64| PlaybackEvent {
        track: Some(t.clone()),
        state: PlaybackState::Playing,
        position: Some(Duration::from_secs(s)),
        source: "com.example.player".into(),
        observed_at: SystemTime::UNIX_EPOCH + Duration::from_secs(START + s),
    };
    for s in (0..=100).step_by(5) {
        scrova.handle_playback_event(event(s)).unwrap();
    }
    assert_eq!(scrova.pending_count().unwrap(), 1);

    *lock(&scrova.engine) = ScrobbleEngine::new(EngineConfig::default());
    scrova.handle_playback_event(event(120)).unwrap();
    assert_eq!(scrova.pending_count().unwrap(), 1);
}

#[tokio::test]
async fn restart_dedupe_covers_a_start_that_drifted_with_a_pause() {
    let (_server, scrova) = setup().await;
    let t = track();
    let event = |position: u64, at: u64| PlaybackEvent {
        track: Some(t.clone()),
        state: PlaybackState::Playing,
        position: Some(Duration::from_secs(position)),
        source: "com.apple.Music".into(),
        observed_at: SystemTime::UNIX_EPOCH + Duration::from_secs(START + at),
    };
    for s in (0..=120).step_by(5) {
        scrova.handle_playback_event(event(s, s)).unwrap();
    }
    assert_eq!(scrova.pending_count().unwrap(), 1);

    // Paused at 125s for 90s, then the app is relaunched and resumes: the
    // estimated start lands 90s after the real one.
    *lock(&scrova.engine) = ScrobbleEngine::new(EngineConfig::default());
    scrova.handle_playback_event(event(125, 215)).unwrap();
    assert_eq!(scrova.pending_count().unwrap(), 1);
}
