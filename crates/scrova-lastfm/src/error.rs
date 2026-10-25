use thiserror::Error;

/// Result type used by every fallible operation in this crate.
pub type Result<T> = std::result::Result<T, LastfmError>;

/// Transport, protocol, and input errors surfaced by the client.
#[derive(Debug, Error)]
pub enum LastfmError {
    /// Last.fm returned a well-formed JSON error response.
    #[error("Last.fm API error {code}: {message}")]
    Api { code: u32, message: String },

    /// The HTTP request itself failed (DNS, connection, TLS, timeout, body read).
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// A non-2xx response that did not carry a Last.fm error JSON body.
    ///
    /// CDNs and proxies in front of Last.fm commonly return HTML or an empty
    /// body during an outage. Those responses must not be confused with a
    /// malformed request: they are worth retrying.
    #[error("unexpected HTTP response {status}: {body_snippet}")]
    Http { status: u16, body_snippet: String },

    /// A 2xx response was not the JSON shape this crate expects.
    #[error("decode error: {0}")]
    Decode(String),

    /// The caller supplied data that cannot be represented in an API request.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// How a caller should react to a failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetryClass {
    /// Retry with normal exponential backoff.
    Retry,
    /// Back off longer than usual; the API is rate limiting us.
    RetryAfterBackoff,
    /// The session key is no longer valid. Re-authenticate, then retry once.
    Reauthenticate,
    /// The request is wrong or the resource is unavailable. Do not retry.
    Permanent,
    /// The application's API key has been suspended. Stop and contact Last.fm.
    Suspended,
}

impl LastfmError {
    /// Classify an error using Last.fm's documented scrobbling guidance.
    ///
    /// The docs only promise retries for 11 and 16, but code 8 is documented as
    /// a backend failure and transport failures are inherently transient. Code
    /// 29 is kept separate so callers can apply a longer delay without dropping
    /// cached scrobbles. Non-2xx responses without a Last.fm error body come
    /// from the HTTP layer (often a CDN or proxy), not from the API contract.
    #[must_use]
    pub fn retry_class(&self) -> RetryClass {
        match self {
            Self::Transport(_) => RetryClass::Retry,
            // A 2xx response with broken JSON is more likely a truncated or
            // half-written response from Last.fm/CDN than a caller bug.
            // Re-sending is safer than dropping a scrobble; callers should
            // still cap attempts and surface the error if it persists.
            Self::Decode(_) => RetryClass::Retry,
            Self::InvalidInput(_) => RetryClass::Permanent,
            Self::Http { status, .. } => match status {
                429 => RetryClass::RetryAfterBackoff,
                // 408 is a timeout on the HTTP layer; 5xx is a server outage.
                408 | 500..=599 => RetryClass::Retry,
                _ => RetryClass::Permanent,
            },
            Self::Api { code, .. } => match code {
                8 | 11 | 16 => RetryClass::Retry,
                29 => RetryClass::RetryAfterBackoff,
                9 => RetryClass::Reauthenticate,
                26 => RetryClass::Suspended,
                _ => RetryClass::Permanent,
            },
        }
    }
}
