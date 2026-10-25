use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::model::{Scrobble, ScrobbleOrigin, TimestampAccuracy, Track};
use crate::recovery::RecoveryConfig;

const KEY_SEPARATOR: char = '\u{1f}';
const MIN_SCROBBLE_DURATION: Duration = Duration::from_secs(30);

/// Source used by the iOS MusicKit integration. The planner is source-agnostic;
/// callers that need another source use [`plan_recent_played_for_source`].
pub const DEFAULT_SOURCE: &str = "com.apple.Music";

/// Snapshot of the keys seen on the previous scan.
///
/// The API returns a de-duplicated list at its latest play, so the keys are
/// enough to find what was already visible; no timestamps are available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSnapshot {
    pub keys: Vec<String>,
    pub taken_at: SystemTime,
}

/// A play already persisted by another path. The 1 hour overlap lets the
/// planner suppress songs that were live-scrobbled just before the previous
/// snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPlay {
    pub key: String,
    pub started_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlayingHint {
    pub key: String,
    pub position: Option<Duration>,
    pub playing: bool,
}

/// Input for [`plan_recent_played`].
#[derive(Debug, Clone, PartialEq)]
pub struct RecentPlayedInput {
    /// Newest first, exactly as returned by MusicKit.
    pub items: Vec<Track>,
    pub previous: Option<RecentSnapshot>,
    pub known: Vec<KnownPlay>,
    pub now_playing: Option<NowPlayingHint>,
    pub now: SystemTime,
    pub config: RecoveryConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecentPlayedPlan {
    /// Oldest first, so callers can enqueue in chronological order.
    pub plays: Vec<Scrobble>,
    pub snapshot: RecentSnapshot,
}

/// Identity key shared by the planner and the FFI now-playing hint.
///
/// Catalog and library IDs differ for the same song, so IDs cannot be used
/// across MusicKit and MediaPlayer. Artist/title is the only common identity.
#[must_use]
pub fn recent_played_key(track: &Track) -> String {
    recent_played_key_parts(&track.artist, &track.title)
}

/// Key builder for callers that only have metadata columns.
#[must_use]
pub fn recent_played_key_parts(artist: &str, title: &str) -> String {
    format!(
        "{}{KEY_SEPARATOR}{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

/// Plans scrobbles from a MusicKit "recently played" list.
///
/// The list has no timestamps, so new entries are chained backwards from `now`
/// (or the current playhead) using track durations. The result is deliberately
/// conservative: a known play or a continuous session wins over guessing.
#[must_use]
pub fn plan_recent_played(input: RecentPlayedInput) -> RecentPlayedPlan {
    plan_recent_played_for_source(input, DEFAULT_SOURCE)
}

/// Same as [`plan_recent_played`], for callers that need a different source
/// identifier in the emitted scrobbles and deterministic session ids.
#[must_use]
pub fn plan_recent_played_for_source(input: RecentPlayedInput, source: &str) -> RecentPlayedPlan {
    let snapshot = RecentSnapshot {
        keys: input.items.iter().map(recent_played_key).collect(),
        taken_at: input.now,
    };

    let Some(previous) = input.previous else {
        // The first scan only establishes a baseline, just like library
        // recovery: the list can contain a lifetime of plays.
        return RecentPlayedPlan {
            plays: Vec::new(),
            snapshot,
        };
    };

    let new_count = new_prefix_len(&previous.keys, &snapshot.keys);
    if new_count == 0 || input.config.max_scrobbles == 0 {
        return RecentPlayedPlan {
            plays: Vec::new(),
            snapshot,
        };
    }

    let mut start_index = 0;
    let mut anchor = input.now;
    if let Some(hint) = input.now_playing.as_ref()
        && snapshot.keys.first() == Some(&hint.key)
    {
        // The song still in the player has not finished, playing or paused:
        // the live observer owns it, and counting it here would duplicate
        // that scrobble once it resumes. Its playhead gives the end time for
        // the next older entry.
        start_index = 1;
        anchor = hint
            .position
            .and_then(|position| input.now.checked_sub(position))
            .unwrap_or(input.now);
    }

    let known = latest_known_by_key(&input.known);
    let cutoff = input
        .now
        .checked_sub(input.config.max_age)
        .unwrap_or(UNIX_EPOCH);
    let mut selected = Vec::new();
    let mut seen_sessions = HashSet::new();

    for index in start_index..new_count {
        let key = &snapshot.keys[index];

        // The list is de-duplicated at the latest play, so a same-key play in
        // the overlap window is indistinguishable from this entry. Skipping it
        // is safer than queueing a duplicate; its real start also anchors the
        // older entries better than an estimate from `now`.
        let track = &input.items[index];
        // Only a known play that was still running at the previous scan can be
        // this entry; one that ended before it means the song was played again.
        if let Some(known_start) = known.get(key.as_str()).copied()
            && known_start + track.duration.unwrap_or_default() >= previous.taken_at
        {
            anchor = known_start;
            continue;
        }

        let Some(duration) = track
            .duration
            .filter(|duration| *duration > MIN_SCROBBLE_DURATION)
        else {
            // Unknown and very short tracks cannot be placed on the timeline.
            // Leaving the anchor alone keeps older entries from shifting.
            continue;
        };

        let Some(candidate_start) = anchor.checked_sub(duration) else {
            break;
        };
        // A helper backs up at most a track length; anything older belongs to
        // the previous scan's window and cannot be distinguished from a skip.
        let start = candidate_start.max(previous.taken_at);
        anchor = start;

        if start < cutoff {
            break;
        }

        let session_id = format!(
            "recent:{}:{:016x}:{}",
            source,
            key_hash(key),
            unix_millis(start)
        );
        if !seen_sessions.insert(session_id.clone()) {
            continue;
        }

        selected.push(Scrobble {
            track: track.clone(),
            started_at: start,
            timestamp_accuracy: TimestampAccuracy::Estimated,
            // A distinct origin would require a schema/UI migration; the
            // existing estimated+recovery pair already describes this play.
            origin: ScrobbleOrigin::HistoryRecovery,
            source: source.to_owned(),
            session_id,
        });

        if selected.len() >= input.config.max_scrobbles {
            break;
        }
    }

    // The loop walked newest to oldest; callers persist oldest first.
    selected.reverse();

    RecentPlayedPlan {
        plays: selected,
        snapshot,
    }
}

/// Smallest number of leading `current` keys that are not a continuation of
/// `previous`.
///
/// Removing the new prefix from the previous list and checking the remainder as
/// a subsequence also tolerates songs dropping off the 30-item window. A
/// replayed old song appears in the prefix, so it is treated as new.
fn new_prefix_len(previous: &[String], current: &[String]) -> usize {
    for prefix in 0..=current.len() {
        let removed: HashSet<&str> = current[..prefix].iter().map(String::as_str).collect();
        let remaining: Vec<&str> = previous
            .iter()
            .map(String::as_str)
            .filter(|key| !removed.contains(key))
            .collect();
        if is_subsequence(&current[prefix..], &remaining) {
            return prefix;
        }
    }
    current.len()
}

fn is_subsequence(current: &[String], previous: &[&str]) -> bool {
    let mut previous = previous.iter();
    current
        .iter()
        .all(|key| previous.any(|candidate| candidate == key))
}

fn latest_known_by_key(known: &[KnownPlay]) -> HashMap<&str, SystemTime> {
    let mut latest = HashMap::new();
    for play in known {
        latest
            .entry(play.key.as_str())
            .and_modify(|started_at: &mut SystemTime| {
                if play.started_at > *started_at {
                    *started_at = play.started_at;
                }
            })
            .or_insert(play.started_at);
    }
    latest
}

fn key_hash(key: &str) -> u64 {
    // FNV-1a is tiny and deterministic across app restarts, which is all the
    // session id needs; it is not used for security.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn unix_millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}
