using Midgard.Core;

namespace Midgard;

static class Program
{
    [STAThread]
    static int Main()
    {
        ApplicationConfiguration.Initialize();
        var paths = Paths.Current();
        var log = new ShellLog(Path.Join(paths.Logs, "shell.log"));
        log.Write($"launch: Midgard {Application.ProductVersion}, resources {paths.Resources}, helpers {paths.Helpers}, "
                  + $"home {paths.Home}, logs {paths.Logs}, web view {paths.WebView}");
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
        Application.ThreadException += (_, e) =>
        {
            log.Write($"unexpected error: {e.Exception}");
            MessageBox.Show($"Midgard ran into an unexpected problem: {e.Exception.Message}\n\nThe details are in {log.File}.",
                            "Midgard", MessageBoxButtons.OK, MessageBoxIcon.Warning);
        };
        AppDomain.CurrentDomain.UnhandledException += (_, e) => log.Write($"fatal error: {e.ExceptionObject}");
        TaskScheduler.UnobservedTaskException += (_, e) =>
        {
            log.Write($"unobserved task error: {e.Exception}");
            e.SetObserved();
        };

        var wiring = new ShellWiring(paths, log);
        using var form = new MainForm(paths, log);
        var shell = new ShellModel(wiring.Steps(), form);
        // Before any window: a second launch brings the first forward and exits. The rest
        // (system check, folder check, sweep, working copy, services, sign-in) runs once the
        // window shows, so the starting screen can say how it goes.
        if (!shell.Claim()) return 0;
        form.Attach(shell);
        Application.Run(form);
        log.Write("exit");
        return 0;
    }
}
