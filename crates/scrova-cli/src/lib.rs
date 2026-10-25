//! Replay helper for the Scrova engine.
//!
//! The binary is intentionally thin: this module owns the deterministic replay
//! loop so integration tests can feed sample JSON Lines and count engine output
//! without spawning a process.

mod input;
mod output;

use std::io::{BufRead, Write};

use scrova_core::{EngineOutput, ScrobbleEngine};
use scrova_storage::{LASTFM_DESTINATION, Store};
use thiserror::Error;

/// Source used when an event omits `source`.
pub const DEFAULT_SOURCE: &str = "com.apple.Music";

/// Fixed epoch for relative `t` values. Replays are deterministic, so a
/// wall-clock "now" would make sample output and tests unstable.
pub const RELATIVE_EPOCH_UNIX_SECONDS: u64 = 1_700_000_000;

/// Output format for engine events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplayConfig {
    pub json: bool,
}

/// Counts collected while replaying.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayOutcome {
    pub events: usize,
    pub now_playing: usize,
    pub scrobbles: usize,
    /// Populated only when a store was supplied.
    pub pending_count: Option<u64>,
}

/// Errors from parsing input, writing output, or persisting scrobbles.
#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("line {line}: invalid JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("line {line}: {message}")]
    Input { line: usize, message: String },
    #[error("storage error: {0}")]
    Storage(#[from] scrova_storage::StoreError),
    #[error("could not format timestamp: {0}")]
    TimeFormat(String),
}

/// Replays JSON Lines events through a fresh [`ScrobbleEngine`].
///
/// When `store` is present, every emitted scrobble is enqueued immediately;
/// the final pending count is returned for the caller to display. The writer is
/// flushed before returning so a caller can safely print a summary afterwards.
pub fn replay<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
    config: ReplayConfig,
    store: Option<&Store>,
) -> Result<ReplayOutcome, ReplayError> {
    let mut engine = ScrobbleEngine::default();
    let mut outcome = ReplayOutcome::default();

    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let event = input::parse_event(line, line_number)?;
        outcome.events += 1;

        for output in engine.handle(event) {
            match &output {
                EngineOutput::NowPlaying { .. } => outcome.now_playing += 1,
                EngineOutput::Scrobble { scrobble } => {
                    outcome.scrobbles += 1;
                    if let Some(store) = store {
                        store.enqueue(scrobble)?;
                    }
                }
            }
            output::write_output(writer, &output, config.json)?;
        }
    }

    writer.flush()?;
    if let Some(store) = store {
        outcome.pending_count = Some(store.pending_count(LASTFM_DESTINATION)?);
    }

    Ok(outcome)
}
