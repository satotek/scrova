using System.Diagnostics;
using Microsoft.UI.Xaml;
using Scrova.Playback;

namespace Scrova;

public sealed partial class MainWindow : Window
{
    private readonly MediaSessionObserver _observer = new();
    private readonly string _emptyTitle;
    private readonly string _emptyArtist;

    public MainWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(420, 600));

        _emptyTitle = TitleText.Text;
        _emptyArtist = ArtistText.Text;

        // The observer raises on background threads; hop to this window's UI thread.
        _observer.SnapshotChanged += (_, snapshot) => DispatcherQueue.TryEnqueue(() => Render(snapshot));
        Closed += (_, _) => _observer.Dispose();

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

    private void Render(MediaSessionSnapshot s)
    {
        if (!s.HasTrack)
        {
            TitleText.Text = _emptyTitle;
            ArtistText.Text = _emptyArtist;
            AlbumText.Visibility = Visibility.Collapsed;
            PlaybackDetails.Visibility = Visibility.Collapsed;
            PlaybackProgress.Value = 0;
            return;
        }

        TitleText.Text = s.Title;
        ArtistText.Text = s.Artist ?? "Unknown artist";
        AlbumText.Text = s.Album ?? "";
        AlbumText.Visibility = s.Album is null ? Visibility.Collapsed : Visibility.Visible;

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

    private static string Format(TimeSpan t) =>
        t.TotalHours >= 1 ? t.ToString(@"h\:mm\:ss") : t.ToString(@"m\:ss");

    /// <summary>"Spotify.exe" → "Spotify", "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" → "Microsoft.ZuneMusic".</summary>
    private static string? FriendlyAppName(string? appId)
    {
        if (appId is null)
        {
            return null;
        }

        var name = appId[(appId.LastIndexOf('!') + 1)..];
        if (name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
        {
            name = name[..^4];
        }

        return name.Length == 0 ? appId : name;
    }
}
