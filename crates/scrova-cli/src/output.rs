use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use scrova_core::{EngineOutput, ScrobbleOrigin, TimestampAccuracy};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::ReplayError;

pub(crate) fn write_output<W: Write>(
    writer: &mut W,
    output: &EngineOutput,
    json: bool,
) -> Result<(), ReplayError> {
    if json {
        write_json(writer, output)
    } else {
        write_human(writer, output)
    }
}

fn write_human<W: Write>(writer: &mut W, output: &EngineOutput) -> Result<(), ReplayError> {
    // NowPlaying carries no session metadata in the core enum; the session id and
    // start time belong to the Scrobble output that closes/qualifies the session.
    match output {
        EngineOutput::NowPlaying { track, source } => {
            writeln!(
                writer,
                "NowPlaying  {} — {}  source={source}",
                track.title, track.artist
            )?;
        }
        EngineOutput::Scrobble { scrobble } => {
            let started_at = format_started_at(scrobble.started_at)?;
            let session = session_prefix(&scrobble.session_id);
            writeln!(
                writer,
                "Scrobble    {} — {}  started_at={started_at}  session={session}  source={}",
                scrobble.track.title, scrobble.track.artist, scrobble.source
            )?;
        }
    }
    Ok(())
}

fn write_json<W: Write>(writer: &mut W, output: &EngineOutput) -> Result<(), ReplayError> {
    let value = match output {
        EngineOutput::NowPlaying { track, source } => serde_json::json!({
            "type": "now_playing",
            "title": track.title.as_str(),
            "artist": track.artist.as_str(),
            "source": source.as_str(),
        }),
        EngineOutput::Scrobble { scrobble } => serde_json::json!({
            "type": "scrobble",
            "title": scrobble.track.title.as_str(),
            "artist": scrobble.track.artist.as_str(),
            "album": scrobble.track.album.as_deref(),
            "started_at": format_started_at(scrobble.started_at)?,
            "session_id": scrobble.session_id.as_str(),
            "source": scrobble.source.as_str(),
            "timestamp_accuracy": timestamp_accuracy(scrobble.timestamp_accuracy),
            "origin": scrobble_origin(scrobble.origin),
        }),
    };

    // Value's Display implementation writes compact JSON, which keeps one
    // engine event per line without an intermediate String allocation.
    writeln!(writer, "{value}")?;
    Ok(())
}

fn format_started_at(time: SystemTime) -> Result<String, ReplayError> {
    let nanos = system_time_to_unix_nanos(time)
        .ok_or_else(|| ReplayError::TimeFormat("timestamp is out of range".to_owned()))?;
    let datetime = OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|error| ReplayError::TimeFormat(error.to_string()))?;
    datetime
        .format(&Rfc3339)
        .map_err(|error| ReplayError::TimeFormat(error.to_string()))
}

fn system_time_to_unix_nanos(time: SystemTime) -> Option<i128> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).ok(),
        Err(error) => i128::try_from(error.duration().as_nanos())
            .ok()
            .map(|nanos| -nanos),
    }
}

fn session_prefix(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn timestamp_accuracy(accuracy: TimestampAccuracy) -> &'static str {
    match accuracy {
        TimestampAccuracy::Observed => "observed",
        TimestampAccuracy::Estimated => "estimated",
    }
}

fn scrobble_origin(origin: ScrobbleOrigin) -> &'static str {
    match origin {
        ScrobbleOrigin::LiveObservation => "live_observation",
        ScrobbleOrigin::HistoryRecovery => "history_recovery",
    }
}
