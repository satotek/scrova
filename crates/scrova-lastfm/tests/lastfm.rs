use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_lastfm::{
    Corrections, Credentials, LastfmClient, LastfmError, NowPlaying, RecentTracksQuery, RetryClass,
    ScrobbleOutcome, ScrobbleRequest, Session,
};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn credentials() -> Credentials {
    Credentials::new("test-key", "test-secret")
}

fn client(server: &MockServer) -> LastfmClient {
    LastfmClient::with_base_url(credentials(), server.uri())
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn json(template: ResponseTemplate, body: &str) -> ResponseTemplate {
    template.set_body_raw(body, "application/json")
}

#[tokio::test]
async fn get_token_uses_json_get_and_parses_the_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("method", "auth.getToken"))
        .and(query_param("api_key", "test-key"))
        .and(query_param("format", "json"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"token":"cf45fe5a3e3cebe168480a086d7fe481"}"##,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let token = client(&server).get_token().await.unwrap();

    assert_eq!(token, "cf45fe5a3e3cebe168480a086d7fe481");
}

#[tokio::test]
async fn get_session_parses_session_fields_and_subscriber() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("method", "auth.getSession"))
        .and(query_param("token", "request-token"))
        .and(query_param("format", "json"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"session":{"name":"MyLastFMUsername","key":"d580d57f32848f5dcf574d1ce18d78b2","subscriber":"1"}}"##,
        ))
        .mount(&server)
        .await;

    let session = client(&server).get_session("request-token").await.unwrap();

    assert_eq!(session.name, "MyLastFMUsername");
    assert_eq!(session.key, "d580d57f32848f5dcf574d1ce18d78b2");
    assert!(session.subscriber);
}

#[tokio::test]
async fn update_now_playing_posts_a_form_and_accepts_an_ok_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("method=track.updateNowPlaying"))
        .and(body_string_contains("artist=Test+Artist"))
        .and(body_string_contains("track=Test+Track"))
        .and(body_string_contains("album=Test+Album"))
        .and(body_string_contains("trackNumber=7"))
        .and(body_string_contains("duration=240"))
        .and(body_string_contains("sk=session-key"))
        .and(body_string_contains("api_sig="))
        .and(body_string_contains("format=json"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"nowplaying":{"track":{"corrected":"0","#text":"Test Track"},"artist":{"corrected":"0","#text":"Test Artist"},"album":{"corrected":"0","#text":""},"albumArtist":{"corrected":"0","#text":""},"ignoredMessage":{"code":"0","#text":""}}}"##,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let now_playing = NowPlaying {
        artist: "Test Artist".to_owned(),
        track: "Test Track".to_owned(),
        album: Some("Test Album".to_owned()),
        album_artist: None,
        duration: Some(Duration::from_secs(240)),
        track_number: Some(7),
    };

    client(&server)
        .update_now_playing("session-key", &now_playing)
        .await
        .unwrap();
}

#[tokio::test]
async fn api_errors_keep_code_and_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(json(
            ResponseTemplate::new(400),
            r##"{"error":9,"message":"Invalid session key - Please re-authenticate"}"##,
        ))
        .mount(&server)
        .await;

    let error = client(&server).get_token().await.unwrap_err();

    let LastfmError::Api { code, message } = &error else {
        panic!("expected an API error, got {error:?}");
    };
    assert_eq!(*code, 9);
    assert_eq!(message, "Invalid session key - Please re-authenticate");
    assert_eq!(error.retry_class(), RetryClass::Reauthenticate);
}

#[tokio::test]
async fn retry_classes_follow_the_documented_mapping() {
    let cases = [
        (4, RetryClass::Permanent),
        (8, RetryClass::Retry),
        (9, RetryClass::Reauthenticate),
        (11, RetryClass::Retry),
        (16, RetryClass::Retry),
        (26, RetryClass::Suspended),
        (29, RetryClass::RetryAfterBackoff),
    ];

    for (code, expected) in cases {
        let server = MockServer::start().await;
        let body = format!(r##"{{"error":{code},"message":"code {code}"}}"##);
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(json(ResponseTemplate::new(400), &body))
            .mount(&server)
            .await;

        let error = client(&server).get_token().await.unwrap_err();

        assert_eq!(error.retry_class(), expected, "error code {code}");
    }
}

#[tokio::test]
async fn malformed_json_on_a_2xx_is_retryable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("not-json", "text/plain"))
        .mount(&server)
        .await;

    let error = client(&server).get_token().await.unwrap_err();

    assert!(matches!(error, LastfmError::Decode(_)));
    // A 2xx with broken JSON usually means a truncated response, not a caller
    // bug, so keep the scrobble and retry.
    assert_eq!(error.retry_class(), RetryClass::Retry);
}

#[tokio::test]
async fn http_502_html_is_retryable_and_keeps_a_short_snippet() {
    let server = MockServer::start().await;
    let html = format!(
        "<!doctype html><html><body>{}</body></html>",
        "x".repeat(300)
    );
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(502).set_body_raw(html, "text/html"))
        .mount(&server)
        .await;

    let error = client(&server).get_token().await.unwrap_err();

    let LastfmError::Http {
        status,
        body_snippet,
    } = &error
    else {
        panic!("expected an HTTP error, got {error:?}");
    };
    assert_eq!(*status, 502);
    assert!(body_snippet.starts_with("<!doctype html>"));
    assert_eq!(body_snippet.chars().count(), 200);
    assert_eq!(error.retry_class(), RetryClass::Retry);
}

#[tokio::test]
async fn http_502_with_non_lfm_json_is_not_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(json(
            ResponseTemplate::new(502),
            r##"{"message":"bad gateway"}"##,
        ))
        .mount(&server)
        .await;

    let error = client(&server).get_token().await.unwrap_err();

    assert!(matches!(&error, LastfmError::Http { status: 502, .. }));
    assert_eq!(error.retry_class(), RetryClass::Retry);
}

#[tokio::test]
async fn an_empty_404_is_permanent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let error = client(&server).get_token().await.unwrap_err();

    let LastfmError::Http {
        status,
        body_snippet,
    } = &error
    else {
        panic!("expected an HTTP error, got {error:?}");
    };
    assert_eq!(*status, 404);
    assert!(body_snippet.is_empty());
    assert_eq!(error.retry_class(), RetryClass::Permanent);
}

#[tokio::test]
async fn an_empty_429_uses_retry_after_backoff() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let error = client(&server).get_token().await.unwrap_err();

    assert!(matches!(&error, LastfmError::Http { status: 429, .. }));
    assert_eq!(error.retry_class(), RetryClass::RetryAfterBackoff);
}

#[tokio::test]
async fn transport_failures_are_retryable() {
    let client = LastfmClient::with_base_url(credentials(), "http://127.0.0.1:1");

    let error = client.get_token().await.unwrap_err();

    assert!(matches!(&error, LastfmError::Transport(_)));
    assert_eq!(error.retry_class(), RetryClass::Retry);
}

#[tokio::test]
async fn an_invalid_base_url_is_a_permanent_input_error() {
    let client = LastfmClient::with_base_url(credentials(), "://invalid");

    let error = client.get_token().await.unwrap_err();

    assert!(matches!(error, LastfmError::InvalidInput(_)));
    assert_eq!(error.retry_class(), RetryClass::Permanent);
}

#[tokio::test]
async fn a_single_scrobble_object_is_accepted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("method=track.scrobble"))
        .and(body_string_contains("artist%5B0%5D=Test+Artist"))
        .and(body_string_contains("timestamp%5B0%5D=1287140447"))
        .and(body_string_contains("sk=session-key"))
        .and(body_string_contains("format=json"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"scrobbles":{"scrobble":{"track":{"corrected":"0","#text":"Test Track"},"artist":{"corrected":"0","#text":"Test Artist"},"album":{"corrected":"0","#text":""},"albumArtist":{"corrected":"0","#text":""},"timestamp":"1287140447","ignoredMessage":{"code":"0","#text":""}},"@attr":{"accepted":"1","ignored":"0"}}}"##,
        ))
        .mount(&server)
        .await;

    let scrobbles = [ScrobbleRequest::new(
        "Test Artist",
        "Test Track",
        at(1_287_140_447),
    )];

    let outcomes = client(&server)
        .scrobble("session-key", &scrobbles)
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![ScrobbleOutcome::Accepted {
            corrected: Corrections::default()
        }]
    );
}

#[tokio::test]
async fn scrobble_results_are_matched_by_timestamp_even_when_reordered() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("method=track.scrobble"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"scrobbles":{"scrobble":[{"track":{"corrected":"0","#text":"Track B"},"artist":{"corrected":"1","#text":"Björk"},"album":{"corrected":"0","#text":""},"albumArtist":{"corrected":"0","#text":""},"timestamp":"200","ignoredMessage":{"code":"0","#text":""}},{"track":{"corrected":"0","#text":"Track A"},"artist":{"corrected":"0","#text":"Artist A"},"album":{"corrected":"0","#text":""},"albumArtist":{"corrected":"0","#text":""},"timestamp":"100","ignoredMessage":{"code":"2","#text":"Track was ignored"}}],"@attr":{"accepted":"1","ignored":"1"}}}"##,
        ))
        .mount(&server)
        .await;

    let scrobbles = [
        ScrobbleRequest {
            artist: "Artist A".to_owned(),
            track: "Track A".to_owned(),
            timestamp: at(100),
            album: Some("Album A".to_owned()),
            album_artist: None,
            duration: Some(Duration::from_secs(200)),
            track_number: Some(1),
        },
        ScrobbleRequest::new("Artist B", "Track B", at(200)),
    ];

    let outcomes = client(&server)
        .scrobble("session-key", &scrobbles)
        .await
        .unwrap();

    assert_eq!(outcomes.len(), scrobbles.len());
    assert_eq!(
        outcomes[0],
        ScrobbleOutcome::Ignored {
            code: 2,
            message: "Track was ignored".to_owned()
        }
    );
    assert_eq!(
        outcomes[1],
        ScrobbleOutcome::Accepted {
            corrected: Corrections {
                artist: Some("Björk".to_owned()),
                ..Corrections::default()
            }
        }
    );
}

#[tokio::test]
async fn update_now_playing_propagates_a_session_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(json(
            ResponseTemplate::new(400),
            r##"{"error":9,"message":"Invalid session key"}"##,
        ))
        .mount(&server)
        .await;

    let error = client(&server)
        .update_now_playing("session-key", &NowPlaying::new("Artist", "Track"))
        .await
        .unwrap_err();

    assert!(matches!(&error, LastfmError::Api { code: 9, .. }));
    assert_eq!(error.retry_class(), RetryClass::Reauthenticate);
}

#[tokio::test]
async fn scrobble_temporary_errors_are_retryable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(json(
            ResponseTemplate::new(400),
            r##"{"error":16,"message":"The service is temporarily unavailable"}"##,
        ))
        .mount(&server)
        .await;

    let scrobbles = [ScrobbleRequest::new("Artist", "Track", at(1))];
    let error = client(&server)
        .scrobble("session-key", &scrobbles)
        .await
        .unwrap_err();

    assert!(matches!(&error, LastfmError::Api { code: 16, .. }));
    assert_eq!(error.retry_class(), RetryClass::Retry);
}

#[tokio::test]
async fn recent_tracks_input_errors_are_permanent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(json(
            ResponseTemplate::new(400),
            r##"{"error":6,"message":"Missing required parameter user"}"##,
        ))
        .mount(&server)
        .await;

    let error = client(&server)
        .recent_tracks("", RecentTracksQuery::default())
        .await
        .unwrap_err();

    assert!(matches!(&error, LastfmError::Api { code: 6, .. }));
    assert_eq!(error.retry_class(), RetryClass::Permanent);
}

#[tokio::test]
async fn more_than_fifty_scrobbles_is_rejected_before_any_request() {
    let client = LastfmClient::with_base_url(credentials(), "http://127.0.0.1:1");
    let scrobbles: Vec<_> = (0..51)
        .map(|index| ScrobbleRequest::new("Artist", format!("Track {index}"), at(index)))
        .collect();

    let error = client
        .scrobble("session-key", &scrobbles)
        .await
        .unwrap_err();

    assert!(matches!(error, LastfmError::InvalidInput(_)));
    assert_eq!(error.retry_class(), RetryClass::Permanent);
}

#[tokio::test]
async fn an_empty_batch_is_a_no_op() {
    let client = LastfmClient::with_base_url(credentials(), "http://127.0.0.1:1");

    let outcomes = client.scrobble("session-key", &[]).await.unwrap();

    assert!(outcomes.is_empty());
}

#[tokio::test]
async fn recent_tracks_parses_attributes_now_playing_and_dates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("method", "user.getRecentTracks"))
        .and(query_param("user", "rj"))
        .and(query_param("page", "2"))
        .and(query_param("limit", "2"))
        .and(query_param("from", "100"))
        .and(query_param("to", "200"))
        .and(query_param("format", "json"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"recenttracks":{"track":[{"artist":{"mbid":"artist-mbid","#text":"Aretha Franklin"},"name":"Sisters Are Doing It For Themselves","mbid":"","album":{"mbid":"album-mbid","#text":"Album"},"url":"https://www.last.fm/music/example","date":{"uts":"1213031819","#text":"9 Jun 2008, 17:16"},"loved":"1","nowplaying":"false"},{"artist":{"mbid":"","#text":"Now Artist"},"name":"Now Track","mbid":"","album":{"mbid":"","#text":""},"url":"","nowplaying":"true"}],"@attr":{"user":"RJ","page":"2","perPage":"2","totalPages":"5"}}}"##,
        ))
        .mount(&server)
        .await;

    let page = client(&server)
        .recent_tracks(
            "rj",
            RecentTracksQuery {
                page: Some(2),
                limit: Some(2),
                from: Some(at(100)),
                to: Some(at(200)),
            },
        )
        .await
        .unwrap();

    assert_eq!(page.user, "RJ");
    assert_eq!(page.page, 2);
    assert_eq!(page.per_page, 2);
    assert_eq!(page.total_pages, 5);
    assert_eq!(page.tracks.len(), 2);

    let first = &page.tracks[0];
    assert_eq!(first.artist, "Aretha Franklin");
    assert_eq!(first.artist_mbid.as_deref(), Some("artist-mbid"));
    assert_eq!(first.track, "Sisters Are Doing It For Themselves");
    assert_eq!(first.album.as_deref(), Some("Album"));
    assert_eq!(first.album_mbid.as_deref(), Some("album-mbid"));
    assert_eq!(first.played_at, Some(at(1_213_031_819)));
    assert!(first.loved);
    assert!(!first.now_playing);

    let second = &page.tracks[1];
    assert_eq!(second.artist, "Now Artist");
    assert_eq!(second.album, None);
    assert_eq!(second.played_at, None);
    assert!(second.now_playing);
}

#[tokio::test]
async fn recent_tracks_accepts_a_single_track_object() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("method", "user.getRecentTracks"))
        .respond_with(json(
            ResponseTemplate::new(200),
            r##"{"recenttracks":{"track":{"artist":"Solo Artist","name":"Solo Track","mbid":"","album":"","url":"","date":{"uts":"1"}},"@attr":{"user":"solo","page":"1","perPage":"1","totalPages":"1"}}}"##,
        ))
        .mount(&server)
        .await;

    let page = client(&server)
        .recent_tracks("solo", RecentTracksQuery::default())
        .await
        .unwrap();

    assert_eq!(page.tracks.len(), 1);
    assert_eq!(page.tracks[0].artist, "Solo Artist");
    assert_eq!(page.tracks[0].track, "Solo Track");
    assert_eq!(page.tracks[0].played_at, Some(at(1)));
}

#[test]
fn authorize_url_contains_the_key_and_token() {
    let client = LastfmClient::new(credentials());

    assert_eq!(
        client.authorize_url("request-token"),
        "https://www.last.fm/api/auth/?api_key=test-key&token=request-token"
    );
}

#[test]
fn secrets_and_session_keys_are_redacted_from_debug_output() {
    let credentials = Credentials::new("api-key", "super-secret");
    assert!(!format!("{credentials:?}").contains("super-secret"));

    let session = Session::new("user", "session-secret", false);
    assert!(!format!("{session:?}").contains("session-secret"));

    let client = LastfmClient::new(Credentials::new("api-key", "client-secret"));
    assert!(!format!("{client:?}").contains("client-secret"));
}
