use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_core::{
    KnownPlay, NowPlayingHint, RECOVERY_MAX_AGE, RecentPlayedInput, RecentPlayedPlan,
    RecentSnapshot, RecoveryConfig, ScrobbleOrigin, TimestampAccuracy, Track, plan_recent_played,
    recent_played_key,
};

const SOURCE: &str = "com.apple.Music";

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn track(title: &str, duration_secs: u64) -> Track {
    Track {
        title: title.into(),
        artist: "Artist".into(),
        album: None,
        album_artist: None,
        duration: Some(Duration::from_secs(duration_secs)),
        source_track_id: Some(format!("catalog-{title}")),
    }
}

fn keys(items: &[Track]) -> Vec<String> {
    items.iter().map(recent_played_key).collect()
}

fn snapshot(items: &[Track], taken_at: SystemTime) -> RecentSnapshot {
    RecentSnapshot {
        keys: keys(items),
        taken_at,
    }
}

fn input(
    items: Vec<Track>,
    previous: Option<RecentSnapshot>,
    known: Vec<KnownPlay>,
    now_playing: Option<NowPlayingHint>,
    now: SystemTime,
) -> RecentPlayedInput {
    RecentPlayedInput {
        items,
        previous,
        known,
        now_playing,
        now,
        config: RecoveryConfig::default(),
    }
}

fn titles(plan: &RecentPlayedPlan) -> Vec<&str> {
    plan.plays
        .iter()
        .map(|play| play.track.title.as_str())
        .collect()
}

#[test]
fn first_scan_only_establishes_a_snapshot() {
    let items = vec![track("A", 200), track("B", 200)];
    let plan = plan_recent_played(input(items.clone(), None, Vec::new(), None, at(1_000)));

    assert!(plan.plays.is_empty());
    assert_eq!(plan.snapshot.keys, keys(&items));
    assert_eq!(plan.snapshot.taken_at, at(1_000));
}

#[test]
fn new_entry_is_placed_before_now_by_its_duration() {
    let previous_items = vec![track("A", 200), track("B", 200)];
    let items = vec![track("X", 200), track("A", 200), track("B", 200)];
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    ));

    assert_eq!(titles(&plan), ["X"]);
    assert_eq!(plan.plays[0].started_at, at(4_800));
    assert_eq!(
        plan.plays[0].timestamp_accuracy,
        TimestampAccuracy::Estimated
    );
    assert_eq!(plan.plays[0].origin, ScrobbleOrigin::HistoryRecovery);
    assert_eq!(plan.plays[0].source, SOURCE);
    assert!(
        plan.plays[0]
            .session_id
            .starts_with("recent:com.apple.Music:")
    );
}

#[test]
fn replayed_old_song_is_treated_as_new_when_it_jumps_to_the_top() {
    let previous_items = vec![
        track("A", 200),
        track("B", 200),
        track("C", 200),
        track("D", 200),
    ];
    let items = vec![
        track("B", 200),
        track("A", 200),
        track("C", 200),
        track("D", 200),
    ];
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    ));

    assert_eq!(titles(&plan), ["B"]);
    assert_eq!(plan.plays[0].started_at, at(4_800));
}

#[test]
fn now_playing_is_skipped_and_its_playhead_anchors_older_entries() {
    let previous_items = vec![track("A", 200), track("B", 200)];
    let items = vec![
        track("X", 200),
        track("Y", 200),
        track("A", 200),
        track("B", 200),
    ];
    let hint = NowPlayingHint {
        key: recent_played_key(&items[0]),
        position: Some(Duration::from_secs(50)),
        playing: true,
    };
    let plan = plan_recent_played(input(
        items.clone(),
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        Some(hint),
        at(5_000),
    ));

    assert_eq!(titles(&plan), ["Y"]);
    assert_eq!(plan.plays[0].started_at, at(4_750));
    assert_eq!(plan.snapshot.keys, keys(&items));
}

#[test]
fn paused_now_playing_is_still_skipped() {
    // Paused mid-song, it has not finished; resuming it later is the live
    // observer's play, so counting it here would duplicate that scrobble.
    let previous_items = vec![track("A", 200)];
    let items = vec![track("X", 200), track("Y", 200), track("A", 200)];
    let hint = NowPlayingHint {
        key: recent_played_key(&items[0]),
        position: Some(Duration::from_secs(50)),
        playing: false,
    };
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        Some(hint),
        at(5_000),
    ));

    assert_eq!(titles(&plan), ["Y"]);
    assert_eq!(plan.plays[0].started_at, at(4_750));
}

#[test]
fn known_play_is_suppressed_and_its_start_anchors_the_next_entry() {
    let previous_items = vec![track("A", 200)];
    let items = vec![track("D", 200), track("E", 200), track("A", 200)];
    let known = vec![KnownPlay {
        key: recent_played_key(&items[0]),
        started_at: at(4_900),
    }];
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        known,
        None,
        at(5_000),
    ));

    assert_eq!(titles(&plan), ["E"]);
    assert_eq!(plan.plays[0].started_at, at(4_700));
}

#[test]
fn start_is_clamped_to_the_previous_snapshot_time() {
    let previous_items = vec![track("A", 200)];
    let items = vec![track("X", 200), track("A", 200)];
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(5_000))),
        Vec::new(),
        None,
        at(5_100),
    ));

    assert_eq!(titles(&plan), ["X"]);
    assert_eq!(plan.plays[0].started_at, at(5_000));
}

#[test]
fn no_overlap_treats_every_item_as_new() {
    let previous_items = vec![track("A", 200), track("B", 200)];
    let items = vec![track("X", 200), track("Y", 200), track("Z", 200)];
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    ));

    assert_eq!(titles(&plan), ["Z", "Y", "X"]);
    assert_eq!(
        plan.plays
            .iter()
            .map(|play| play.started_at)
            .collect::<Vec<_>>(),
        [at(4_400), at(4_600), at(4_800)]
    );
}

#[test]
fn second_run_with_the_same_list_is_duplicate_free() {
    let previous_items = vec![track("A", 200), track("B", 200)];
    let items = vec![track("X", 200), track("A", 200), track("B", 200)];
    let first = plan_recent_played(input(
        items.clone(),
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    ));
    assert_eq!(titles(&first), ["X"]);

    let second = plan_recent_played(input(
        items,
        Some(first.snapshot.clone()),
        Vec::new(),
        None,
        at(5_100),
    ));
    assert!(second.plays.is_empty());
}

#[test]
fn unknown_or_short_tracks_do_not_shift_the_timeline() {
    let previous_items = vec![track("A", 200)];
    let mut short = track("S", 20);
    short.duration = Some(Duration::from_secs(20));
    let mut unknown = track("U", 200);
    unknown.duration = None;
    let items = vec![short, unknown, track("X", 200), track("A", 200)];
    let plan = plan_recent_played(input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    ));

    // S and U are skipped without consuming time, so X still starts at now - 200.
    assert_eq!(titles(&plan), ["X"]);
    assert_eq!(plan.plays[0].started_at, at(4_800));
}

#[test]
fn max_age_and_max_plays_are_enforced() {
    let previous_items = vec![track("A", 200)];
    let items = vec![
        track("X", 200),
        track("Y", 200),
        track("Z", 200),
        track("A", 200),
    ];
    let mut capped = input(
        items.clone(),
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    );
    capped.config = RecoveryConfig {
        max_age: RECOVERY_MAX_AGE,
        max_scrobbles: 2,
    };
    let plan = plan_recent_played(capped);
    assert_eq!(titles(&plan), ["Y", "X"]);

    let mut too_old = input(
        items,
        Some(snapshot(&previous_items, at(1_000))),
        Vec::new(),
        None,
        at(5_000),
    );
    too_old.config = RecoveryConfig {
        max_age: Duration::from_secs(100),
        max_scrobbles: 200,
    };
    assert!(plan_recent_played(too_old).plays.is_empty());
}
