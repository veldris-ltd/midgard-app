using Midgard.Core;

namespace Midgard;

static class Program
{
    [STAThread]
    static int Main()
    {
        ApplicationConfiguration.Initialize();
        var paths = Paths.Current();
        // Before any window: a second launch brings the first forward and exits.
        if (!SingleInstance.Claim(paths.Home, out var instance)) return 0;
        using (instance)
        {
            if (SystemCheck.Problem(WindowsFacts.ForThisPc(paths.Home)) is { } problem)
            {
                MessageBox.Show(problem, "Midgard cannot run on this PC", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return 1;
            }
            var me = System.Security.Principal.WindowsIdentity.GetCurrent().User?.Value;
            if (FolderAccess.Problem(paths, WindowsAcl.Entries, me: me) is { } access)
            {
                MessageBox.Show(access, "Midgard's folder is not private", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return 1;
            }
            Application.Run(new MainForm());
            return 0;
        }
    }
}
