use std::time::{Duration, SystemTime};

use scrova_core::{
    EngineConfig, EngineOutput, PlaybackEvent, PlaybackState, Scrobble, ScrobbleEngine, Track,
    scrobble_threshold,
};

const SOURCE: &str = "com.apple.Music";

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

fn at(s: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + secs(1_700_000_000 + s)
}

fn track(id: &str, duration: Option<u64>) -> Track {
    Track {
        title: format!("Title {id}"),
        artist: "Artist".into(),
        album: Some("Album".into()),
        album_artist: None,
        duration: duration.map(secs),
        source_track_id: Some(id.into()),
    }
}

fn ev(track: &Track, state: PlaybackState, position: u64, t: u64) -> PlaybackEvent {
    PlaybackEvent {
        track: Some(track.clone()),
        state,
        position: Some(secs(position)),
        source: SOURCE.into(),
        observed_at: at(t),
    }
}

fn scrobbles(outputs: &[EngineOutput]) -> Vec<&Scrobble> {
    outputs
        .iter()
        .filter_map(|o| match o {
            EngineOutput::Scrobble { scrobble } => Some(scrobble),
            _ => None,
        })
        .collect()
}

fn now_playing_count(outputs: &[EngineOutput]) -> usize {
    outputs
        .iter()
        .filter(|o| matches!(o, EngineOutput::NowPlaying { .. }))
        .count()
}

/// Feeds events and collects every output.
fn run(
    engine: &mut ScrobbleEngine,
    events: impl IntoIterator<Item = PlaybackEvent>,
) -> Vec<EngineOutput> {
    events.into_iter().flat_map(|e| engine.handle(e)).collect()
}

/// Simulates an observer polling every `step` seconds while playing normally.
fn play_through(
    t: &Track,
    from_pos: u64,
    to_pos: u64,
    start_t: u64,
    step: u64,
) -> Vec<PlaybackEvent> {
    (0..=(to_pos - from_pos) / step)
        .map(|i| {
            ev(
                t,
                PlaybackState::Playing,
                from_pos + i * step,
                start_t + i * step,
            )
        })
        .collect()
}

#[test]
fn threshold_follows_lastfm_rules() {
    let c = EngineConfig::default();
    assert_eq!(
        scrobble_threshold(Some(secs(30)), &c),
        None,
        "30s exactly is not longer than 30s"
    );
    assert_eq!(
        scrobble_threshold(Some(secs(31)), &c),
        Some(Duration::from_millis(15_500))
    );
    assert_eq!(scrobble_threshold(Some(secs(200)), &c), Some(secs(100)));
    assert_eq!(
        scrobble_threshold(Some(secs(600)), &c),
        Some(secs(240)),
        "capped at 4 minutes"
    );
    assert_eq!(scrobble_threshold(None, &c), Some(secs(240)));
}

#[test]
fn scrobbles_once_at_half_with_observed_start_time() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let out = run(&mut engine, play_through(&t, 0, 200, 0, 5));

    let s = scrobbles(&out);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].started_at, at(0));
    assert_eq!(now_playing_count(&out), 1);
}

#[test]
fn scrobble_is_emitted_when_threshold_is_crossed_not_at_track_end() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    assert!(scrobbles(&run(&mut engine, play_through(&t, 0, 95, 0, 5))).is_empty());
    assert_eq!(
        scrobbles(&run(
            &mut engine,
            [ev(&t, PlaybackState::Playing, 100, 100)]
        ))
        .len(),
        1
    );
}

#[test]
fn short_tracks_are_never_scrobbled() {
    let mut engine = ScrobbleEngine::default();
    let t = track("short", Some(30));
    assert!(scrobbles(&run(&mut engine, play_through(&t, 0, 30, 0, 1))).is_empty());
}

#[test]
fn long_tracks_need_four_minutes() {
    let mut engine = ScrobbleEngine::default();
    let t = track("long", Some(1200));
    assert!(scrobbles(&run(&mut engine, play_through(&t, 0, 235, 0, 5))).is_empty());
    assert_eq!(
        scrobbles(&run(
            &mut engine,
            [ev(&t, PlaybackState::Playing, 240, 240)]
        ))
        .len(),
        1
    );
}

#[test]
fn paused_time_is_not_counted() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let mut events = play_through(&t, 0, 60, 0, 5);
    events.push(ev(&t, PlaybackState::Paused, 60, 60));
    // An hour on pause; the playhead does not move.
    events.push(ev(&t, PlaybackState::Paused, 60, 3660));
    events.push(ev(&t, PlaybackState::Playing, 60, 3660));
    events.push(ev(&t, PlaybackState::Playing, 99, 3699));
    assert!(
        scrobbles(&run(&mut engine, events)).is_empty(),
        "only 99s played"
    );

    assert_eq!(
        scrobbles(&run(
            &mut engine,
            [ev(&t, PlaybackState::Playing, 100, 3700)]
        ))
        .len(),
        1
    );
}

#[test]
fn resuming_from_pause_resends_now_playing() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let out = run(
        &mut engine,
        [
            ev(&t, PlaybackState::Playing, 0, 0),
            ev(&t, PlaybackState::Paused, 10, 10),
            ev(&t, PlaybackState::Playing, 10, 100),
        ],
    );
    assert_eq!(now_playing_count(&out), 2);
}

#[test]
fn forward_seek_is_not_counted_as_listening() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let out = run(
        &mut engine,
        [
            ev(&t, PlaybackState::Playing, 0, 0),
            ev(&t, PlaybackState::Playing, 10, 10),
            // Seek to near the end within one polling interval.
            ev(&t, PlaybackState::Playing, 190, 15),
            ev(&t, PlaybackState::Playing, 199, 24),
        ],
    );
    assert!(scrobbles(&out).is_empty());
}

#[test]
fn backward_seek_mid_track_keeps_the_session() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let mut events = play_through(&t, 0, 80, 0, 5);
    // Jump back to 40s: not near the start, so it is a seek, not a repeat.
    events.extend(play_through(&t, 40, 60, 85, 5));
    let out = run(&mut engine, events);
    let s = scrobbles(&out);
    assert_eq!(s.len(), 1, "80s + 5s + 20s of listening crosses 100s");
    assert_eq!(s[0].started_at, at(0));
    assert_eq!(now_playing_count(&out), 1);
}

#[test]
fn repeat_one_creates_a_new_session_and_scrobble() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let mut events = play_through(&t, 0, 195, 0, 5);
    // Loops back to the start: the observer sees the playhead at 3s.
    events.extend(play_through(&t, 3, 198, 203, 5));
    let out = run(&mut engine, events);

    let s = scrobbles(&out);
    assert_eq!(s.len(), 2);
    assert_ne!(s[0].session_id, s[1].session_id);
    assert_eq!(s[0].started_at, at(0));
    assert_eq!(s[1].started_at, at(200));
}

#[test]
fn restart_before_threshold_far_from_end_is_treated_as_seek() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let mut events = play_through(&t, 0, 50, 0, 5);
    events.extend(play_through(&t, 0, 50, 55, 5));
    let out = run(&mut engine, events);
    let s = scrobbles(&out);
    assert_eq!(s.len(), 1, "both passes accumulate into one session");
    assert_eq!(s[0].started_at, at(0));
}

#[test]
fn same_title_and_artist_with_different_ids_are_different_tracks() {
    let mut engine = ScrobbleEngine::default();
    let a = track("a", Some(200));
    let mut b = track("b", Some(200));
    b.title = a.title.clone();

    let mut events = play_through(&a, 0, 100, 0, 5);
    events.extend(play_through(&b, 0, 100, 100, 5));
    let out = run(&mut engine, events);
    assert_eq!(scrobbles(&out).len(), 2);
}

#[test]
fn track_change_closes_previous_session_without_overcounting() {
    let mut engine = ScrobbleEngine::default();
    let a = track("a", Some(100));
    let b = track("b", Some(300));
    let out = run(
        &mut engine,
        [
            ev(&a, PlaybackState::Playing, 0, 0),
            ev(&a, PlaybackState::Playing, 40, 40),
            // The next event arrives late (sparse notifications), already on track b.
            ev(&b, PlaybackState::Playing, 5, 200),
        ],
    );
    let s = scrobbles(&out);
    assert_eq!(
        s.len(),
        1,
        "a played 40s observed + at most 60s remaining = 100s >= 50s"
    );
    assert_eq!(s[0].track.source_track_id.as_deref(), Some("a"));
    assert_eq!(now_playing_count(&out), 2);
}

#[test]
fn stop_ends_the_session() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let mut events = play_through(&t, 0, 60, 0, 5);
    events.push(PlaybackEvent {
        track: None,
        state: PlaybackState::Stopped,
        position: None,
        source: SOURCE.into(),
        observed_at: at(65),
    });
    events.extend(play_through(&t, 0, 60, 70, 5));
    assert!(
        scrobbles(&run(&mut engine, events)).is_empty(),
        "two separate 60-65s listens"
    );
}

#[test]
fn unknown_position_falls_back_to_wall_clock_capped_by_duration() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let no_pos = |state, s| PlaybackEvent {
        position: None,
        ..ev(&t, state, 0, s)
    };
    // Machine slept for 8 hours without a pause event.
    let out = run(
        &mut engine,
        [
            no_pos(PlaybackState::Playing, 0),
            no_pos(PlaybackState::Playing, 8 * 3600),
        ],
    );
    let s = scrobbles(&out);
    assert_eq!(
        s.len(),
        1,
        "credit is capped at 200s, which still meets the 100s threshold"
    );

    let mut engine = ScrobbleEngine::default();
    let out = run(
        &mut engine,
        [
            no_pos(PlaybackState::Playing, 0),
            no_pos(PlaybackState::Playing, 99),
        ],
    );
    assert!(scrobbles(&out).is_empty());
}

#[test]
fn out_of_order_events_are_ignored() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let out = run(
        &mut engine,
        [
            ev(&t, PlaybackState::Playing, 0, 0),
            ev(&t, PlaybackState::Playing, 50, 50),
            ev(&t, PlaybackState::Playing, 20, 20),
            ev(&t, PlaybackState::Playing, 100, 100),
        ],
    );
    assert_eq!(scrobbles(&out).len(), 1);
}

#[test]
fn sources_are_tracked_independently() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let other = |e: PlaybackEvent| PlaybackEvent {
        source: "com.spotify.client".into(),
        ..e
    };
    let mut events = Vec::new();
    for e in play_through(&t, 0, 100, 0, 5) {
        events.push(other(e.clone()));
        events.push(e);
    }
    let out = run(&mut engine, events);
    let s = scrobbles(&out);
    assert_eq!(s.len(), 2);
    assert_ne!(s[0].source, s[1].source);
}

#[test]
fn tracks_without_artist_or_title_are_not_submitted() {
    let mut engine = ScrobbleEngine::default();
    let mut t = track("stream", Some(200));
    t.artist = "  ".into();
    let out = run(&mut engine, play_through(&t, 0, 200, 0, 5));
    assert!(out.is_empty(), "neither Now Playing nor Scrobble: {out:?}");
}

#[test]
fn progress_reports_credited_time_and_scrobble_state() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    assert!(engine.progress(SOURCE).is_none());

    run(&mut engine, play_through(&t, 0, 60, 0, 5));
    let progress = engine.progress(SOURCE).unwrap();
    assert_eq!(progress.played, secs(60));
    assert_eq!(progress.threshold, Some(secs(100)));
    assert!(!progress.scrobbled);
    assert_eq!(progress.position, Some(secs(60)));
    assert_eq!(progress.observed_at, at(60));

    run(&mut engine, play_through(&t, 65, 100, 65, 5));
    assert!(engine.progress(SOURCE).unwrap().scrobbled);
}

#[test]
fn progress_has_no_threshold_for_tracks_that_never_scrobble() {
    let mut engine = ScrobbleEngine::default();
    let short = track("short", Some(30));
    run(&mut engine, [ev(&short, PlaybackState::Playing, 0, 0)]);
    assert_eq!(engine.progress(SOURCE).unwrap().threshold, None);
}

#[test]
fn joining_midway_credits_the_playhead() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    // Observation starts at 90s into a 200s track; threshold is 100s.
    let out = run(&mut engine, play_through(&t, 90, 100, 0, 5));
    assert_eq!(scrobbles(&out).len(), 1);
    assert_eq!(scrobbles(&out)[0].started_at, at(0) - secs(90));
}

#[test]
fn joining_midway_credit_is_capped_by_duration() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    run(&mut engine, [ev(&t, PlaybackState::Paused, 500, 0)]);
    assert_eq!(engine.progress(SOURCE).unwrap().played, secs(200));
}

#[test]
fn joining_past_the_threshold_scrobbles_without_another_event() {
    let mut engine = ScrobbleEngine::default();
    let t = track("a", Some(200));
    let out = run(&mut engine, [ev(&t, PlaybackState::Paused, 150, 0)]);
    assert_eq!(scrobbles(&out).len(), 1);
    assert!(engine.progress(SOURCE).unwrap().scrobbled);
}

#[test]
fn track_change_does_not_credit_a_stale_playhead() {
    let mut engine = ScrobbleEngine::default();
    let a = track("a", Some(200));
    let b = track("b", Some(200));
    let mut events = play_through(&a, 0, 150, 0, 5);
    // The player reports b's metadata with a's position for a moment.
    events.push(ev(&b, PlaybackState::Playing, 150, 152));
    events.push(ev(&b, PlaybackState::Playing, 3, 155));
    let out = run(&mut engine, events);
    let s = scrobbles(&out);
    assert_eq!(s.len(), 1, "only a: {s:?}");
    assert_eq!(s[0].track.title, "Title a");
}

#[test]
fn metadata_filled_in_stages_stays_one_play() {
    let mut engine = ScrobbleEngine::default();
    let mut bare = track("a", None);
    bare.source_track_id = None;
    bare.album = None;
    let mut full = track("a", Some(200));
    full.source_track_id = None;
    let mut events = vec![ev(&bare, PlaybackState::Playing, 0, 0)];
    events.extend(play_through(&full, 3, 120, 3, 5));
    let out = run(&mut engine, events);
    assert_eq!(scrobbles(&out).len(), 1);
}
