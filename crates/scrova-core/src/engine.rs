use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::model::{
    EngineOutput, PlaybackEvent, PlaybackState, Scrobble, ScrobbleOrigin, SessionProgress,
    TimestampAccuracy, Track,
};

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Tracks must be strictly longer than this to be scrobbled (Last.fm: 30s).
    pub min_track_duration: Duration,
    /// Upper bound of the required listening time (Last.fm: 4 minutes).
    pub max_required_play: Duration,
    /// A backward jump landing within this many seconds of the start may be a
    /// repeat rather than a seek. Must exceed the observer's polling interval.
    pub repeat_start_window: Duration,
    /// How close to the end the previous position must be for a jump back to the
    /// start to count as a repeat even before the threshold was reached.
    pub repeat_end_window: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            min_track_duration: Duration::from_secs(30),
            max_required_play: Duration::from_secs(240),
            repeat_start_window: Duration::from_secs(10),
            repeat_end_window: Duration::from_secs(10),
        }
    }
}

/// Listening time required before a track may be scrobbled, or `None` if the
/// track is too short to ever qualify.
///
/// With an unknown duration only the 4-minute rule can be applied; reaching it
/// also proves the track is longer than the minimum.
pub fn scrobble_threshold(duration: Option<Duration>, config: &EngineConfig) -> Option<Duration> {
    match duration {
        Some(d) if d <= config.min_track_duration => None,
        Some(d) => Some((d / 2).min(config.max_required_play)),
        None => Some(config.max_required_play),
    }
}

#[derive(Debug)]
struct Session {
    id: String,
    track: Track,
    started_at: SystemTime,
    played: Duration,
    scrobbled: bool,
    state: PlaybackState,
    last_observed_at: SystemTime,
    last_position: Option<Duration>,
    /// The player reported a stop. Players also report one for a long pause
    /// (Music on iPhone does), so the session is kept and resumes if the same
    /// track carries on from where it was.
    stopped: bool,
}

/// Longer than any gap between observations while we are watching (Android
/// re-reads every 3s, iOS polls while open). A track found after a longer gap
/// was not seen starting, so it is joined midway.
const JOIN_GAP: Duration = Duration::from_secs(30);

/// A stopped track that comes back this far behind where it stopped was
/// started again rather than resumed.
const RESUME_TOLERANCE: Duration = Duration::from_secs(5);

/// Turns a stream of [`PlaybackEvent`]s into Now Playing updates and scrobbles.
///
/// The engine never reads the system clock: all elapsed time is derived from
/// `observed_at`, which keeps it deterministic and independent of how often a
/// native observer reports.
#[derive(Debug, Default)]
pub struct ScrobbleEngine {
    config: EngineConfig,
    sessions: HashMap<String, Session>,
}

impl ScrobbleEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            sessions: HashMap::new(),
        }
    }

    /// The session currently open for `source`, if the player has a track.
    pub fn progress(&self, source: &str) -> Option<SessionProgress> {
        let session = self.sessions.get(source)?;
        Some(SessionProgress {
            track: session.track.clone(),
            state: session.state,
            played: session.played,
            threshold: session
                .track
                .is_submittable()
                .then(|| scrobble_threshold(session.track.duration, &self.config))
                .flatten(),
            scrobbled: session.scrobbled,
            position: session.last_position,
            observed_at: session.last_observed_at,
        })
    }

    pub fn handle(&mut self, event: PlaybackEvent) -> Vec<EngineOutput> {
        let mut outputs = Vec::new();
        // A join is a track we did not see start: the first observation of a
        // player, or one after a gap in which we were not watching (the iOS
        // app returning from the background). A track change seen while
        // watching is not one.
        let joining = self.sessions.get(&event.source).is_none_or(|session| {
            event
                .observed_at
                .duration_since(session.last_observed_at)
                .is_ok_and(|gap| gap > JOIN_GAP)
        });

        if let Some(mut session) = self.sessions.remove(&event.source) {
            // Out-of-order delivery would make elapsed time negative; the newer
            // snapshot already describes the player better than this one.
            if event.observed_at < session.last_observed_at {
                self.sessions.insert(event.source, session);
                return outputs;
            }

            let same_track = event
                .track
                .as_ref()
                .is_some_and(|t| t.same_track_as(&session.track));

            if session.state == PlaybackState::Playing {
                session.played += self.credit(&session, &event, same_track);
            }
            self.scrobble_if_due(&mut session, &event.source, &mut outputs);

            if event.state == PlaybackState::Stopped {
                session.state = PlaybackState::Paused;
                session.stopped = true;
                session.last_observed_at = event.observed_at;
                self.sessions.insert(event.source, session);
                return outputs;
            }

            let restarted_after_stop = session.stopped
                && matches!(
                    (session.last_position, event.position),
                    (Some(prev), Some(now)) if now + RESUME_TOLERANCE < prev
                );
            let continues =
                same_track && !restarted_after_stop && !self.is_repeat(&session, &event);

            if continues {
                if session.state == PlaybackState::Paused
                    && event.state == PlaybackState::Playing
                    && session.track.is_submittable()
                {
                    outputs.push(EngineOutput::NowPlaying {
                        track: session.track.clone(),
                        source: event.source.clone(),
                    });
                }
                // Same track, possibly with metadata the player filled in
                // since (album, length); the threshold and the scrobble should
                // use the most complete version.
                if let Some(track) = event.track {
                    session.track = track;
                }
                session.state = event.state;
                session.stopped = false;
                session.last_observed_at = event.observed_at;
                session.last_position = event.position;
                self.sessions.insert(event.source, session);
                return outputs;
            }
        }

        self.start_session(event, joining, &mut outputs);
        outputs
    }

    fn start_session(
        &mut self,
        event: PlaybackEvent,
        joining: bool,
        outputs: &mut Vec<EngineOutput>,
    ) {
        let Some(track) = event.track else { return };
        if event.state == PlaybackState::Stopped {
            return;
        }

        // When we join a track midway, assume it played continuously up to now;
        // that is the best available estimate of when listening began. For a
        // track seen starting, a position past the first seconds is the
        // previous track's, still reported with the new metadata.
        let position = event
            .position
            .filter(|p| joining || *p <= self.config.repeat_start_window);
        let started_at = position
            .and_then(|p| event.observed_at.checked_sub(p))
            .unwrap_or(event.observed_at);

        // Joining midway (the observer just started, or the iOS app was opened
        // with Music already playing) credits what came before the playhead.
        // The start time above already assumes continuous playback; leaving
        // the listening time at zero would contradict it, and on iOS, where
        // opening the app mid-song is normal, most tracks would never qualify.
        //
        // A track change while observing is not a join. Players can still
        // report the previous track's position with the new metadata (seen
        // with YouTube Music on Android and Music on iPhone), and crediting it
        // would scrobble the new track the moment it starts.
        let played = position
            .map(|position| track.duration.map_or(position, |d| position.min(d)))
            .unwrap_or_default();

        if event.state == PlaybackState::Playing && track.is_submittable() {
            outputs.push(EngineOutput::NowPlaying {
                track: track.clone(),
                source: event.source.clone(),
            });
        }

        let mut session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            track,
            started_at,
            played,
            scrobbled: false,
            state: event.state,
            last_observed_at: event.observed_at,
            last_position: position,
            stopped: false,
        };
        // Joining a playing track past the threshold scrobbles now rather
        // than on the next report. A paused one waits until it plays: reopening
        // the app on a paused track is not a new listen, and its estimated
        // start drifts with the pause, so it would not match the first one.
        if event.state == PlaybackState::Playing {
            self.scrobble_if_due(&mut session, &event.source, outputs);
        }
        self.sessions.insert(event.source, session);
    }

    /// Listening time to add for the interval since the previous observation,
    /// during which the session was playing.
    fn credit(&self, session: &Session, event: &PlaybackEvent, same_track: bool) -> Duration {
        let wall = event
            .observed_at
            .duration_since(session.last_observed_at)
            .unwrap_or_default();

        let mut credit = wall;
        if same_track
            && let (Some(prev), Some(now)) = (session.last_position, event.position)
            && now >= prev
        {
            // A forward seek advances the playhead faster than the clock, so
            // capping at `wall` keeps the skipped part out. A playhead slower than
            // the clock means a stall or an unreported pause.
            credit = credit.min(now - prev);
        }
        if !same_track
            && let (Some(duration), Some(prev)) = (session.track.duration, session.last_position)
        {
            // The old track cannot have played past its own end.
            credit = credit.min(duration.saturating_sub(prev));
        }
        // Without positions a long gap (e.g. sleep without a pause event) would
        // otherwise be credited in full.
        if let Some(duration) = session.track.duration {
            credit = credit.min(duration);
        }
        credit
    }

    fn is_repeat(&self, session: &Session, event: &PlaybackEvent) -> bool {
        let (Some(prev), Some(now)) = (session.last_position, event.position) else {
            return false;
        };
        if now >= prev || now > self.config.repeat_start_window {
            return false;
        }
        let near_end = session
            .track
            .duration
            .is_some_and(|d| prev + self.config.repeat_end_window >= d);
        session.scrobbled || near_end
    }

    fn scrobble_if_due(
        &self,
        session: &mut Session,
        source: &str,
        outputs: &mut Vec<EngineOutput>,
    ) {
        if session.scrobbled || !session.track.is_submittable() {
            return;
        }
        let Some(threshold) = scrobble_threshold(session.track.duration, &self.config) else {
            return;
        };
        if session.played >= threshold {
            session.scrobbled = true;
            outputs.push(EngineOutput::Scrobble {
                scrobble: Scrobble {
                    track: session.track.clone(),
                    started_at: session.started_at,
                    timestamp_accuracy: TimestampAccuracy::Observed,
                    origin: ScrobbleOrigin::LiveObservation,
                    source: source.to_owned(),
                    session_id: session.id.clone(),
                },
            });
        }
    }
}
