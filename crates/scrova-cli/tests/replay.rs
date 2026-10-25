use std::io::Cursor;

use scrova_cli::{ReplayConfig, ReplayError, ReplayOutcome, replay};
use scrova_storage::{LASTFM_DESTINATION, Store};

const NORMAL: &str = include_str!("../examples/normal.jsonl");
const PAUSE: &str = include_str!("../examples/pause.jsonl");
const FORWARD_SEEK: &str = include_str!("../examples/forward_seek.jsonl");
const REPEAT_ONE: &str = include_str!("../examples/repeat_one.jsonl");
const SLEEP: &str = include_str!("../examples/sleep.jsonl");
const ALBUM: &str = include_str!("../examples/album.jsonl");

fn replay_sample(sample: &str, json: bool, store: Option<&Store>) -> (ReplayOutcome, String) {
    let mut output = Vec::new();
    let outcome = replay(
        Cursor::new(sample.as_bytes()),
        &mut output,
        ReplayConfig { json },
        store,
    )
    .unwrap();
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn sample_scenarios_produce_expected_scrobble_counts() {
    let cases = [
        ("normal", NORMAL, 1, 1),
        ("pause", PAUSE, 1, 2),
        ("forward seek", FORWARD_SEEK, 0, 1),
        ("repeat one", REPEAT_ONE, 2, 2),
        ("sleep", SLEEP, 1, 1),
        ("album", ALBUM, 3, 3),
    ];

    for (name, sample, scrobbles, now_playing) in cases {
        let (outcome, _) = replay_sample(sample, false, None);
        assert_eq!(
            outcome.scrobbles, scrobbles,
            "{name}: unexpected scrobble count"
        );
        assert_eq!(
            outcome.now_playing, now_playing,
            "{name}: unexpected NowPlaying count"
        );
        assert_eq!(outcome.pending_count, None);
    }
}

#[test]
fn normal_sample_accepts_iso8601_and_emits_human_readable_output() {
    let (outcome, output) = replay_sample(NORMAL, false, None);
    assert_eq!(outcome.events, 22);
    assert_eq!(outcome.scrobbles, 1);

    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("NowPlaying  Night Drive — Example Artist"));
    assert!(lines[1].starts_with("Scrobble    Night Drive — Example Artist"));
    assert!(lines[1].contains("started_at=2026-01-01T00:00:00Z"));
    let session = lines[1]
        .split("session=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap();
    assert_eq!(session.len(), 8);
}

#[test]
fn json_mode_emits_one_parseable_object_per_engine_output() {
    let (outcome, output) = replay_sample(NORMAL, true, None);
    assert_eq!(outcome.scrobbles, 1);

    let values: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(values.len(), 2);

    assert_eq!(values[0]["type"], "now_playing");
    assert_eq!(values[0]["title"], "Night Drive");
    assert_eq!(values[0]["source"], "com.apple.Music");

    let scrobble = &values[1];
    assert_eq!(scrobble["type"], "scrobble");
    assert_eq!(scrobble["title"], "Night Drive");
    assert_eq!(scrobble["artist"], "Example Artist");
    assert_eq!(scrobble["started_at"], "2026-01-01T00:00:00Z");
    assert_eq!(scrobble["timestamp_accuracy"], "observed");
    assert_eq!(scrobble["origin"], "live_observation");
    let session_id = scrobble["session_id"].as_str().unwrap();
    assert_eq!(session_id.len(), 36);
}

#[test]
fn iso8601_without_timezone_is_treated_as_utc() {
    let sample = concat!(
        r#"{"t":"2026-01-01T00:00:00","state":"playing","pos":0,"track":{"id":"naive","title":"Naive Time","artist":"Example Artist","duration":200}}"#,
        "\n",
        r#"{"t":"2026-01-01T00:01:40","state":"playing","pos":100,"track":{"id":"naive","title":"Naive Time","artist":"Example Artist","duration":200}}"#,
        "\n",
    );

    let (outcome, output) = replay_sample(sample, false, None);
    assert_eq!(outcome.scrobbles, 1);
    assert!(output.contains("started_at=2026-01-01T00:00:00Z"));
}

#[test]
fn store_receives_emitted_scrobbles_and_pending_count() {
    let store = Store::open_in_memory().unwrap();
    let (outcome, _) = replay_sample(NORMAL, false, Some(&store));

    assert_eq!(outcome.scrobbles, 1);
    assert_eq!(outcome.pending_count, Some(1));
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 1);

    let recent = store.recent(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].scrobble.track.title, "Night Drive");
    assert_eq!(recent[0].scrobble.session_id.len(), 36);
}

#[test]
fn album_scenario_enqueues_every_scrobble() {
    let store = Store::open_in_memory().unwrap();
    let (outcome, _) = replay_sample(ALBUM, false, Some(&store));

    assert_eq!(outcome.scrobbles, 3);
    assert_eq!(outcome.pending_count, Some(3));
    assert_eq!(store.pending_count(LASTFM_DESTINATION).unwrap(), 3);
}

#[test]
fn invalid_json_reports_the_line_number() {
    let mut output = Vec::new();
    let error = replay(
        Cursor::new(b"{\"state\":\"playing\"}\n".as_slice()),
        &mut output,
        ReplayConfig::default(),
        None,
    )
    .unwrap_err();

    assert!(matches!(error, ReplayError::Json { line: 1, .. }));
}
