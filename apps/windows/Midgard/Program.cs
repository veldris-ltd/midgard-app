using Midgard.Core;

namespace Midgard;

static class Program
{
    [STAThread]
    static int Main()
    {
        ApplicationConfiguration.Initialize();
        var paths = Paths.Current();
        if (SystemCheck.Problem(WindowsFacts.ForThisPc(paths.Home)) is { } problem)
        {
            MessageBox.Show(problem, "Midgard cannot run on this PC", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return 1;
        }
        Application.Run(new MainForm());
        return 0;
    }
}
