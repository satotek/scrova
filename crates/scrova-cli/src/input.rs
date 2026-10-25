use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_core::{PlaybackEvent, PlaybackState, Track};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::{DEFAULT_SOURCE, RELATIVE_EPOCH_UNIX_SECONDS, ReplayError};

const NAIVE_DATE_TIME_FORMAT: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");

#[derive(Debug, Deserialize)]
struct RawEvent {
    t: RawTime,
    state: RawState,
    #[serde(default)]
    pos: Option<f64>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    track: Option<RawTrack>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawTime {
    Seconds(f64),
    Iso(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Deserialize)]
struct RawTrack {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    album_artist: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
}

pub(crate) fn parse_event(line: &str, line_number: usize) -> Result<PlaybackEvent, ReplayError> {
    let raw: RawEvent = serde_json::from_str(line).map_err(|source| ReplayError::Json {
        line: line_number,
        source,
    })?;
    raw.into_event(line_number)
}

impl RawEvent {
    fn into_event(self, line_number: usize) -> Result<PlaybackEvent, ReplayError> {
        let observed_at = match self.t {
            RawTime::Seconds(seconds) => relative_time(seconds),
            RawTime::Iso(value) => parse_iso8601(&value),
        }
        .map_err(|message| input_error(line_number, message))?;

        let position = self
            .pos
            .map(|seconds| seconds_to_duration(seconds, "pos"))
            .transpose()
            .map_err(|message| input_error(line_number, message))?;

        let track = self
            .track
            .map(|track| track.into_track(line_number))
            .transpose()?;

        let source = self
            .source
            .and_then(non_empty)
            .unwrap_or_else(|| DEFAULT_SOURCE.to_owned());

        Ok(PlaybackEvent {
            track,
            state: self.state.into_state(),
            position,
            source,
            observed_at,
        })
    }
}

impl RawState {
    fn into_state(self) -> PlaybackState {
        match self {
            Self::Playing => PlaybackState::Playing,
            Self::Paused => PlaybackState::Paused,
            Self::Stopped => PlaybackState::Stopped,
        }
    }
}

impl RawTrack {
    fn into_track(self, line_number: usize) -> Result<Track, ReplayError> {
        let duration = self
            .duration
            .map(|seconds| seconds_to_duration(seconds, "track.duration"))
            .transpose()
            .map_err(|message| input_error(line_number, message))?;

        Ok(Track {
            title: self.title.unwrap_or_default(),
            artist: self.artist.unwrap_or_default(),
            album: self.album.and_then(non_empty),
            album_artist: self.album_artist.and_then(non_empty),
            duration,
            source_track_id: self.id.and_then(non_empty),
        })
    }
}

fn input_error(line: usize, message: String) -> ReplayError {
    ReplayError::Input { line, message }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn relative_time(seconds: f64) -> Result<SystemTime, String> {
    let offset = seconds_to_duration(seconds, "t")?;
    let base = UNIX_EPOCH + Duration::from_secs(RELATIVE_EPOCH_UNIX_SECONDS);
    base.checked_add(offset)
        .ok_or_else(|| "t is outside the supported range".to_owned())
}

fn seconds_to_duration(value: f64, field: &str) -> Result<Duration, String> {
    Duration::try_from_secs_f64(value)
        .map_err(|_| format!("{field} must be a finite, non-negative number of seconds"))
}

fn parse_iso8601(value: &str) -> Result<SystemTime, String> {
    let value = value.trim();
    let parsed = match OffsetDateTime::parse(value, &Rfc3339) {
        Ok(parsed) => parsed,
        Err(_) => {
            let naive = PrimitiveDateTime::parse(value, NAIVE_DATE_TIME_FORMAT)
                .map_err(|_| format!("t must be RFC3339 or YYYY-MM-DDTHH:MM:SS (got {value:?})"))?;
            naive.assume_utc()
        }
    };

    system_time_from_unix_nanos(parsed.unix_timestamp_nanos())
        .ok_or_else(|| "t is outside the supported range".to_owned())
}

fn system_time_from_unix_nanos(nanos: i128) -> Option<SystemTime> {
    if nanos >= 0 {
        let nanos = u64::try_from(nanos).ok()?;
        UNIX_EPOCH.checked_add(Duration::from_nanos(nanos))
    } else {
        let nanos = u64::try_from(nanos.unsigned_abs()).ok()?;
        UNIX_EPOCH.checked_sub(Duration::from_nanos(nanos))
    }
}
