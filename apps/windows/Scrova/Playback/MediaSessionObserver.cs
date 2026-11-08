using System.Diagnostics;
using Windows.Media.Control;
using Windows.Storage.Streams;

namespace Scrova.Playback;

/// <summary>
/// Follows the current GSMTC (Global System Media Transport Controls) session and
/// publishes a <see cref="MediaSessionSnapshot"/> whenever it changes.
/// </summary>
/// <remarks>
/// <para>
/// Sessions from every app that registers with the system media controls are accepted
/// (Spotify, browsers, Media Player, ...); nothing is filtered by app.
/// </para>
/// <para>
/// GSMTC raises its events on arbitrary threads and a session can disappear between two
/// calls. All reads go through one serialized refresh loop, every WinRT call on a session
/// is guarded, and a failure only resets the snapshot; the session manager stays alive.
/// </para>
/// <para>
/// <see cref="SnapshotChanged"/> is raised on a background thread, in order. UI code must
/// marshal to its own DispatcherQueue.
/// </para>
/// </remarks>
public sealed class MediaSessionObserver : IDisposable
{
    private static readonly TimeSpan TimelineRefreshInterval = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan WinRtCallTimeout = TimeSpan.FromSeconds(5);
    // Covers are usually tens to hundreds of KB; anything far larger is not worth decoding.
    private const ulong MaxArtworkBytes = 8 * 1024 * 1024;

    [Flags]
    private enum Dirty
    {
        None = 0,
        Session = 1,
        Media = 2,
        Timeline = 4,
        All = Session | Media | Timeline,
    }

    // Serializes refreshes (async). Held while reading from the session.
    private readonly SemaphoreSlim _refreshGate = new(1, 1);
    // Guards subscription state against Dispose (sync, never held across an await).
    private readonly object _sync = new();
    private readonly Timer _timelineTimer;

    private GlobalSystemMediaTransportControlsSessionManager? _manager;
    private GlobalSystemMediaTransportControlsSession? _session;
    private int _dirty;
    private bool _timelineTimerRunning;
    private bool _disposed;

    // Last metadata read from _session; re-read only when MediaPropertiesChanged fires.
    // Its artwork array is reused by every timeline tick, so those snapshots compare by reference.
    private MediaInfo _media;

    private readonly record struct MediaInfo(string? Title, string? Artist, string? Album, byte[]? Artwork);

    public MediaSessionObserver()
    {
        _timelineTimer = new Timer(_ => Request(Dirty.Timeline), null, Timeout.Infinite, Timeout.Infinite);
    }

    /// <summary>Raised (off the UI thread) when the snapshot changes.</summary>
    public event EventHandler<MediaSessionSnapshot>? SnapshotChanged;

    public MediaSessionSnapshot Current { get; private set; } = MediaSessionSnapshot.Empty;

    /// <summary>Requests the session manager and starts following the current session.</summary>
    public async Task StartAsync()
    {
        var manager = await GlobalSystemMediaTransportControlsSessionManager.RequestAsync();

        lock (_sync)
        {
            if (_disposed || _manager is not null)
            {
                return;
            }

            _manager = manager;
            _manager.CurrentSessionChanged += OnCurrentSessionChanged;
            _manager.SessionsChanged += OnSessionsChanged;
        }

        Log("session manager ready");
        Request(Dirty.All);
    }

    public void Dispose()
    {
        lock (_sync)
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
            _timelineTimer.Dispose();
            DetachSession();

            if (_manager is not null)
            {
                _manager.CurrentSessionChanged -= OnCurrentSessionChanged;
                _manager.SessionsChanged -= OnSessionsChanged;
                _manager = null;
            }
        }

        SnapshotChanged = null;
    }

    // ---- Event handlers (any thread) ----

    private void OnCurrentSessionChanged(GlobalSystemMediaTransportControlsSessionManager sender, CurrentSessionChangedEventArgs args) =>
        Request(Dirty.Session);

    private void OnSessionsChanged(GlobalSystemMediaTransportControlsSessionManager sender, SessionsChangedEventArgs args) =>
        Request(Dirty.Session);

    private void OnMediaPropertiesChanged(GlobalSystemMediaTransportControlsSession sender, MediaPropertiesChangedEventArgs args) =>
        Request(Dirty.Media);

    private void OnTimelinePropertiesChanged(GlobalSystemMediaTransportControlsSession sender, TimelinePropertiesChangedEventArgs args) =>
        Request(Dirty.Timeline);

    private void OnPlaybackInfoChanged(GlobalSystemMediaTransportControlsSession sender, PlaybackInfoChangedEventArgs args) =>
        Request(Dirty.Timeline);

    // ---- Refresh loop ----

    /// <summary>
    /// Marks work as pending and starts a refresh unless one is already queued. Bursts of
    /// events collapse into one refresh that sees all the accumulated flags.
    /// </summary>
    private void Request(Dirty what)
    {
        if (Interlocked.Or(ref _dirty, (int)what) == 0)
        {
            _ = Task.Run(RunRefreshAsync);
        }
    }

    private async Task RunRefreshAsync()
    {
        await _refreshGate.WaitAsync().ConfigureAwait(false);
        try
        {
            var dirty = (Dirty)Interlocked.Exchange(ref _dirty, 0);
            if (dirty != Dirty.None && !_disposed)
            {
                await RefreshAsync(dirty).ConfigureAwait(false);
            }
        }
        catch (Exception ex)
        {
            // Last line of defence: never let a refresh take the observer down.
            Log($"refresh failed: {ex.GetType().Name}: {ex.Message}");
        }
        finally
        {
            _refreshGate.Release();
        }
    }

    private async Task RefreshAsync(Dirty dirty)
    {
        if (dirty.HasFlag(Dirty.Session) && SwitchSessionIfNeeded())
        {
            dirty |= Dirty.Media | Dirty.Timeline;
        }

        var session = _session;
        if (session is null)
        {
            Publish(MediaSessionSnapshot.Empty);
            return;
        }

        try
        {
            if (dirty.HasFlag(Dirty.Media))
            {
                _media = await ReadMediaAsync(session, _media.Artwork).ConfigureAwait(false);
            }

            Publish(BuildSnapshot(session, _media));
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            // Typically the session closed between the change event and our read
            // (COMException / ObjectDisposedException / timeout). SessionsChanged or
            // CurrentSessionChanged will follow, so just report no track for now.
            Log($"session read failed: {ex.GetType().Name}: {ex.Message}");
            _media = default;
            Publish(MediaSessionSnapshot.Empty with { HasSession = true, SourceAppId = TryGetAppId(session) });
        }
    }

    /// <summary>Attaches to the session Windows considers current. Returns true when it changed.</summary>
    private bool SwitchSessionIfNeeded()
    {
        var target = ResolveTargetSession();

        lock (_sync)
        {
            if (_disposed || ReferenceEquals(target, _session))
            {
                return false;
            }

            DetachSession();
            _media = default;

            if (target is not null)
            {
                try
                {
                    target.MediaPropertiesChanged += OnMediaPropertiesChanged;
                    target.TimelinePropertiesChanged += OnTimelinePropertiesChanged;
                    target.PlaybackInfoChanged += OnPlaybackInfoChanged;
                    _session = target;
                }
                catch (Exception ex) when (ex is not OutOfMemoryException)
                {
                    Log($"subscribing to session failed: {ex.GetType().Name}: {ex.Message}");
                    _session = target;
                    DetachSession();
                }
            }
        }

        Log(_session is null ? "no current session" : $"following session {TryGetAppId(_session)}");
        return true;
    }

    /// <summary>
    /// The current session, or (when Windows reports none while sessions still exist)
    /// the first playing session, else the first session.
    /// </summary>
    private GlobalSystemMediaTransportControlsSession? ResolveTargetSession()
    {
        var manager = _manager;
        if (manager is null)
        {
            return null;
        }

        try
        {
            var current = manager.GetCurrentSession();
            if (current is not null)
            {
                return current;
            }

            GlobalSystemMediaTransportControlsSession? fallback = null;
            foreach (var session in manager.GetSessions())
            {
                fallback ??= session;
                if (TryGetStatus(session) == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing)
                {
                    return session;
                }
            }

            return fallback;
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            Log($"listing sessions failed: {ex.GetType().Name}: {ex.Message}");
            return null;
        }
    }

    /// <summary>Caller holds <see cref="_sync"/>.</summary>
    private void DetachSession()
    {
        var session = _session;
        _session = null;
        if (session is null)
        {
            return;
        }

        try
        {
            session.MediaPropertiesChanged -= OnMediaPropertiesChanged;
            session.TimelinePropertiesChanged -= OnTimelinePropertiesChanged;
            session.PlaybackInfoChanged -= OnPlaybackInfoChanged;
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            // The session's process may already be gone; nothing left to unsubscribe from.
            Log($"unsubscribing from session failed: {ex.GetType().Name}: {ex.Message}");
        }
    }

    // ---- Reading a session ----

    private static async Task<MediaInfo> ReadMediaAsync(
        GlobalSystemMediaTransportControlsSession session,
        byte[]? previousArtwork)
    {
        // Some apps never complete this call; don't let one wedge the refresh loop.
        var props = await session.TryGetMediaPropertiesAsync().AsTask().WaitAsync(WinRtCallTimeout).ConfigureAwait(false);
        if (props is null)
        {
            return default;
        }

        var title = NullIfBlank(props.Title);
        var artwork = title is null ? null : await TryReadArtworkAsync(props.Thumbnail).ConfigureAwait(false);
        if (MediaSessionSnapshot.SameArtwork(artwork, previousArtwork))
        {
            // Same cover re-read (e.g. a metadata-only change): keep the old array so the
            // UI does not decode it again and later comparisons stay reference-cheap.
            artwork = previousArtwork;
        }

        return new MediaInfo(
            title,
            NullIfBlank(props.Artist) ?? NullIfBlank(props.AlbumArtist),
            NullIfBlank(props.AlbumTitle),
            artwork);
    }

    /// <summary>
    /// Copies the session thumbnail into memory. A missing, empty, oversized or unreadable
    /// thumbnail yields null; artwork never fails the metadata read.
    /// </summary>
    private static async Task<byte[]?> TryReadArtworkAsync(IRandomAccessStreamReference? thumbnail)
    {
        if (thumbnail is null)
        {
            return null;
        }

        try
        {
            using var stream = await thumbnail.OpenReadAsync().AsTask().WaitAsync(WinRtCallTimeout).ConfigureAwait(false);
            var size = stream.Size;
            if (size == 0 || size > MaxArtworkBytes)
            {
                Log($"ignoring artwork of {size} bytes");
                return null;
            }

            var buffer = new Windows.Storage.Streams.Buffer((uint)size);
            var read = await stream.ReadAsync(buffer, (uint)size, InputStreamOptions.None)
                .AsTask().WaitAsync(WinRtCallTimeout).ConfigureAwait(false);
            if (read.Length == 0)
            {
                return null;
            }

            var bytes = new byte[read.Length];
            using (var reader = DataReader.FromBuffer(read))
            {
                reader.ReadBytes(bytes);
            }

            return bytes;
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            Log($"artwork read failed: {ex.GetType().Name}: {ex.Message}");
            return null;
        }
    }

    private static MediaSessionSnapshot BuildSnapshot(
        GlobalSystemMediaTransportControlsSession session,
        MediaInfo media)
    {
        var playback = session.GetPlaybackInfo();
        var state = playback?.PlaybackStatus switch
        {
            GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing => PlaybackState.Playing,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus.Paused => PlaybackState.Paused,
            _ => PlaybackState.Stopped,
        };

        var (position, duration) = ReadTimeline(session.GetTimelineProperties(), state, playback?.PlaybackRate);

        if (media.Title is null)
        {
            // No metadata means no track, whatever the timeline says.
            position = null;
            duration = null;
        }

        return new MediaSessionSnapshot(
            HasSession: true,
            media.Title,
            media.Artist,
            media.Album,
            TryGetAppId(session),
            state,
            position,
            duration,
            media.Title is null ? null : media.Artwork);
    }

    private static (TimeSpan? Position, TimeSpan? Duration) ReadTimeline(
        GlobalSystemMediaTransportControlsSessionTimelineProperties? timeline,
        PlaybackState state,
        double? playbackRate)
    {
        if (timeline is null)
        {
            return (null, null);
        }

        TimeSpan? duration = timeline.EndTime - timeline.StartTime;
        if (duration <= TimeSpan.Zero)
        {
            duration = null;
        }

        var position = timeline.Position - timeline.StartTime;

        // Many apps (Spotify among them) only push the timeline on seek / state change.
        // While playing, advance from the moment the app last reported it.
        var updated = timeline.LastUpdatedTime;
        if (state == PlaybackState.Playing && updated.Year >= 2000)
        {
            var elapsed = DateTimeOffset.Now - updated;
            if (elapsed > TimeSpan.Zero)
            {
                var rate = playbackRate is > 0 ? playbackRate.Value : 1.0;
                position += elapsed * rate;
            }
        }

        if (position < TimeSpan.Zero)
        {
            position = TimeSpan.Zero;
        }

        if (duration is { } d && position > d)
        {
            position = d;
        }

        // Round to whole seconds so the 1 s refresh doesn't publish sub-second jitter
        // and identical snapshots compare equal.
        position = TimeSpan.FromSeconds(Math.Floor(position.TotalSeconds));
        return (position, duration);
    }

    private static GlobalSystemMediaTransportControlsSessionPlaybackStatus? TryGetStatus(
        GlobalSystemMediaTransportControlsSession session)
    {
        try
        {
            return session.GetPlaybackInfo()?.PlaybackStatus;
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            return null;
        }
    }

    private static string? TryGetAppId(GlobalSystemMediaTransportControlsSession session)
    {
        try
        {
            return NullIfBlank(session.SourceAppUserModelId);
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            return null;
        }
    }

    private static string? NullIfBlank(string? s) => string.IsNullOrWhiteSpace(s) ? null : s.Trim();

    // ---- Publishing ----

    private void Publish(MediaSessionSnapshot snapshot)
    {
        UpdateTimelineTimer(snapshot.HasTrack && snapshot.State == PlaybackState.Playing);

        if (_disposed || snapshot == Current)
        {
            return;
        }

        Current = snapshot;
        SnapshotChanged?.Invoke(this, snapshot);
    }

    private void UpdateTimelineTimer(bool run)
    {
        lock (_sync)
        {
            if (_disposed || run == _timelineTimerRunning)
            {
                return;
            }

            _timelineTimerRunning = run;
            if (run)
            {
                _timelineTimer.Change(TimelineRefreshInterval, TimelineRefreshInterval);
            }
            else
            {
                _timelineTimer.Change(Timeout.Infinite, Timeout.Infinite);
            }
        }
    }

    private static void Log(string message) => Debug.WriteLine($"[Scrova.Gsmtc] {message}");
}
