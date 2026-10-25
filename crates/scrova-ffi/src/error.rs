use scrova_lastfm::LastfmError;
use scrova_storage::StoreError;

/// Errors as the native UI needs to tell them apart, not as the crates below
/// happen to produce them.
///
/// Fields are named `detail`, not `message`: Kotlin exceptions already have a
/// `message` property, and UniFFI's generated class would not compile.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ScrovaError {
    #[error("not signed in to Last.fm")]
    NotSignedIn,
    /// The user has not approved the token in the browser yet.
    #[error("Last.fm authorization is still pending")]
    AuthorizationPending,
    /// The session key was revoked or expired; the user must sign in again.
    #[error("Last.fm session is no longer valid")]
    SessionInvalid,
    /// Affects every user of this build; nothing the user can do about it.
    #[error("the Last.fm API key has been suspended")]
    ApiKeySuspended,
    #[error("network error: {detail}")]
    Network { detail: String },
    #[error("Last.fm error {code}: {detail}")]
    Lastfm { code: u32, detail: String },
    #[error("storage error: {detail}")]
    Storage { detail: String },
    #[error("internal error: {detail}")]
    Internal { detail: String },
}

impl From<LastfmError> for ScrovaError {
    fn from(error: LastfmError) -> Self {
        match error {
            LastfmError::Api { code: 9, .. } => Self::SessionInvalid,
            LastfmError::Api { code: 26, .. } => Self::ApiKeySuspended,
            LastfmError::Api { code, message } => Self::Lastfm {
                code,
                detail: message,
            },
            LastfmError::Transport(error) => Self::Network {
                detail: error.to_string(),
            },
            LastfmError::Http {
                status,
                body_snippet,
            } => Self::Network {
                detail: format!("HTTP {status}: {body_snippet}"),
            },
            other @ (LastfmError::Decode(_) | LastfmError::InvalidInput(_)) => Self::Internal {
                detail: other.to_string(),
            },
        }
    }
}

impl From<StoreError> for ScrovaError {
    fn from(error: StoreError) -> Self {
        Self::Storage {
            detail: error.to_string(),
        }
    }
}
