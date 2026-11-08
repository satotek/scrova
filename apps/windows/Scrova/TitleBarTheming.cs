using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;

namespace Scrova;

/// <summary>
/// Keeps a window's system title bar (and its caption buttons) in the same light/dark
/// theme as the XAML content, so a dark window never gets a white title bar.
/// </summary>
/// <remarks>
/// The title bar stays system-drawn; only its theme is chosen. The system then supplies
/// matching caption-button hover/pressed/inactive colors, so none are hard-coded here.
/// </remarks>
internal static class TitleBarTheming
{
    /// <summary>Applies the content's current theme now and again whenever it changes.</summary>
    public static void Attach(Window window)
    {
        if (window.Content is not FrameworkElement root)
        {
            return;
        }

        Apply(window, root.ActualTheme);
        root.ActualThemeChanged += (sender, _) => Apply(window, sender.ActualTheme);
    }

    private static void Apply(Window window, ElementTheme theme)
    {
        if (!AppWindowTitleBar.IsCustomizationSupported())
        {
            return;
        }

        window.AppWindow.TitleBar.PreferredTheme = theme == ElementTheme.Dark
            ? TitleBarTheme.Dark
            : TitleBarTheme.Light;
    }
}
