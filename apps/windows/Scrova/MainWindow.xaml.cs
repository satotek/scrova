using Microsoft.UI.Xaml;

namespace Scrova;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(420, 600));
    }
}
