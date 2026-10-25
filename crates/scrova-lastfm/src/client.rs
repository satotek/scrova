use std::time::{Duration, SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use reqwest::Client;

use crate::error::{LastfmError, Result};
use crate::raw::{
    ApiEnvelope, BoolLike, IgnoredMessageRaw, OneOrMany, RecentTrackRaw, RecentTracksRaw,
    ScrobbleRaw, StringOrNumber, TextLike, non_empty,
};
use crate::types::{
    Corrections, Credentials, NowPlaying, RecentTrack, RecentTracksPage, RecentTracksQuery,
    ScrobbleOutcome, ScrobbleRequest, Session,
};

/// Last.fm's public Web Services endpoint.
pub const DEFAULT_BASE_URL: &str = "https://ws.audioscrobbler.com/2.0/";

/// Last.fm returns at most this many scrobbles per `track.scrobble` call.
const MAX_SCROBBLES_PER_REQUEST: usize = 50;

/// Enough context for logs without putting an entire error page in a queue row.
const BODY_SNIPPET_CHARS: usize = 200;

/// Last.fm Scrobbling 2.0 client.
#[derive(Clone, Debug)]
pub struct LastfmClient {
    http: Client,
    credentials: Credentials,
    base_url: String,
}

impl LastfmClient {
    /// Create a client for the production Last.fm endpoint.
    #[must_use]
    pub fn new(credentials: Credentials) -> Self {
        Self::with_base_url(credentials, DEFAULT_BASE_URL)
    }

    /// Create a client for another endpoint, primarily for tests.
    ///
    /// `base_url` is the full API root, for example
    /// `http://127.0.0.1:8080/2.0/`. A trailing slash is optional.
    #[must_use]
    pub fn with_base_url(credentials: Credentials, base_url: impl AsRef<str>) -> Self {
        let http = Client::builder()
            // Last.fm asks clients to send an identifiable User-Agent.
            .user_agent(concat!("scrova-lastfm/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client configuration should be valid");

        Self {
            http,
            credentials,
            base_url: base_url.as_ref().trim_end_matches('/').to_owned(),
        }
    }

    /// Build the browser URL that asks the user to authorize a request token.
    #[must_use]
    pub fn authorize_url(&self, token: &str) -> String {
        format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            self.credentials.api_key, token
        )
    }

    /// Fetch an unauthorized request token (`auth.getToken`).
    pub async fn get_token(&self) -> Result<String> {
        let params = self.base_params("auth.getToken");
        let envelope = self.send(params, false).await?;

        envelope
            .token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| LastfmError::Decode("response did not contain a token".to_owned()))
    }

    /// Exchange an authorized request token for a session (`auth.getSession`).
    pub async fn get_session(&self, token: &str) -> Result<Session> {
        let mut params = self.base_params("auth.getSession");
        params.push(("token".to_owned(), token.to_owned()));

        let envelope = self.send(params, false).await?;
        let session = envelope
            .session
            .ok_or_else(|| LastfmError::Decode("response did not contain a session".to_owned()))?;

        if session.key.is_empty() {
            return Err(LastfmError::Decode(
                "response contained an empty session key".to_owned(),
            ));
        }

        Ok(Session {
            name: session.name,
            key: session.key,
            subscriber: session.subscriber.as_ref().is_some_and(BoolLike::is_true),
        })
    }

    /// Update the user's "now playing" track (`track.updateNowPlaying`).
    pub async fn update_now_playing(
        &self,
        session_key: &str,
        now_playing: &NowPlaying,
    ) -> Result<()> {
        let mut params = self.write_params("track.updateNowPlaying", session_key);
        params.push(("artist".to_owned(), now_playing.artist.clone()));
        params.push(("track".to_owned(), now_playing.track.clone()));
        push_optional(&mut params, "album", now_playing.album.as_ref());
        push_optional(
            &mut params,
            "albumArtist",
            now_playing.album_artist.as_ref(),
        );
        if let Some(track_number) = now_playing.track_number {
            params.push(("trackNumber".to_owned(), track_number.to_string()));
        }
        if let Some(duration) = now_playing.duration {
            params.push(("duration".to_owned(), duration.as_secs().to_string()));
        }

        let envelope = self.send(params, true).await?;
        envelope
            .nowplaying
            .map(|_| ())
            .ok_or_else(|| LastfmError::Decode("response did not contain nowplaying".to_owned()))
    }

    /// Submit one or more scrobbles (`track.scrobble`).
    ///
    /// The response is normalized to the same order and length as `scrobbles`.
    /// Last.fm includes each submitted timestamp in the response, so results are
    /// matched by timestamp rather than relying on response order.
    pub async fn scrobble(
        &self,
        session_key: &str,
        scrobbles: &[ScrobbleRequest],
    ) -> Result<Vec<ScrobbleOutcome>> {
        if scrobbles.len() > MAX_SCROBBLES_PER_REQUEST {
            return Err(LastfmError::InvalidInput(format!(
                "track.scrobble accepts at most {MAX_SCROBBLES_PER_REQUEST} scrobbles per request"
            )));
        }
        if scrobbles.is_empty() {
            return Ok(Vec::new());
        }

        // Validate every timestamp before making a request. A failure halfway
        // through would otherwise leave the caller unable to tell whether a
        // batch was partially submitted.
        let timestamps = scrobbles
            .iter()
            .map(|scrobble| unix_secs(scrobble.timestamp, "scrobble timestamp"))
            .collect::<Result<Vec<_>>>()?;

        let mut params = self.write_params("track.scrobble", session_key);
        for (index, scrobble) in scrobbles.iter().enumerate() {
            params.push((format!("artist[{index}]"), scrobble.artist.clone()));
            params.push((format!("track[{index}]"), scrobble.track.clone()));
            params.push((format!("timestamp[{index}]"), timestamps[index].to_string()));
            push_optional_indexed(&mut params, "album", index, scrobble.album.as_ref());
            push_optional_indexed(
                &mut params,
                "albumArtist",
                index,
                scrobble.album_artist.as_ref(),
            );
            if let Some(track_number) = scrobble.track_number {
                params.push((format!("trackNumber[{index}]"), track_number.to_string()));
            }
            if let Some(duration) = scrobble.duration {
                params.push((format!("duration[{index}]"), duration.as_secs().to_string()));
            }
        }

        let envelope = self.send(params, true).await?;
        let raw = envelope
            .scrobbles
            .ok_or_else(|| LastfmError::Decode("response did not contain scrobbles".to_owned()))?;
        let raws = raw.scrobble.map(OneOrMany::into_vec).unwrap_or_default();

        correlate(scrobbles, &timestamps, raws)
    }

    /// Fetch a page of a user's recent tracks (`user.getRecentTracks`).
    pub async fn recent_tracks(
        &self,
        user: &str,
        query: RecentTracksQuery,
    ) -> Result<RecentTracksPage> {
        let mut params = self.base_params("user.getRecentTracks");
        params.push(("user".to_owned(), user.to_owned()));
        if let Some(page) = query.page {
            params.push(("page".to_owned(), page.to_string()));
        }
        if let Some(limit) = query.limit {
            params.push(("limit".to_owned(), limit.to_string()));
        }
        if let Some(from) = query.from {
            params.push(("from".to_owned(), unix_secs(from, "from")?.to_string()));
        }
        if let Some(to) = query.to {
            params.push(("to".to_owned(), unix_secs(to, "to")?.to_string()));
        }

        let envelope = self.send(params, false).await?;
        let raw = envelope.recenttracks.ok_or_else(|| {
            LastfmError::Decode("response did not contain recenttracks".to_owned())
        })?;

        Ok(recent_tracks_page(raw, user))
    }

    fn base_params(&self, method: &str) -> Vec<(String, String)> {
        vec![
            ("api_key".to_owned(), self.credentials.api_key.clone()),
            ("method".to_owned(), method.to_owned()),
        ]
    }

    fn write_params(&self, method: &str, session_key: &str) -> Vec<(String, String)> {
        let mut params = self.base_params(method);
        params.push(("sk".to_owned(), session_key.to_owned()));
        params
    }

    async fn send(&self, params: Vec<(String, String)>, write: bool) -> Result<ApiEnvelope> {
        let params = self.signed_params(params);
        let endpoint = format!("{}/", self.base_url);

        let request = if write {
            self.http.post(&endpoint).form(&params)
        } else {
            self.http.get(self.endpoint_with_query(&params)?)
        };

        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;

        let envelope: ApiEnvelope = match serde_json::from_str(&body) {
            Ok(envelope) => envelope,
            Err(error) if status.is_success() => {
                return Err(LastfmError::Decode(format!(
                    "invalid JSON response (HTTP {status}): {error}"
                )));
            }
            Err(_) => return Err(http_error(status, &body)),
        };

        if let Some(code) = &envelope.error {
            let Some(code) = code.as_u32() else {
                return Err(if status.is_success() {
                    LastfmError::Decode("response contained an invalid error code".to_owned())
                } else {
                    http_error(status, &body)
                });
            };
            return Err(LastfmError::Api {
                code,
                message: envelope
                    .message
                    .unwrap_or_else(|| "unknown Last.fm error".to_owned()),
            });
        }

        // A structured body is not enough: a proxy may return valid JSON that
        // is not a Last.fm envelope. Only treat non-2xx as an API error above.
        if !status.is_success() {
            return Err(http_error(status, &body));
        }

        Ok(envelope)
    }

    fn signed_params(&self, params: Vec<(String, String)>) -> Vec<(String, String)> {
        let signature = signature(&params, &self.credentials.shared_secret);
        let mut signed = params;
        signed.push(("api_sig".to_owned(), signature));
        // format/callback must never be part of the signature; attaching the
        // response format only after signing keeps that invariant local.
        signed.push(("format".to_owned(), "json".to_owned()));
        signed
    }

    fn endpoint_with_query(&self, params: &[(String, String)]) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{}/", self.base_url))
            .map_err(|error| LastfmError::InvalidInput(format!("invalid base URL: {error}")))?;
        {
            let mut query = url.query_pairs_mut();
            for (name, value) in params {
                query.append_pair(name, value);
            }
        }
        Ok(url)
    }
}

fn push_optional(params: &mut Vec<(String, String)>, name: &str, value: Option<&String>) {
    if let Some(value) = value {
        params.push((name.to_owned(), value.clone()));
    }
}

fn push_optional_indexed(
    params: &mut Vec<(String, String)>,
    name: &str,
    index: usize,
    value: Option<&String>,
) {
    if let Some(value) = value {
        params.push((format!("{name}[{index}]"), value.clone()));
    }
}

/// Build the official `api_sig`.
///
/// Last.fm requires parameter names sorted by their UTF-8 bytes, values
/// concatenated without separators, the shared secret appended, and MD5 of the
/// result. `format`, `callback`, and `api_sig` are never signed.
fn signature(params: &[(String, String)], secret: &str) -> String {
    let mut signed: Vec<&(String, String)> = params
        .iter()
        .filter(|(name, _)| name != "format" && name != "callback" && name != "api_sig")
        .collect();
    signed.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));

    let mut canonical = String::new();
    for (name, value) in signed {
        canonical.push_str(name);
        canonical.push_str(value);
    }
    canonical.push_str(secret);

    let digest = Md5::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(32);
    for byte in digest {
        use std::fmt::Write as _;
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

fn unix_secs(value: SystemTime, field: &str) -> Result<u64> {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| LastfmError::InvalidInput(format!("{field} is before the UNIX epoch")))
}

fn http_error(status: reqwest::StatusCode, body: &str) -> LastfmError {
    LastfmError::Http {
        status: status.as_u16(),
        // Chars, not bytes: a UTF-8 boundary must not panic on an HTML error
        // page containing non-ASCII text.
        body_snippet: body.chars().take(BODY_SNIPPET_CHARS).collect(),
    }
}

fn correlate(
    inputs: &[ScrobbleRequest],
    timestamps: &[u64],
    raws: Vec<ScrobbleRaw>,
) -> Result<Vec<ScrobbleOutcome>> {
    if raws.len() != inputs.len() {
        return Err(LastfmError::Decode(format!(
            "expected {} scrobble results, got {}",
            inputs.len(),
            raws.len()
        )));
    }

    let mut used = vec![false; raws.len()];
    let mut outcomes = Vec::with_capacity(inputs.len());

    for (index, input) in inputs.iter().enumerate() {
        let matched = find_response_index(&raws, &used, index, timestamps[index], input)
            .ok_or_else(|| LastfmError::Decode("could not match a scrobble result".to_owned()))?;
        used[matched] = true;
        outcomes.push(to_outcome(&raws[matched]));
    }

    Ok(outcomes)
}

fn find_response_index(
    raws: &[ScrobbleRaw],
    used: &[bool],
    fallback_index: usize,
    timestamp: u64,
    input: &ScrobbleRequest,
) -> Option<usize> {
    // Prefer the full identity so duplicate timestamps within one batch still
    // map to the right entry.
    if let Some((index, _)) = raws.iter().enumerate().find(|(index, raw)| {
        !used[*index]
            && raw.timestamp_secs() == Some(timestamp)
            && raw.artist_text() == input.artist
            && raw.track_text() == input.track
    }) {
        return Some(index);
    }

    if let Some((index, _)) = raws
        .iter()
        .enumerate()
        .find(|(index, raw)| !used[*index] && raw.timestamp_secs() == Some(timestamp))
    {
        return Some(index);
    }

    if !used[fallback_index] {
        return Some(fallback_index);
    }

    (0..raws.len()).find(|index| !used[*index])
}

fn to_outcome(raw: &ScrobbleRaw) -> ScrobbleOutcome {
    let code = raw
        .ignored_message
        .as_ref()
        .map(IgnoredMessageRaw::code)
        .unwrap_or(0);

    if code == 0 {
        ScrobbleOutcome::Accepted {
            corrected: Corrections {
                track: corrected_text(&raw.track),
                artist: corrected_text(&raw.artist),
                album: corrected_text(&raw.album),
                album_artist: corrected_text(&raw.album_artist),
            },
        }
    } else {
        ScrobbleOutcome::Ignored {
            code,
            message: raw
                .ignored_message
                .as_ref()
                .map(IgnoredMessageRaw::message)
                .unwrap_or_default(),
        }
    }
}

fn corrected_text(value: &Option<TextLike>) -> Option<String> {
    value
        .as_ref()
        .filter(|value| value.is_corrected())
        .map(TextLike::text)
}

fn recent_tracks_page(raw: RecentTracksRaw, requested_user: &str) -> RecentTracksPage {
    let RecentTracksRaw { attr, track } = raw;
    let attr = attr.unwrap_or_default();

    RecentTracksPage {
        user: attr.user.unwrap_or_else(|| requested_user.to_owned()),
        page: attr
            .page
            .as_ref()
            .and_then(StringOrNumber::as_u32)
            .unwrap_or(1),
        per_page: attr
            .per_page
            .as_ref()
            .and_then(StringOrNumber::as_u32)
            .unwrap_or(0),
        total_pages: attr
            .total_pages
            .as_ref()
            .and_then(StringOrNumber::as_u32)
            .unwrap_or(0),
        tracks: track
            .map(OneOrMany::into_vec)
            .unwrap_or_default()
            .into_iter()
            .map(recent_track)
            .collect(),
    }
}

fn recent_track(raw: RecentTrackRaw) -> RecentTrack {
    let played_at = raw
        .date
        .and_then(|date| date.uts)
        .and_then(|uts| uts.as_u64())
        .map(|uts| UNIX_EPOCH + Duration::from_secs(uts));

    RecentTrack {
        artist: raw.artist.as_ref().map(TextLike::text).unwrap_or_default(),
        artist_mbid: raw.artist.as_ref().and_then(TextLike::mbid),
        track: raw.name.as_ref().map(TextLike::text).unwrap_or_default(),
        album: raw
            .album
            .as_ref()
            .map(TextLike::text)
            .and_then(|value| non_empty(Some(value))),
        album_mbid: raw.album.as_ref().and_then(TextLike::mbid),
        mbid: non_empty(raw.mbid),
        url: non_empty(raw.url),
        loved: raw.loved.as_ref().is_some_and(BoolLike::is_true),
        now_playing: raw.nowplaying.as_ref().is_some_and(BoolLike::is_true),
        played_at,
    }
}

#[cfg(test)]
mod tests {
    use super::signature;

    fn params(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn signature_matches_the_documented_example() {
        let actual = signature(
            &params(&[
                ("api_key", "xxxxxxxxxx"),
                ("method", "auth.getSession"),
                ("token", "yyyyyy"),
            ]),
            "ilovecher",
        );

        assert_eq!(actual, "b87d61da3cda91a8b6746c4aef55d6f8");
    }

    #[test]
    fn format_and_callback_are_not_signed() {
        let without = signature(
            &params(&[("api_key", "key"), ("method", "track.scrobble")]),
            "secret",
        );
        let with = signature(
            &params(&[
                ("format", "json"),
                ("callback", "cb"),
                ("api_key", "key"),
                ("method", "track.scrobble"),
            ]),
            "secret",
        );

        assert_eq!(without, with);
    }

    #[test]
    fn array_indices_are_sorted_as_ascii_not_numerically() {
        // artist[10] sorts before artist[1] because '0' < ']'.
        let actual = signature(
            &params(&[
                ("artist[2]", "A"),
                ("method", "track.scrobble"),
                ("artist[1]", "B"),
                ("api_key", "key"),
                ("artist[10]", "C"),
            ]),
            "secret",
        );

        assert_eq!(actual, "b0542b34eaa6751d4b94afa864f84bcc");
    }
}
