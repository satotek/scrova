using System.Diagnostics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Scrova.Playback;
using Windows.ApplicationModel;
using Windows.Storage.Streams;
using Windows.System;

namespace Scrova;

public sealed partial class MainWindow : Window
{
    private readonly MediaSessionObserver _observer = new();
    private readonly string _emptyTitle;
    private readonly string _emptyArtist;

    // AppUserModelID → display name (null when there is nothing worth showing).
    private readonly Dictionary<string, string?> _appNames = new(StringComparer.OrdinalIgnoreCase);

    // Artwork currently shown (or being decoded), and a counter so a slow decode of an
    // older cover never replaces a newer one.
    private byte[]? _shownArtwork;
    private int _artworkVersion;

    private SettingsWindow? _settingsWindow;

    public MainWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(420, 640));
        TitleBarTheming.Attach(this);

        // Ctrl+, opens Settings from anywhere in the window (VK_OEM_COMMA has no VirtualKey name).
        SettingsButton.KeyboardAccelerators.Add(new KeyboardAccelerator
        {
            Key = (VirtualKey)188,
            Modifiers = VirtualKeyModifiers.Control,
        });

        _emptyTitle = TitleText.Text;
        _emptyArtist = ArtistText.Text;

        // The observer raises on background threads; hop to this window's UI thread.
        _observer.SnapshotChanged += (_, snapshot) => DispatcherQueue.TryEnqueue(() => Render(snapshot));
        Closed += (_, _) =>
        {
            _observer.Dispose();
            // Don't leave the app running behind an orphaned Settings window.
            _settingsWindow?.Close();
        };

        _ = StartObserverAsync();
    }

    private async Task StartObserverAsync()
    {
        try
        {
            await _observer.StartAsync();
        }
        catch (Exception ex)
        {
            // GSMTC unavailable (e.g. stripped-down Windows). Keep showing the empty state.
            Debug.WriteLine($"[Scrova.Gsmtc] could not start: {ex.GetType().Name}: {ex.Message}");
        }
    }

    // ---- Settings ----

    private void OnSettingsClick(object sender, RoutedEventArgs e) => OpenSettings();

    /// <summary>Opens the Settings window, or brings the open one to the front.</summary>
    private void OpenSettings()
    {
        if (_settingsWindow is null)
        {
            var window = new SettingsWindow();
            window.Closed += (_, _) =>
            {
                if (ReferenceEquals(_settingsWindow, window))
                {
                    _settingsWindow = null;
                }
            };
            _settingsWindow = window;
        }

        _settingsWindow.Activate();
    }

    // ---- Now Playing ----

    private void Render(MediaSessionSnapshot s)
    {
        if (!s.HasTrack)
        {
            TitleText.Text = _emptyTitle;
            ArtistText.Text = _emptyArtist;
            AlbumText.Visibility = Visibility.Collapsed;
            PlaybackDetails.Visibility = Visibility.Collapsed;
            PlaybackProgress.Value = 0;
            RenderArtwork(null);
            return;
        }

        TitleText.Text = s.Title;
        ArtistText.Text = s.Artist ?? "Unknown artist";
        AlbumText.Text = s.Album ?? "";
        AlbumText.Visibility = s.Album is null ? Visibility.Collapsed : Visibility.Visible;
        RenderArtwork(s.Artwork);

        var state = s.State switch
        {
            PlaybackState.Playing => "Playing",
            PlaybackState.Paused => "Paused",
            _ => "Stopped",
        };
        var app = FriendlyAppName(s.SourceAppId);
        StateText.Text = app is null ? state : $"{state} · {app}";

        TimeText.Text = (s.Position, s.Duration) switch
        {
            ({ } p, { } d) => $"{Format(p)} / {Format(d)}",
            ({ } p, null) => Format(p),
            _ => "",
        };
        PlaybackDetails.Visibility = Visibility.Visible;

        PlaybackProgress.Value = s is { Position: { } pos, Duration: { } dur } && dur > TimeSpan.Zero
            ? Math.Clamp(pos / dur * 100, 0, 100)
            : 0;
    }

    /// <summary>Shows the cover (decoded on this UI thread), or the note icon when there is none.</summary>
    private void RenderArtwork(byte[]? bytes)
    {
        if (MediaSessionSnapshot.SameArtwork(bytes, _shownArtwork))
        {
            return;
        }

        _shownArtwork = bytes;
        var version = ++_artworkVersion;

        if (bytes is null)
        {
            ShowArtwork(null);
            return;
        }

        _ = DecodeArtworkAsync(bytes, version);
    }

    private async Task DecodeArtworkAsync(byte[] bytes, int version)
    {
        try
        {
            using var stream = new InMemoryRandomAccessStream();
            using (var writer = new DataWriter(stream.GetOutputStreamAt(0)))
            {
                writer.WriteBytes(bytes);
                await writer.StoreAsync();
                writer.DetachStream();
            }

            stream.Seek(0);

            // The box is 240 epx; decoding at that size keeps large covers from holding full-size bitmaps.
            var bitmap = new BitmapImage
            {
                DecodePixelType = DecodePixelType.Logical,
                DecodePixelWidth = 240,
            };
            await bitmap.SetSourceAsync(stream);

            if (version == _artworkVersion)
            {
                ShowArtwork(bitmap);
            }
        }
        catch (Exception ex)
        {
            // Unknown or corrupt image format: keep the fallback icon.
            Debug.WriteLine($"[Scrova] artwork decode failed: {ex.GetType().Name}: {ex.Message}");
            if (version == _artworkVersion)
            {
                ShowArtwork(null);
            }
        }
    }

    private void ShowArtwork(BitmapImage? bitmap)
    {
        ArtworkImage.Source = bitmap;
        ArtworkImage.Visibility = bitmap is null ? Visibility.Collapsed : Visibility.Visible;
        ArtworkFallback.Visibility = bitmap is null ? Visibility.Visible : Visibility.Collapsed;
    }

    private static string Format(TimeSpan t) =>
        t.TotalHours >= 1 ? t.ToString(@"h\:mm\:ss") : t.ToString(@"m\:ss");

    // ---- Source app name ----

    /// <summary>
    /// Human-readable name of the app that owns the session, or null when none can be
    /// determined. Packaged apps resolve through their manifest ("Apple Music");
    /// otherwise the AUMID is trimmed ("Spotify.exe" → "Spotify"). A bare entry-point
    /// name such as "App" ("Vendor.Product_hash!App") says nothing, so it yields null.
    /// </summary>
    private string? FriendlyAppName(string? appId)
    {
        if (appId is null)
        {
            return null;
        }

        if (!_appNames.TryGetValue(appId, out var name))
        {
            name = ResolveAppName(appId);
            _appNames[appId] = name;
        }

        return name;
    }

    private static string? ResolveAppName(string appId)
    {
        try
        {
            var displayName = AppInfo.GetFromAppUserModelId(appId)?.DisplayInfo?.DisplayName;
            if (Meaningful(displayName) is { } resolved)
            {
                return resolved;
            }
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            // Not a packaged app (e.g. "Spotify.exe"), or the package is gone.
        }

        var name = appId[(appId.LastIndexOf('!') + 1)..];
        if (name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
        {
            name = name[..^4];
        }

        return Meaningful(name);
    }

    private static string? Meaningful(string? name)
    {
        name = name?.Trim();
        return string.IsNullOrEmpty(name) || name.Equals("App", StringComparison.OrdinalIgnoreCase) ? null : name;
    }
}
