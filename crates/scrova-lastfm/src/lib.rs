//! Last.fm Scrobbling 2.0 client.
//!
//! The crate deliberately owns its request/response types instead of depending
//! on the playback engine. That keeps the API surface usable from the macOS and
//! iOS shells, and avoids coupling wire formats to the engine's internal model.
//!
//! All requests use JSON. Last.fm's JSON conversion is lossy in a few places:
//! repeated children may collapse into a single object, attributes live under
//! `@attr`, and text with attributes lives under `#text`. The private `raw`
//! module normalizes those shapes before they reach the public API.

mod client;
mod error;
mod raw;
mod types;

pub use client::{DEFAULT_BASE_URL, LastfmClient};
pub use error::{LastfmError, Result, RetryClass};
pub use types::{
    Corrections, Credentials, NowPlaying, RecentTrack, RecentTracksPage, RecentTracksQuery,
    ScrobbleOutcome, ScrobbleRequest, Session,
};
