using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.WinForms;
using Midgard.Core;

namespace Midgard;

/// <summary>Design-system colours used by the native screens (StartingView.swift's Palette).</summary>
static class Palette
{
    public static readonly Color Page = Color.FromArgb(0x0a, 0x0b, 0x0d);
    public static readonly Color Card = Color.FromArgb(0x16, 0x18, 0x1c);
    public static readonly Color Border = Color.FromArgb(0x2a, 0x2d, 0x34);
    public static readonly Color Text = Color.FromArgb(0xe9, 0xeb, 0xee);
    public static readonly Color Muted = Color.FromArgb(0x91, 0x99, 0xa3);
    public static readonly Color Secure = Color.FromArgb(0x21, 0xc0, 0x7a);
    public static readonly Color Warn = Color.FromArgb(0xf0, 0xa8, 0x24);
    public static readonly Color Danger = Color.FromArgb(0xf2, 0x6b, 0x70);
    public static readonly Color Accent = Color.FromArgb(0xca, 0x1d, 0x25);
}

/// <summary>
/// The one window: a toolbar (Home, Reset demo, Quit), a note strip, the starting screen
/// (each service's status, or what went wrong) and the WebView2 control with the offline
/// rules. It is <see cref="IShellView"/> for <see cref="ShellModel"/>; everything it shows
/// comes from <see cref="ShellModel.State"/>. Closing it quits Midgard and stops every service.
/// </summary>
sealed class MainForm : Form, IShellView
{
    readonly Paths paths;
    readonly ShellLog log;
    ShellModel? shell;

    readonly ToolStrip toolbar = new();
    readonly ToolStripButton homeButton = new("Home");
    readonly ToolStripButton resetButton = new("Reset demo…");
    readonly ToolStripLabel signedIn = new();
    readonly ToolStripButton quitButton = new("Quit");

    readonly Panel notePanel = new();
    readonly Label noteText = new();

    readonly Panel startingPanel = new();
    readonly TableLayoutPanel card = new();
    readonly Label titleLabel = new();
    readonly Label stepLabel = new();
    readonly ProgressBar progress = new();
    readonly TableLayoutPanel serviceRows = new();
    readonly Label hintLabel = new();
    readonly Label detailsLabel = new();
    readonly Label logHeading = new();
    readonly TextBox logLines = new();
    readonly FlowLayoutPanel failureButtons = new();
    readonly Button tryAgain = new();
    readonly Button resetFromFailure = new();
    readonly Label logsPath = new();

    readonly WebView2 web = new();
    Task? creating;
    readonly List<string> scriptIds = [];
    // The origins the offline rules let through; read on WebView2's events (the UI thread).
    string appOrigin = "";
    string? gatewayOrigin;

    bool closeAllowed;
    bool quitting;
    bool offeredShortcut;

    /// <summary>
    /// Tests only (<see cref="HeadlessCheck"/>): no Start-menu offer or other dialog, and
    /// <see cref="Observe"/> is told of every request and navigation with the offline rules' decision.
    /// </summary>
    internal bool Headless { get; init; }

    /// <summary>Tests only: (kind, URL, allowed) for each request, navigation and new window, on the window's thread.</summary>
    internal Action<string, string, bool>? Observe { get; set; }

    public MainForm(Paths paths, ShellLog log)
    {
        this.paths = paths;
        this.log = log;
        Text = "Midgard";
        BackColor = Palette.Page;
        ForeColor = Palette.Text;
        Font = new Font("Segoe UI", 10f);
        MinimumSize = new Size(1100, 700);
        StartPosition = FormStartPosition.CenterScreen;
        WindowState = FormWindowState.Maximized;
        Size = new Size(1440, 900);

        BuildToolbar();
        BuildNote();
        BuildStarting();
        web.Dock = DockStyle.Fill;
        web.DefaultBackgroundColor = Palette.Page;
        web.AllowExternalDrop = false;
        web.Visible = false;

        // Docking order: the last added fills what the earlier ones leave.
        Controls.Add(web);
        Controls.Add(startingPanel);
        Controls.Add(notePanel);
        Controls.Add(toolbar);
        web.BringToFront();
        startingPanel.BringToFront();
    }

    /// <summary>
    /// Connects the shell (made with this form as its view) and starts it once the window
    /// shows, unless <paramref name="launch"/> is false (the headless check launches it itself).
    /// </summary>
    public void Attach(ShellModel model, bool launch = true)
    {
        shell = model;
        model.Changed += () => Post(Render);
        Shown += (_, _) =>
        {
            Render();
            if (launch) _ = model.LaunchAsync();
        };
        Microsoft.Win32.SystemEvents.PowerModeChanged += PowerModeChanged;
    }

    void PowerModeChanged(object? sender, Microsoft.Win32.PowerModeChangedEventArgs e)
    {
        if (e.Mode == Microsoft.Win32.PowerModes.Resume) shell?.DidWake();
    }

    // Layout

    void BuildToolbar()
    {
        toolbar.Dock = DockStyle.Top;
        toolbar.GripStyle = ToolStripGripStyle.Hidden;
        toolbar.BackColor = Palette.Card;
        toolbar.ForeColor = Palette.Text;
        toolbar.Padding = new Padding(8, 4, 8, 4);
        toolbar.RenderMode = ToolStripRenderMode.System;
        homeButton.ToolTipText = "The home page and the Presenter's guide";
        homeButton.Click += (_, _) => shell?.GoHome();
        resetButton.ToolTipText = "Restore the starting data";
        resetButton.Click += (_, _) => AskReset();
        quitButton.ToolTipText = "Stop every service and close Midgard";
        quitButton.Alignment = ToolStripItemAlignment.Right;
        quitButton.Click += (_, _) => Close();
        signedIn.Alignment = ToolStripItemAlignment.Right;
        signedIn.ForeColor = Palette.Muted;
        toolbar.Items.AddRange([homeButton, new ToolStripSeparator(), resetButton, quitButton, signedIn]);
    }

    void BuildNote()
    {
        notePanel.Dock = DockStyle.Top;
        notePanel.Height = 40;
        notePanel.BackColor = Palette.Card;
        notePanel.Padding = new Padding(16, 8, 16, 8);
        notePanel.Visible = false;
        var dismiss = new Button
        {
            Text = "Dismiss", Dock = DockStyle.Right, AutoSize = true, FlatStyle = FlatStyle.Flat,
            ForeColor = Palette.Text, BackColor = Palette.Card,
        };
        dismiss.FlatAppearance.BorderColor = Palette.Border;
        dismiss.Click += (_, _) => shell?.DismissNote();
        noteText.Dock = DockStyle.Fill;
        noteText.ForeColor = Palette.Warn;
        noteText.TextAlign = ContentAlignment.MiddleLeft;
        notePanel.Controls.Add(noteText);
        notePanel.Controls.Add(dismiss);
    }

    void BuildStarting()
    {
        startingPanel.Dock = DockStyle.Fill;
        startingPanel.BackColor = Palette.Page;
        startingPanel.Resize += (_, _) => CenterCard();

        card.BackColor = Palette.Card;
        card.Padding = new Padding(40);
        card.Width = 640;
        card.AutoSize = true;
        card.AutoSizeMode = AutoSizeMode.GrowAndShrink;
        card.ColumnCount = 1;
        card.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 560));
        card.SizeChanged += (_, _) => CenterCard();

        titleLabel.Font = new Font("Segoe UI Semibold", 20f);
        titleLabel.AutoSize = true;
        titleLabel.MaximumSize = new Size(560, 0);
        titleLabel.Margin = new Padding(0, 0, 0, 16);
        stepLabel.AutoSize = true;
        stepLabel.ForeColor = Palette.Muted;
        stepLabel.Margin = new Padding(0, 0, 0, 12);
        progress.Width = 560;
        progress.Height = 6;
        progress.Margin = new Padding(0, 0, 0, 16);
        serviceRows.AutoSize = true;
        serviceRows.ColumnCount = 3;
        serviceRows.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 28));
        serviceRows.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 160));
        serviceRows.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 360));
        serviceRows.Margin = new Padding(0, 0, 0, 16);
        hintLabel.AutoSize = true;
        hintLabel.ForeColor = Palette.Muted;
        hintLabel.Text = "This takes up to a minute.";
        detailsLabel.AutoSize = true;
        detailsLabel.MaximumSize = new Size(560, 0);
        detailsLabel.Margin = new Padding(0, 0, 0, 12);
        logHeading.AutoSize = true;
        logHeading.ForeColor = Palette.Muted;
        logHeading.Text = "Last lines of its log";
        logLines.Multiline = true;
        logLines.ReadOnly = true;
        logLines.ScrollBars = ScrollBars.Vertical;
        logLines.Width = 560;
        logLines.Height = 220;
        logLines.Font = new Font("Consolas", 9f);
        logLines.BackColor = Palette.Page;
        logLines.ForeColor = Palette.Text;
        logLines.BorderStyle = BorderStyle.None;
        logLines.Margin = new Padding(0, 4, 0, 12);
        failureButtons.AutoSize = true;
        failureButtons.FlowDirection = FlowDirection.LeftToRight;
        failureButtons.Margin = new Padding(0, 0, 0, 8);
        Style(tryAgain, "Try again");
        tryAgain.Click += (_, _) => _ = shell?.RetryAsync();
        Style(resetFromFailure, "Reset demo…");
        resetFromFailure.Click += (_, _) => AskReset();
        failureButtons.Controls.Add(tryAgain);
        failureButtons.Controls.Add(resetFromFailure);
        logsPath.AutoSize = true;
        logsPath.MaximumSize = new Size(560, 0);
        logsPath.ForeColor = Palette.Muted;
        logsPath.Font = new Font("Segoe UI", 8.5f);
        logsPath.Text = $"Logs: {paths.Logs}";

        foreach (var control in new Control[]
                 { titleLabel, stepLabel, progress, serviceRows, hintLabel, detailsLabel, logHeading, logLines, failureButtons, logsPath })
            card.Controls.Add(control);
        startingPanel.Controls.Add(card);
    }

    static void Style(Button button, string text)
    {
        button.Text = text;
        button.AutoSize = true;
        button.FlatStyle = FlatStyle.Flat;
        button.BackColor = Palette.Border;
        button.ForeColor = Palette.Text;
        button.Padding = new Padding(10, 4, 10, 4);
        button.Margin = new Padding(0, 0, 12, 0);
        button.FlatAppearance.BorderColor = Palette.Border;
    }

    void CenterCard() =>
        card.Location = new Point(Math.Max(0, (startingPanel.ClientSize.Width - card.Width) / 2),
                                  Math.Max(0, (startingPanel.ClientSize.Height - card.Height) / 2));

    // Rendering the shell's state

    void Render()
    {
        if (shell is null) return;
        var state = shell.State;
        var ready = state.Phase == ShellPhase.Ready;
        web.Visible = ready;
        startingPanel.Visible = !ready;
        homeButton.Enabled = ready;
        resetButton.Enabled = ready && state.CanReset;
        signedIn.Text = ready && state.SignedInAs is { } who ? $"Signed in as {who}" : "";
        notePanel.Visible = ready && state.Note is not null;
        noteText.Text = state.Note ?? "";

        var failed = state.Phase == ShellPhase.Failed && state.Failure is not null;
        var stopping = state.Phase == ShellPhase.Stopping;
        card.SuspendLayout();
        if (failed)
        {
            var failure = state.Failure!;
            var (summary, lines) = failure.Split();
            titleLabel.Text = "⚠ " + failure.Title;
            titleLabel.ForeColor = Palette.Danger;
            detailsLabel.Text = summary;
            logLines.Text = lines?.Replace("\n", "\r\n", StringComparison.Ordinal) ?? "";
            logHeading.Visible = logLines.Visible = lines is not null;
            failureButtons.Visible = failure.CanRetry;
            resetFromFailure.Visible = failure.CanReset;
            logsPath.Visible = failure.CanRetry;
        }
        else
        {
            titleLabel.Text = state.Title;
            titleLabel.ForeColor = Palette.Text;
            logHeading.Visible = logLines.Visible = failureButtons.Visible = logsPath.Visible = false;
        }
        detailsLabel.Visible = failed;
        stepLabel.Text = stopping ? "Stopping the services…" : state.Step ?? "";
        stepLabel.Visible = !failed && (stopping || state.Step is not null);
        progress.Visible = serviceRows.Visible = hintLabel.Visible = !failed && !stopping;
        progress.Maximum = Math.Max(1, state.ServiceOrder.Count);
        progress.Value = Math.Min(progress.Maximum, state.ReadyCount);
        RenderRows(state);
        card.ResumeLayout();
        CenterCard();

        if (ready && !offeredShortcut && !Headless)
        {
            offeredShortcut = true;
            BeginInvoke(OfferShortcut);
        }
    }

    void RenderRows(ShellState state)
    {
        serviceRows.SuspendLayout();
        foreach (var old in serviceRows.Controls.Cast<Control>().ToList()) old.Dispose();
        serviceRows.RowCount = state.ServiceOrder.Count;
        for (var i = 0; i < state.ServiceOrder.Count; i++)
        {
            var name = state.ServiceOrder[i];
            var status = state.Statuses.GetValueOrDefault(name);
            var (glyph, colour) = status switch
            {
                ServiceStatus.Ready => ("✓", Palette.Secure),
                ServiceStatus.Starting => ("…", Palette.Text),
                ServiceStatus.Restarting => ("↻", Palette.Warn),
                ServiceStatus.Failed => ("✕", Palette.Danger),
                _ => ("○", Palette.Muted),
            };
            serviceRows.Controls.Add(Row(glyph, colour), 0, i);
            serviceRows.Controls.Add(Row(ServiceNames.Friendly(name), Palette.Text), 1, i);
            serviceRows.Controls.Add(Row(ServiceNames.StatusLabel(status), Palette.Muted), 2, i);
        }
        serviceRows.ResumeLayout();

        static Label Row(string text, Color colour) =>
            new() { Text = text, ForeColor = colour, AutoSize = true, Margin = new Padding(0, 3, 0, 3) };
    }

    // Presenter actions

    void AskReset()
    {
        if (shell is null) return;
        shell.ResetDemo(() => MessageBox.Show(this,
            "This restores the starting data and removes changes made during the demo. It takes about a minute.",
            "Reset the demo?", MessageBoxButtons.OKCancel, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2)
            == DialogResult.OK, out _);
    }

    /// <summary>The first run's offer of a Start-menu shortcut, remembered in settings.json either way.</summary>
    void OfferShortcut()
    {
        var settings = AppSettings.Load(paths.Settings);
        if (!AppSettings.ShouldOfferShortcut(settings, paths.DevLayout, StartMenuShortcut.Exists())) return;
        var answer = MessageBox.Show(this,
            "Add Midgard to the Start menu? You can then open it from Start like any other app.",
            "Midgard", MessageBoxButtons.YesNo, MessageBoxIcon.Question);
        if (answer == DialogResult.Yes)
        {
            try
            {
                StartMenuShortcut.Create(Environment.ProcessPath ?? Application.ExecutablePath);
                settings.StartMenuShortcut = "added";
                log.Write($"start menu shortcut added: {StartMenuShortcut.File}");
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException or System.Runtime.InteropServices.COMException)
            {
                log.Write($"start menu shortcut failed: {e.Message}");
                MessageBox.Show(this, $"Midgard could not add itself to the Start menu ({e.Message}).", "Midgard",
                                MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
        }
        else
        {
            settings.StartMenuShortcut = "declined";
            log.Write("start menu shortcut declined");
        }
        try
        {
            settings.Save(paths.Settings);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            log.Write($"settings not saved: {e.Message}");
        }
    }

    /// <summary>Closing the window quits: every service is stopped first, then the window closes.</summary>
    protected override void OnFormClosing(FormClosingEventArgs e)
    {
        if (!closeAllowed && shell is not null)
        {
            e.Cancel = true;
            if (!quitting)
            {
                quitting = true;
                log.Write($"quit requested ({e.CloseReason})");
                shell.BeginShutdown();
                shell.ShutdownAsync().ContinueWith(_ => Post(() =>
                {
                    closeAllowed = true;
                    Close();
                }), TaskScheduler.Default);
            }
            return;
        }
        Microsoft.Win32.SystemEvents.PowerModeChanged -= PowerModeChanged;
        base.OnFormClosing(e);
    }

    /// <summary>Runs <paramref name="action"/> on the window's thread (at once if already on it).</summary>
    void Post(Action action)
    {
        if (IsDisposed || !IsHandleCreated) return;
        if (!InvokeRequired)
        {
            action();
            return;
        }
        try
        {
            BeginInvoke(action);
        }
        catch (InvalidOperationException)
        {
            // The window is closing.
        }
    }

    /// <summary><paramref name="work"/> on the window's thread, its outcome as a task for the caller.</summary>
    Task OnUiAsync(Func<Task> work)
    {
        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        if (IsDisposed || !IsHandleCreated)
        {
            done.SetException(new InvalidOperationException("the window is not open"));
            return done.Task;
        }
        Post(async () =>
        {
            try
            {
                await work();
                done.SetResult();
            }
            catch (Exception e)
            {
                done.SetException(e);
            }
        });
        return done.Task;
    }

    // IShellView: the web view

    public Task ConfigureAsync(IReadOnlyList<string> scripts, string appOrigin, string? gatewayOrigin) => OnUiAsync(async () =>
    {
        this.appOrigin = appOrigin;
        this.gatewayOrigin = gatewayOrigin;
        creating ??= Create(appOrigin);
        await creating;
        var core = web.CoreWebView2;
        foreach (var id in scriptIds) core.RemoveScriptToExecuteOnDocumentCreated(id);
        scriptIds.Clear();
        foreach (var script in scripts) scriptIds.Add(await core.AddScriptToExecuteOnDocumentCreatedAsync(script));
    });

    // Explicit: Form has a Load event.
    void IShellView.Load(string url) => Post(() =>
    {
        if (web.CoreWebView2 is { } core) core.Navigate(url);
    });

    public void Reload() => Post(() => web.CoreWebView2?.Reload());

    public void Evaluate(string script) => Post(() =>
    {
        if (web.CoreWebView2 is { } core) _ = core.ExecuteScriptAsync(script);
    });

    public Task ClearBrowsingDataAsync() => OnUiAsync(async () =>
    {
        if (web.CoreWebView2 is { } core) await core.Profile.ClearBrowsingDataAsync();
    });

    // Tests only (HeadlessCheck): load a page and wait for it, read it, and capture it.

    /// <summary>
    /// Navigates to <paramref name="url"/> and waits for a navigation to it to finish (a
    /// cancelled one, such as the load it replaced, is passed over): whether it loaded, and how.
    /// </summary>
    internal async Task<(bool Ok, string Detail)> NavigateAndWaitAsync(string url, TimeSpan timeout)
    {
        var done = new TaskCompletionSource<(bool, string)>(TaskCreationOptions.RunContinuationsAsynchronously);
        CoreWebView2? core = null;
        void Completed(object? sender, CoreWebView2NavigationCompletedEventArgs e)
        {
            if (!e.IsSuccess && e.WebErrorStatus == CoreWebView2WebErrorStatus.OperationCanceled) return;
            var source = core!.Source;
            if (e.IsSuccess && !source.StartsWith(url, StringComparison.Ordinal)) return;
            done.TrySetResult(e.IsSuccess ? (true, $"loaded {source}") : (false, $"{e.WebErrorStatus} for {source}"));
        }
        await OnUiAsync(() =>
        {
            core = web.CoreWebView2 ?? throw new InvalidOperationException("the web view was not created");
            core.NavigationCompleted += Completed;
            core.Navigate(url);
            return Task.CompletedTask;
        });
        try
        {
            var finished = await Task.WhenAny(done.Task, Task.Delay(timeout));
            return finished == done.Task ? await done.Task : (false, $"not loaded after {timeout.TotalSeconds:0} s");
        }
        finally
        {
            Post(() =>
            {
                core!.NavigationCompleted -= Completed;
            });
        }
    }

    /// <summary>The JSON result of <paramref name="script"/> in the open page.</summary>
    internal async Task<string> EvaluateForResultAsync(string script)
    {
        var result = "null";
        await OnUiAsync(async () =>
        {
            var core = web.CoreWebView2 ?? throw new InvalidOperationException("the web view was not created");
            result = await core.ExecuteScriptAsync(script);
        });
        return result;
    }

    /// <summary>The web view as it shows now, as a PNG in <paramref name="file"/>.</summary>
    internal Task CaptureAsync(string file) => OnUiAsync(async () =>
    {
        var core = web.CoreWebView2 ?? throw new InvalidOperationException("the web view was not created");
        await using var stream = File.Create(file);
        await core.CapturePreviewAsync(CoreWebView2CapturePreviewImageFormat.Png, stream);
    });

    /// <summary>The web view's size on screen, in pixels.</summary>
    internal Size WebViewSize => web.Size;

    /// <summary>
    /// Creates the WebView2 environment (profile in <see cref="Paths.WebView"/>) and sets the
    /// offline rules before anything loads. Besides the request filter and the navigation
    /// checks, the browser is given the app's own web server as its proxy: Chromium sends
    /// loopback traffic direct, so this changes nothing for the app's pages and the gateway,
    /// while anything that slips past WebResourceRequested (WebSockets do) goes to a server
    /// that refuses it (421 for another host, 405 for CONNECT) rather than out to the network.
    /// </summary>
    async Task Create(string origin)
    {
        var proxy = new Uri(origin);
        var options = new CoreWebView2EnvironmentOptions
        {
            AdditionalBrowserArguments = string.Join(" ",
                $"--proxy-server=http://127.0.0.1:{proxy.Port}",
                "--disable-background-networking", "--disable-component-update", "--no-pings",
                "--force-webrtc-ip-handling-policy=disable_non_proxied_udp"),
            AllowSingleSignOnUsingOSPrimaryAccount = false,
            AreBrowserExtensionsEnabled = false,
            // Crash reports stay on the PC rather than going to Microsoft.
            IsCustomCrashReportingEnabled = true,
            ExclusiveUserDataFolderAccess = true,
        };
        Directory.CreateDirectory(paths.WebView);
        var environment = await CoreWebView2Environment.CreateAsync(null, paths.WebView, options);
        await web.EnsureCoreWebView2Async(environment);
        var core = web.CoreWebView2;
        log.Write($"web view: WebView2 {environment.BrowserVersionString}, profile {paths.WebView}");

        var settings = core.Settings;
        settings.AreDevToolsEnabled = false;
        settings.AreBrowserAcceleratorKeysEnabled = false;
        settings.IsStatusBarEnabled = false;
        settings.IsReputationCheckingRequired = false;
        settings.IsPasswordAutosaveEnabled = false;
        settings.IsGeneralAutofillEnabled = false;
        settings.IsSwipeNavigationEnabled = false;
        settings.AreHostObjectsAllowed = false;
        settings.IsWebMessageEnabled = false;

        try
        {
            core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All,
                                               CoreWebView2WebResourceRequestSourceKinds.All);
        }
        catch (Exception e) when (e is NotImplementedException or InvalidCastException or ArgumentException)
        {
            // An older runtime without service-worker filtering: documents only.
            core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All);
        }
        core.WebResourceRequested += (_, e) =>
        {
            var uri = e.Request.Uri;
            var allowed = OfflineRules.IsAllowed(uri, appOrigin, gatewayOrigin);
            Observe?.Invoke("request", uri, allowed);
            if (allowed) return;
            log.Write($"web view: refused request {Short(uri)}");
            e.Response = environment.CreateWebResourceResponse(null, 403, "Forbidden", "Content-Type: text/plain");
        };
        core.NavigationStarting += (_, e) =>
        {
            var allowed = OfflineRules.IsAllowed(e.Uri, appOrigin, gatewayOrigin);
            Observe?.Invoke("navigation", e.Uri, allowed);
            if (allowed) return;
            log.Write($"web view: refused navigation to {Short(e.Uri)}");
            e.Cancel = true;
        };
        core.FrameNavigationStarting += (_, e) =>
        {
            var allowed = OfflineRules.IsAllowed(e.Uri, appOrigin, gatewayOrigin);
            Observe?.Invoke("frame navigation", e.Uri, allowed);
            if (allowed) return;
            log.Write($"web view: refused frame navigation to {Short(e.Uri)}");
            e.Cancel = true;
        };
        core.NewWindowRequested += (_, e) =>
        {
            // target=_blank and window.open: the same view, never a new window.
            e.Handled = true;
            var allowed = OfflineRules.IsAllowed(e.Uri, appOrigin, gatewayOrigin);
            Observe?.Invoke("new window", e.Uri, allowed);
            if (allowed) core.Navigate(e.Uri);
            else log.Write($"web view: refused new window for {Short(e.Uri)}");
        };
        core.DownloadStarting += (_, e) =>
        {
            log.Write($"web view: refused download of {Short(e.DownloadOperation.Uri)}");
            e.Cancel = true;
            e.Handled = true;
        };
        core.LaunchingExternalUriScheme += (_, e) =>
        {
            log.Write($"web view: refused external link {Short(e.Uri)}");
            e.Cancel = true;
        };
        core.PermissionRequested += (_, e) =>
        {
            // Camera, microphone, location, notifications …: never.
            e.State = CoreWebView2PermissionState.Deny;
            e.Handled = true;
        };
        core.BasicAuthenticationRequested += (_, e) => e.Cancel = true;
        core.ContextMenuRequested += (_, e) =>
        {
            var items = e.MenuItems;
            var names = items.Select(i => i.Kind == CoreWebView2ContextMenuItemKind.Separator ? null : i.Name).ToList();
            var keep = OfflineRules.KeepInContextMenu(names).ToHashSet();
            for (var i = items.Count - 1; i >= 0; i--)
            {
                if (!keep.Contains(i)) items.RemoveAt(i);
            }
            if (items.Count == 0) e.Handled = true;
        };
        core.NavigationCompleted += (_, e) =>
        {
            if (e.IsSuccess)
            {
                shell?.PageLoaded(core.Source);
            }
            else if (e.WebErrorStatus != CoreWebView2WebErrorStatus.OperationCanceled
                     && OfflineRules.IsAllowed(core.Source, appOrigin, gatewayOrigin))
            {
                shell?.PageFailed(core.Source, e.WebErrorStatus.ToString());
            }
        };
        core.ProcessFailed += (_, e) =>
        {
            log.Write($"web view: {e.ProcessFailedKind} ({e.Reason}, exit code {e.ExitCode})");
            switch (e.ProcessFailedKind)
            {
                case CoreWebView2ProcessFailedKind.RenderProcessExited:
                case CoreWebView2ProcessFailedKind.RenderProcessUnresponsive:
                    _ = shell?.ContentCrashedAsync(core.Source);
                    break;
                case CoreWebView2ProcessFailedKind.BrowserProcessExited:
                    shell?.PageFailed(core.Source, "the window's web view stopped; quit and reopen Midgard");
                    break;
            }
        };
    }

    /// <summary>A URL for the log: a data: URL is cut short.</summary>
    static string Short(string? url) => url is null ? "?" : url.Length > 200 ? url[..200] + "…" : url;
}
