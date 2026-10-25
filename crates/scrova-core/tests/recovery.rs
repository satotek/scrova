use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_core::{LibraryItem, LibraryState, RecoveryConfig, Track, plan_recovery};

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn duration(seconds: u64) -> Option<Duration> {
    Some(Duration::from_secs(seconds))
}

fn item(
    play_count: u64,
    last_played_at: Option<SystemTime>,
    track_duration: Option<Duration>,
) -> LibraryItem {
    LibraryItem {
        track: Track {
            title: "Song".to_owned(),
            artist: "Artist".to_owned(),
            album: None,
            album_artist: None,
            duration: track_duration,
            source_track_id: Some("track-1".to_owned()),
        },
        play_count,
        last_played_at,
    }
}

fn state(play_count: u64, last_played_at: Option<SystemTime>) -> LibraryState {
    LibraryState {
        play_count,
        last_played_at,
    }
}

fn plan(
    item: &LibraryItem,
    previous: Option<&LibraryState>,
    existing: &[SystemTime],
) -> scrova_core::RecoveryPlan {
    plan_recovery(
        item,
        previous,
        at(1_000),
        existing,
        at(10_000),
        &RecoveryConfig::default(),
    )
}

#[test]
fn first_seen_after_the_baseline_queues_the_latest_start_only() {
    let item = item(10, Some(at(5_000)), duration(200));

    let plan = plan(&item, None, &[]);

    assert_eq!(plan.starts, vec![at(4_800)]);
    assert_eq!(plan.already_covered, 0);
}

#[test]
fn first_seen_before_the_baseline_is_silent() {
    let item = item(10, Some(at(900)), duration(200));

    let plan = plan(&item, None, &[]);

    assert!(plan.starts.is_empty());
}

#[test]
fn known_delta_reconstructs_back_to_back_before_the_latest_end() {
    let previous = state(1, Some(at(1_000)));
    let item = item(3, Some(at(2_000)), duration(200));

    let plan = plan(&item, Some(&previous), &[]);

    assert_eq!(plan.starts, vec![at(1_600), at(1_800)]);
}

#[test]
fn unchanged_counter_or_timestamp_is_silent() {
    let previous = state(2, Some(at(2_000)));

    assert!(
        plan(
            &item(2, Some(at(3_000)), duration(200)),
            Some(&previous),
            &[]
        )
        .starts
        .is_empty()
    );
    assert!(
        plan(
            &item(3, Some(at(2_000)), duration(200)),
            Some(&previous),
            &[]
        )
        .starts
        .is_empty()
    );
    assert!(
        plan(
            &item(3, Some(at(1_500)), duration(200)),
            Some(&previous),
            &[]
        )
        .starts
        .is_empty()
    );
}

#[test]
fn a_decreased_counter_is_a_library_reset_not_a_recovery() {
    let previous = state(5, Some(at(2_000)));
    let item = item(4, Some(at(3_000)), duration(200));

    assert!(plan(&item, Some(&previous), &[]).starts.is_empty());
}

#[test]
fn unplaceable_items_are_skipped() {
    let previous = state(1, Some(at(1_000)));

    let unknown_duration = item(2, Some(at(2_000)), None);
    assert!(
        plan(&unknown_duration, Some(&previous), &[])
            .starts
            .is_empty()
    );

    let no_last_played = item(2, None, duration(200));
    assert!(
        plan(&no_last_played, Some(&previous), &[])
            .starts
            .is_empty()
    );

    let zero_duration = item(2, Some(at(2_000)), duration(0));
    assert!(plan(&zero_duration, Some(&previous), &[]).starts.is_empty());

    let mut missing_id = item(2, Some(at(2_000)), duration(200));
    missing_id.track.source_track_id = None;
    assert!(plan(&missing_id, Some(&previous), &[]).starts.is_empty());
}

#[test]
fn candidates_at_or_before_the_previous_play_are_dropped() {
    let previous = state(1, Some(at(1_750)));
    let latest = item(3, Some(at(2_000)), duration(200));

    // Latest start is 1800; the older 1600 would precede the previous play.
    assert_eq!(plan(&latest, Some(&previous), &[]).starts, vec![at(1_800)]);

    let previous = state(1, Some(at(1_800)));
    let equal = item(2, Some(at(2_000)), duration(200));
    assert!(plan(&equal, Some(&previous), &[]).starts.is_empty());
}

#[test]
fn candidates_older_than_the_recovery_window_are_dropped() {
    let now = 100 * 24 * 60 * 60;
    let day = 24 * 60 * 60;
    let config = RecoveryConfig::default();

    let fresh = item(1, Some(at(now - day)), duration(200));
    let plan = plan_recovery(&fresh, None, at(0), &[], at(now), &config);
    assert_eq!(plan.starts.len(), 1);

    let stale = item(1, Some(at(now - 20 * day)), duration(200));
    let plan = plan_recovery(&stale, None, at(0), &[], at(now), &config);
    assert!(plan.starts.is_empty());
}

#[test]
fn an_existing_scrobble_within_tolerance_covers_the_candidate() {
    let previous = state(1, Some(at(1_000)));
    let item = item(2, Some(at(2_000)), duration(200));

    let covered = plan(&item, Some(&previous), &[at(1_899)]);
    assert!(covered.starts.is_empty());
    assert_eq!(covered.already_covered, 1);

    // The tolerance for a 200s track is 100s; 250s away is a different play
    // (and outside the previous-play count window).
    let outside = plan(&item, Some(&previous), &[at(2_050)]);
    assert_eq!(outside.starts, vec![at(1_800)]);
    assert_eq!(outside.already_covered, 0);
}

#[test]
fn existing_scrobbles_in_the_count_window_limit_new_candidates() {
    let previous = state(1, Some(at(1_000)));
    let item = item(4, Some(at(2_000)), duration(100));

    // Two live scrobbles in (old.last - duration, last] account for two of
    // the three plays, so only the latest candidate remains.
    let plan = plan(&item, Some(&previous), &[at(1_990), at(1_991)]);

    assert_eq!(plan.starts, vec![at(1_900)]);
    assert_eq!(plan.already_covered, 2);
}

#[test]
fn the_configured_cap_keeps_the_most_recent_candidates() {
    let previous = state(1, Some(at(0)));
    let item = item(11, Some(at(1_000)), duration(60));
    let config = RecoveryConfig {
        max_scrobbles: 3,
        ..RecoveryConfig::default()
    };

    let plan = plan_recovery(&item, Some(&previous), at(0), &[], at(10_000), &config);

    assert_eq!(plan.starts, vec![at(820), at(880), at(940)]);
}
