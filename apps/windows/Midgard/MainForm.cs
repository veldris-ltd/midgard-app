namespace Midgard;

/// <summary>The one window. Blank for now: WebView2 and the starting screen come later in W2.</summary>
sealed class MainForm : Form
{
    public MainForm()
    {
        Text = "Midgard";
        ClientSize = new Size(1280, 800);
        StartPosition = FormStartPosition.CenterScreen;
    }
}
