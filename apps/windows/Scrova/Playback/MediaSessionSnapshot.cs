namespace Scrova.Playback;

/// <summary>Playback state reduced to what Scrova cares about.</summary>
public enum PlaybackState
{
    Stopped,
    Paused,
    Playing,
}

/// <summary>
/// Immutable view of the media session Windows currently considers "current"
/// (Global System Media Transport Controls). Record equality is used to drop
/// duplicate updates; <see cref="Artwork"/> is compared by content, so re-reading the
/// same cover into a new array does not count as a change.
/// </summary>
/// <param name="HasSession">Whether any media session exists at all.</param>
/// <param name="Title">Track title, or null when the session reports none.</param>
/// <param name="Artist">Track artist (falls back to the album artist).</param>
/// <param name="Album">Album title.</param>
/// <param name="SourceAppId">AppUserModelID of the app that owns the session, e.g. <c>Spotify.exe</c>.</param>
/// <param name="State">Playing / paused / stopped.</param>
/// <param name="Position">Position from the start of the track, when the app reports a timeline.</param>
/// <param name="Duration">Track length, when the app reports a timeline.</param>
/// <param name="Artwork">Encoded cover image (PNG, JPEG, ...) from the session thumbnail, or null.</param>
public sealed record MediaSessionSnapshot(
    bool HasSession,
    string? Title,
    string? Artist,
    string? Album,
    string? SourceAppId,
    PlaybackState State,
    TimeSpan? Position,
    TimeSpan? Duration,
    byte[]? Artwork)
{
    public static MediaSessionSnapshot Empty { get; } =
        new(false, null, null, null, null, PlaybackState.Stopped, null, null, null);

    /// <summary>A session exists and reports a non-empty title. Anything else is "no track".</summary>
    public bool HasTrack => HasSession && !string.IsNullOrWhiteSpace(Title);

    public bool Equals(MediaSessionSnapshot? other) =>
        other is not null
        && HasSession == other.HasSession
        && Title == other.Title
        && Artist == other.Artist
        && Album == other.Album
        && SourceAppId == other.SourceAppId
        && State == other.State
        && Position == other.Position
        && Duration == other.Duration
        && SameArtwork(Artwork, other.Artwork);

    public override int GetHashCode() =>
        HashCode.Combine(HasSession, Title, Artist, Album, SourceAppId, State, HashCode.Combine(Position, Duration), Artwork?.Length);

    /// <summary>
    /// Byte-wise comparison. The observer reuses the same array across timeline ticks,
    /// so the reference check settles the common case without touching the bytes.
    /// </summary>
    public static bool SameArtwork(byte[]? a, byte[]? b) =>
        ReferenceEquals(a, b) || (a is not null && b is not null && a.AsSpan().SequenceEqual(b));
}
