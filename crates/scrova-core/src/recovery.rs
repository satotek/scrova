use std::cmp::max;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::model::LibraryItem;

/// Last.fm rejects scrobbles older than this in practice, so recovered plays
/// past the window are not worth queueing.
pub const RECOVERY_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Upper bound for one recovery scan. Older candidates are discarded so a
/// long-unused library cannot stall the queue.
pub const MAX_RECOVERY_SCROBBLES: usize = 200;

/// Previous snapshot state for one track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryState {
    pub play_count: u64,
    pub last_played_at: Option<SystemTime>,
}

/// Tunables for history recovery. Exposed so callers and tests can shorten the
/// age window without changing the algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryConfig {
    pub max_age: Duration,
    pub max_scrobbles: usize,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_age: RECOVERY_MAX_AGE,
            max_scrobbles: MAX_RECOVERY_SCROBBLES,
        }
    }
}

/// Start times (oldest first) that should be queued, plus how many candidates
/// an already-known scrobble made unnecessary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryPlan {
    pub starts: Vec<SystemTime>,
    pub already_covered: u32,
}

/// Reconstruct plays from one library snapshot item.
///
/// The caller supplies the current snapshot item, the previously stored state,
/// the baseline scan time, nearby scrobble starts for the same track, and
/// `now`. No I/O or clock reads happen here, which keeps the rules testable.
///
/// `last_played_at` is treated as the *end* of the most recent play. When the
/// duration is unknown the play cannot be placed, so the item is skipped.
#[must_use]
pub fn plan_recovery(
    item: &LibraryItem,
    previous: Option<&LibraryState>,
    baseline_scanned_at: SystemTime,
    existing_scrobbles: &[SystemTime],
    now: SystemTime,
    config: &RecoveryConfig,
) -> RecoveryPlan {
    let Some(track_id) = item.track.source_track_id.as_deref() else {
        return RecoveryPlan::default();
    };
    if track_id.is_empty() {
        return RecoveryPlan::default();
    }

    let Some(duration) = item.track.duration.filter(|duration| !duration.is_zero()) else {
        return RecoveryPlan::default();
    };
    let Some(last_played_at) = item.last_played_at else {
        return RecoveryPlan::default();
    };
    let Some(latest_start) = last_played_at.checked_sub(duration) else {
        return RecoveryPlan::default();
    };

    let (candidate_count, previous_last) = match previous {
        None => {
            // A first-seen item is only interesting if it was played after the
            // baseline. Otherwise it is part of the pre-existing library.
            if last_played_at <= baseline_scanned_at {
                return RecoveryPlan::default();
            }
            (1, None)
        }
        Some(old) => {
            if item.play_count < old.play_count {
                // The library was reset or its counters were rewritten; there
                // is no trustworthy delta to recover.
                return RecoveryPlan::default();
            }
            let delta = item.play_count - old.play_count;
            if delta == 0 {
                return RecoveryPlan::default();
            }
            if old
                .last_played_at
                .is_some_and(|old_last| last_played_at <= old_last)
            {
                return RecoveryPlan::default();
            }
            (delta, old.last_played_at)
        }
    };

    if config.max_scrobbles == 0 {
        return RecoveryPlan::default();
    }

    // Reconstruct only the latest candidates. If delta is huge (for example a
    // library reset that still increased the counter), older plays would be
    // discarded by the cap anyway.
    let candidate_limit = usize::try_from(candidate_count)
        .unwrap_or(usize::MAX)
        .min(config.max_scrobbles);
    let min_allowed = now.checked_sub(config.max_age).unwrap_or(UNIX_EPOCH);

    let mut candidates = Vec::with_capacity(candidate_limit);
    for index in 0..candidate_limit {
        let Ok(index) = u32::try_from(index) else {
            break;
        };
        let Some(back) = duration.checked_mul(index) else {
            break;
        };
        let Some(start) = latest_start.checked_sub(back) else {
            break;
        };
        // Candidates are generated newest first, so once one is too old or
        // predates the previous known play, every older candidate is too.
        if start < min_allowed {
            break;
        }
        if previous_last.is_some_and(|old_last| start <= old_last) {
            break;
        }
        candidates.push(start);
    }

    let existing_in_window = previous_last.map_or(0, |old_last| {
        let lower = old_last.checked_sub(duration);
        existing_scrobbles
            .iter()
            .filter(|&&start| lower.is_none_or(|lower| start > lower) && start <= last_played_at)
            .count() as u64
    });
    let max_new = candidate_count.saturating_sub(existing_in_window);

    // A live scrobble can land a little before or after the reconstructed
    // start; half a track length is enough to absorb clock/tag drift, with a
    // floor for very short tracks.
    let tolerance = max(duration / 2, Duration::from_secs(30));

    let mut selected = Vec::new();
    let mut already_covered = 0_u32;
    for start in candidates {
        if existing_scrobbles
            .iter()
            .any(|existing| within_tolerance(*existing, start, tolerance))
        {
            already_covered = already_covered.saturating_add(1);
            continue;
        }
        if selected.len() as u64 >= max_new {
            // Existing scrobbles in the count window account for these plays
            // even if their timestamps did not match within the tolerance.
            already_covered = already_covered.saturating_add(1);
            continue;
        }
        selected.push(start);
    }

    // Callers enqueue in chronological order; the cap selection above worked
    // newest-first because that is the useful end of the delta.
    selected.reverse();

    RecoveryPlan {
        starts: selected,
        already_covered,
    }
}

fn within_tolerance(left: SystemTime, right: SystemTime, tolerance: Duration) -> bool {
    match left.duration_since(right) {
        Ok(difference) => difference <= tolerance,
        Err(error) => error.duration() <= tolerance,
    }
}
