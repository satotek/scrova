use serde::Deserialize;

/// Last.fm JSON represents almost every scalar as a string, but error codes
/// and a few numbers arrive as numbers. Accept both rather than guessing.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum StringOrNumber {
    String(String),
    Number(serde_json::Number),
}

impl StringOrNumber {
    pub(crate) fn as_u64(&self) -> Option<u64> {
        match self {
            Self::String(value) => value.trim().parse().ok(),
            Self::Number(value) => value.as_u64(),
        }
    }

    pub(crate) fn as_u32(&self) -> Option<u32> {
        self.as_u64().and_then(|value| u32::try_from(value).ok())
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum BoolLike {
    Bool(bool),
    Number(serde_json::Number),
    String(String),
}

impl BoolLike {
    pub(crate) fn is_true(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Number(value) => value.as_u64().is_some_and(|value| value != 0),
            Self::String(value) => {
                let value = value.trim();
                value == "1"
                    || value.eq_ignore_ascii_case("true")
                    || value.eq_ignore_ascii_case("yes")
            }
        }
    }
}

/// A string that may carry XML attributes in its object form.
///
/// `<artist mbid="...">Name</artist>` becomes either `"Name"` or
/// `{"#text":"Name","mbid":"..."}` depending on the endpoint and whether
/// attributes are present.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum TextLike {
    Plain(String),
    WithAttrs(TextWithAttrs),
    Number(serde_json::Number),
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TextWithAttrs {
    #[serde(rename = "#text", default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) mbid: Option<String>,
    #[serde(default)]
    pub(crate) corrected: Option<BoolLike>,
}

impl TextLike {
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Plain(value) => value.clone(),
            Self::WithAttrs(value) => value.text.clone().unwrap_or_default(),
            Self::Number(value) => value.to_string(),
        }
    }

    pub(crate) fn mbid(&self) -> Option<String> {
        match self {
            Self::WithAttrs(value) => non_empty(value.mbid.clone()),
            Self::Plain(_) | Self::Number(_) => None,
        }
    }

    pub(crate) fn is_corrected(&self) -> bool {
        match self {
            Self::WithAttrs(value) => value.corrected.as_ref().is_some_and(BoolLike::is_true),
            Self::Plain(_) | Self::Number(_) => false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum IgnoredMessageRaw {
    Object(IgnoredMessageObject),
    String(String),
    Number(serde_json::Number),
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct IgnoredMessageObject {
    #[serde(default)]
    pub(crate) code: Option<StringOrNumber>,
    #[serde(rename = "#text", default)]
    pub(crate) text: Option<String>,
}

impl IgnoredMessageRaw {
    pub(crate) fn code(&self) -> u32 {
        match self {
            Self::Object(value) => value
                .code
                .as_ref()
                .and_then(StringOrNumber::as_u32)
                .unwrap_or(0),
            Self::Number(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            Self::String(_) => 0,
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Object(value) => value.text.clone().unwrap_or_default(),
            Self::String(value) => value.clone(),
            Self::Number(_) => String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum OneOrMany<T> {
    // `Many` must come first: serde can deserialize a JSON array into a struct
    // as a positional sequence, so `One(T)` would happily swallow an array of
    // scrobbles as a single struct and silently drop the rest.
    Many(Vec<T>),
    One(T),
}

impl<T> OneOrMany<T> {
    pub(crate) fn into_vec(self) -> Vec<T> {
        match self {
            Self::Many(values) => values,
            Self::One(value) => vec![value],
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ScrobbleRaw {
    #[serde(default)]
    pub(crate) track: Option<TextLike>,
    #[serde(default)]
    pub(crate) artist: Option<TextLike>,
    #[serde(default)]
    pub(crate) album: Option<TextLike>,
    // The scrobbling guide's XML example uses lowercase `albumartist`, while
    // the method page uses `albumArtist`. Accept both to survive the mismatch.
    #[serde(rename = "albumArtist", alias = "albumartist", default)]
    pub(crate) album_artist: Option<TextLike>,
    #[serde(default)]
    pub(crate) timestamp: Option<StringOrNumber>,
    #[serde(rename = "ignoredMessage", alias = "ignoredmessage", default)]
    pub(crate) ignored_message: Option<IgnoredMessageRaw>,
}

impl ScrobbleRaw {
    pub(crate) fn timestamp_secs(&self) -> Option<u64> {
        self.timestamp.as_ref().and_then(StringOrNumber::as_u64)
    }

    pub(crate) fn artist_text(&self) -> String {
        self.artist.as_ref().map(TextLike::text).unwrap_or_default()
    }

    pub(crate) fn track_text(&self) -> String {
        self.track.as_ref().map(TextLike::text).unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ScrobblesRaw {
    #[serde(default)]
    pub(crate) scrobble: Option<OneOrMany<ScrobbleRaw>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SessionRaw {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) subscriber: Option<BoolLike>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RecentTracksAttr {
    #[serde(default)]
    pub(crate) user: Option<String>,
    #[serde(default)]
    pub(crate) page: Option<StringOrNumber>,
    #[serde(rename = "perPage", default)]
    pub(crate) per_page: Option<StringOrNumber>,
    #[serde(rename = "totalPages", default)]
    pub(crate) total_pages: Option<StringOrNumber>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RecentTracksRaw {
    #[serde(rename = "@attr", default)]
    pub(crate) attr: Option<RecentTracksAttr>,
    #[serde(default)]
    pub(crate) track: Option<OneOrMany<RecentTrackRaw>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RecentTrackRaw {
    #[serde(default)]
    pub(crate) artist: Option<TextLike>,
    #[serde(default)]
    pub(crate) name: Option<TextLike>,
    #[serde(default)]
    pub(crate) album: Option<TextLike>,
    #[serde(default)]
    pub(crate) mbid: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) date: Option<DateRaw>,
    #[serde(default)]
    pub(crate) loved: Option<BoolLike>,
    #[serde(default)]
    pub(crate) nowplaying: Option<BoolLike>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DateRaw {
    #[serde(default)]
    pub(crate) uts: Option<StringOrNumber>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiEnvelope {
    #[serde(default)]
    pub(crate) error: Option<StringOrNumber>,
    #[serde(default)]
    pub(crate) message: Option<String>,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) session: Option<SessionRaw>,
    #[serde(default)]
    pub(crate) scrobbles: Option<ScrobblesRaw>,
    #[serde(default)]
    pub(crate) nowplaying: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) recenttracks: Option<RecentTracksRaw>,
}

pub(crate) fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}
