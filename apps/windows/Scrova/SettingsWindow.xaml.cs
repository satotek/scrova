using Microsoft.UI.Xaml;

namespace Scrova;

/// <summary>Placeholder settings window. Opened from the main window's footer.</summary>
public sealed partial class SettingsWindow : Window
{
    public SettingsWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(440, 400));
        TitleBarTheming.Attach(this);
    }

    private void OnCloseClick(object sender, RoutedEventArgs e) => Close();
}
