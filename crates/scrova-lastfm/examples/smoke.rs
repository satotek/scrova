//! Manual smoke test against the real Last.fm API.
//!
//! Requires `LASTFM_API_KEY` and `LASTFM_SHARED_SECRET` in the environment.
//! The session key is kept in memory only and authentication runs every time;
//! nothing is written to disk.
//!
//! Writes performed by this example:
//! - always: one `track.updateNowPlaying` request for a test track
//! - only with `--scrobble`: one `track.scrobble` request with a timestamp
//!   five minutes in the past
//!
//! Without `--scrobble`, the only write is the now playing update. The session
//! key and shared secret are never printed.

use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrova_lastfm::{
    Credentials, LastfmClient, LastfmError, NowPlaying, RecentTracksQuery, ScrobbleOutcome,
    ScrobbleRequest, Session,
};

const TEST_ARTIST: &str = "Scrova";
const TEST_TRACK: &str = "Smoke Test";
const TEST_ALBUM: &str = "Scrova Diagnostics";
const TEST_DURATION: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        // Display, not Debug: Error values can contain request details, and
        // Debug output is not needed for a human-run smoke test.
        eprintln!("smoke: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let api_key = required_env("LASTFM_API_KEY")?;
    let shared_secret = required_env("LASTFM_SHARED_SECRET")?;
    let client = LastfmClient::new(Credentials::new(api_key, shared_secret));

    let token = client.get_token().await?;
    println!("Open this URL in a browser and approve Scrova:");
    println!("{}", client.authorize_url(&token));

    let session = wait_for_session(&client, &token).await?;
    println!("Signed in as {}.", session.name);

    let now_playing = test_now_playing();
    client
        .update_now_playing(&session.key, &now_playing)
        .await?;
    println!("Now playing updated.");

    if env::args().skip(1).any(|argument| argument == "--scrobble") {
        let timestamp = SystemTime::now()
            .checked_sub(Duration::from_secs(5 * 60))
            .unwrap_or_else(SystemTime::now);
        let request = ScrobbleRequest {
            artist: TEST_ARTIST.to_owned(),
            track: TEST_TRACK.to_owned(),
            timestamp,
            album: Some(TEST_ALBUM.to_owned()),
            album_artist: None,
            duration: Some(TEST_DURATION),
            track_number: None,
        };

        let outcomes = client.scrobble(&session.key, &[request]).await?;
        for outcome in outcomes {
            print_outcome(&outcome);
        }
    }

    let page = client
        .recent_tracks(
            &session.name,
            RecentTracksQuery {
                limit: Some(5),
                ..RecentTracksQuery::default()
            },
        )
        .await?;

    println!(
        "Recent tracks for {} (page {}, {} of {} pages):",
        page.user, page.page, page.per_page, page.total_pages
    );
    for track in page.tracks {
        let album = track
            .album
            .as_deref()
            .map(|album| format!(" [{album}]"))
            .unwrap_or_default();
        let loved = if track.loved { " [loved]" } else { "" };
        let when = if track.now_playing {
            "now playing".to_owned()
        } else {
            track
                .played_at
                .and_then(|played_at| played_at.duration_since(UNIX_EPOCH).ok())
                .map(|played_at| format!("uts={}", played_at.as_secs()))
                .unwrap_or_else(|| "unknown time".to_owned())
        };
        println!(
            "- {} - {}{}{} ({when})",
            track.artist, track.track, album, loved
        );
    }

    Ok(())
}

async fn wait_for_session(client: &LastfmClient, token: &str) -> Result<Session, Box<dyn Error>> {
    loop {
        wait_for_enter("Press Enter after approving the application: ")?;
        match client.get_session(token).await {
            Ok(session) => return Ok(session),
            Err(LastfmError::Api { code: 14, message }) => {
                println!("Authorization is still pending ({message}).");
                println!("Approve the application in the browser, then press Enter again.");
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn wait_for_enter(prompt: &str) -> io::Result<()> {
    print!("{prompt}");
    io::stdout().flush()?;

    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "stdin closed while waiting for authorization",
        ));
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{name} is not set in the environment"),
        )
    })
}

fn test_now_playing() -> NowPlaying {
    NowPlaying {
        artist: TEST_ARTIST.to_owned(),
        track: TEST_TRACK.to_owned(),
        album: Some(TEST_ALBUM.to_owned()),
        album_artist: None,
        duration: Some(TEST_DURATION),
        track_number: None,
    }
}

fn print_outcome(outcome: &ScrobbleOutcome) {
    match outcome {
        ScrobbleOutcome::Accepted { corrected } if corrected.is_empty() => {
            println!("Scrobble accepted.");
        }
        ScrobbleOutcome::Accepted { corrected } => {
            println!("Scrobble accepted with corrections:");
            if let Some(track) = &corrected.track {
                println!("  track: {track}");
            }
            if let Some(artist) = &corrected.artist {
                println!("  artist: {artist}");
            }
            if let Some(album) = &corrected.album {
                println!("  album: {album}");
            }
            if let Some(album_artist) = &corrected.album_artist {
                println!("  album artist: {album_artist}");
            }
        }
        ScrobbleOutcome::Ignored { code, message } => {
            println!("Scrobble ignored (code {code}): {message}");
        }
    }
}
