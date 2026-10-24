using System.Diagnostics;
using System.Globalization;
using System.Net.Http.Headers;
using System.Text.Json;
using System.Text.Json.Nodes;
using Midgard.Core;

namespace Midgard;

/// <summary>
/// <c>Midgard.exe --headless-check &lt;report.json&gt; [--model-stub &lt;python.exe&gt; &lt;stub_llm.py&gt;]
/// [--no-window] [--timeout-minutes N]</c>: the end-to-end check CI runs (the
/// windows-shell-e2e job, docs/plans/2026-10-23-windows-app-w2-shell.md Task 7). Hidden and
/// for tests only: presenters never use it, and it is not in any shortcut or help text.
/// </summary>
/// <remarks>
/// <para>It runs the real flow (<see cref="ShellModel"/> with <see cref="ShellWiring"/>'s steps,
/// each timed into the report) against the package Midgard.exe is in (<c>helpers\</c> and
/// <c>resources\</c> beside it; MIDGARD_APP_HOME and MIDGARD_APP_LOGS as usual): instance lock,
/// system check, ACL check, sweep, working copy, web server, the window's web view, the
/// services, sign-in. Then it fetches <c>/</c> and <c>/heimdall/</c> from the web server, reads the
/// gateway's picture for the fixture's layers, and in the real window (<see cref="MainForm"/>
/// with WebView2) loads the home page and Heimdall and captures each to a PNG beside the
/// report, recording every request and navigation the web view made with the offline
/// rules' decision. Then Reset demo (stop, reset, start), then Quit, then a look for
/// anything left running: the app's job must be empty, no process may run from
/// <c>helpers\</c>, and no service's pid may still run its executable.</para>
/// <para>The model server: <c>--model-stub</c> runs tools/win-app/stub_llm.py with that python.exe
/// in place of llama-server (<see cref="ShellWiring"/>'s test-only command; the launcher
/// starts .exe files only, and python.exe is one). Without it the real
/// <c>helpers\llama\llama-server.exe</c> is used.</para>
/// <para>The window part needs an interactive desktop and the WebView2 runtime; without
/// either (or with <c>--no-window</c>) it is skipped, the report says why, and the rest runs
/// with a view that shows nothing.</para>
/// <para>Guarded (<see cref="HeadlessGuard"/>): it runs only with MIDGARD_HEADLESS_CHECK=1 and both
/// MIDGARD_APP_HOME and MIDGARD_APP_LOGS set explicitly, as the windows-shell-e2e job sets
/// them, because it resets the working copy (Reset demo, confirmed by itself) and may run a
/// model stub. Otherwise it refuses with exit 3 before it reads or writes anything, so a
/// presenter who stumbles on the switch can never wipe their own demo.</para>
/// <para>Exit 0 only when every step passed; 1 when one failed, 2 for bad arguments, 3 when
/// the guard refused, 4 when
/// it did not finish within the timeout (the report is written then too, and the services
/// end with this process: the app's job is kill-on-close).</para>
/// </remarks>
static class HeadlessCheck
{
    public const string Switch = "--headless-check";

    /// <summary>The fixture's layers (tools/win-app/run-suite.py's LAYERS).</summary>
    static readonly string[] Layers = ["air", "sea", "land", "events", "utilities"];

    const string Usage =
        "usage: Midgard.exe --headless-check <report.json> [--model-stub <python.exe> <stub_llm.py>] [--no-window] [--timeout-minutes N]";

    public static int Run(string[] args)
    {
        if (HeadlessGuard.Refusal(HeadlessGuard.ProcessEnvironment()) is { } refused)
        {
            Console.Error.WriteLine($"Midgard.exe: {refused}");
            return 3;
        }
        string? reportFile = null, python = null, stub = null;
        var noWindow = false;
        var minutes = 25;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--model-stub" when i + 2 < args.Length:
                    python = Path.GetFullPath(args[++i]);
                    stub = Path.GetFullPath(args[++i]);
                    break;
                case "--no-window":
                    noWindow = true;
                    break;
                case "--timeout-minutes" when i + 1 < args.Length
                                             && int.TryParse(args[i + 1], NumberStyles.None, CultureInfo.InvariantCulture, out var m) && m > 0:
                    minutes = m;
                    i++;
                    break;
                case var a when reportFile is null && !a.StartsWith("--", StringComparison.Ordinal):
                    reportFile = Path.GetFullPath(a);
                    break;
                default:
                    Console.Error.WriteLine(Usage);
                    return 2;
            }
        }
        if (reportFile is null)
        {
            Console.Error.WriteLine(Usage);
            return 2;
        }
        var outDir = Path.GetDirectoryName(reportFile)!;
        Directory.CreateDirectory(outDir);

        var report = new CheckReport();
        var paths = Paths.Current();
        var log = new ShellLog(Path.Join(paths.Logs, "shell.log"));
        log.Write($"headless check: Midgard {Application.ProductVersion}, report {reportFile}, resources {paths.Resources}, "
                  + $"helpers {paths.Helpers}, home {paths.Home}, logs {paths.Logs}, web view {paths.WebView}");
        report.Facts["paths"] = new JsonObject
        {
            ["exe"] = Environment.ProcessPath, ["resources"] = paths.Resources, ["helpers"] = paths.Helpers,
            ["devLayout"] = paths.DevLayout, ["home"] = paths.Home, ["logs"] = paths.Logs, ["webView"] = paths.WebView,
        };
        report.Facts["pc"] = new JsonObject
        {
            ["os"] = Environment.OSVersion.VersionString, ["interactive"] = Environment.UserInteractive,
            ["session"] = WindowsProcesses.CurrentSession, ["logicalProcessors"] = Environment.ProcessorCount,
            ["physicalCores"] = ModelServer.PhysicalCores(), ["webView2"] = WindowsFacts.WebView2Version(),
            ["memoryBytes"] = WindowsFacts.InstalledMemoryBytes(),
        };
        report.Facts["model"] = python is null ? "llama-server (helpers\\llama)" : $"stub: {python} {stub}";
        if (python is not null && (!File.Exists(python) || !File.Exists(stub)))
        {
            report.Step("model stub present", false, TimeSpan.Zero, $"{python} or {stub} is missing");
            return report.Finish(reportFile, log);
        }

        Func<ushort, Command>? model = python is null ? null
            : port => new Command(python, ["-u", stub!, "--port", port.ToString(CultureInfo.InvariantCulture)]);
        var wiring = new ShellWiring(paths, log, model);
        var steps = Instrument(wiring.Steps(), report);
        var driver = new Driver(report, paths, log, wiring, outDir);

        using var watchdog = new System.Threading.Timer(_ =>
        {
            report.Step("finished in time", false, TimeSpan.FromMinutes(minutes), $"not finished after {minutes} minutes");
            report.Write(reportFile, 4);
            log.Write($"headless check: timed out after {minutes} minutes");
            Environment.Exit(4);
        }, null, TimeSpan.FromMinutes(minutes), Timeout.InfiniteTimeSpan);

        var skipWindow = noWindow ? "--no-window was given"
            : !Environment.UserInteractive ? "this session has no interactive desktop"
            : WindowsFacts.WebView2Version() is null ? "the WebView2 runtime is not installed"
            : null;
        report.Facts["window"] = skipWindow is null ? "the real window (MainForm, WebView2)" : $"skipped: {skipWindow}";

        ShellModel shell;
        Stopwatch? quitting = null;
        if (skipWindow is null)
        {
            Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
            Application.ThreadException += (_, e) =>
            {
                log.Write($"headless check: unexpected UI error: {e.Exception}");
                report.Step("no unexpected error", false, TimeSpan.Zero, $"{e.Exception.GetType().Name}: {e.Exception.Message}");
            };
            using var form = new MainForm(paths, log) { Headless = true };
            form.Observe = report.Request;
            shell = new ShellModel(steps, form);
            if (!shell.Claim()) return report.Finish(reportFile, log);
            form.Attach(shell, launch: false);
            form.Shown += async (_, _) =>
            {
                try
                {
                    await driver.RunAsync(shell, form);
                }
                catch (Exception e)
                {
                    log.Write($"headless check: {e}");
                    report.Step("no unexpected error", false, TimeSpan.Zero, $"{e.GetType().Name}: {e.Message}");
                }
                // Quit as the toolbar's Quit does: the form stops everything, then closes.
                quitting = Stopwatch.StartNew();
                form.Close();
            };
            Application.Run(form);
        }
        else
        {
            shell = new ShellModel(steps, new NullView());
            if (!shell.Claim()) return report.Finish(reportFile, log);
            Task.Run(async () =>
            {
                try
                {
                    await driver.RunAsync(shell, null).ConfigureAwait(false);
                }
                catch (Exception e)
                {
                    log.Write($"headless check: {e}");
                    report.Step("no unexpected error", false, TimeSpan.Zero, $"{e.GetType().Name}: {e.Message}");
                }
                quitting = Stopwatch.StartNew();
                await shell.ShutdownAsync().ConfigureAwait(false);
            }).GetAwaiter().GetResult();
        }

        var shutdown = shell.ShutdownAsync();
        var quitOk = shutdown.Wait(TimeSpan.FromMinutes(2)) && shutdown.IsCompletedSuccessfully;
        report.Step("quit", quitOk && shell.State.Phase == ShellPhase.Stopping, quitting?.Elapsed ?? TimeSpan.Zero,
                    $"phase {shell.State.Phase}");
        driver.CheckNothingLeft();
        return report.Finish(reportFile, log);
    }

    /// <summary>The real steps, each timed into the report with whether it went as it should.</summary>
    static ShellSteps Instrument(ShellSteps s, CheckReport r) => new()
    {
        ClaimInstance = () => r.Time("instance lock", s.ClaimInstance, c => (c == InstanceClaim.Held, c.ToString())),
        SystemProblem = () => r.Time("system check", s.SystemProblem, p => (p is null, p ?? "this PC is supported")),
        FolderProblem = () => r.Time("ACL check", s.FolderProblem, p => (p is null, p ?? "the app's folders are private")),
        Sweep = held => r.TimeValueAsync("sweep", () => s.Sweep(held), o => (!o.Contains("could not end", StringComparison.Ordinal), o)),
        PrepareHome = cancellation => r.TimeAsync("home prepare", () => s.PrepareHome(cancellation)),
        LoadUsers = () => r.Time("demo users", s.LoadUsers, u => (u.Count > 0, $"{u.Count} demo users")),
        StartWebServer = () => r.TimeValueAsync("web server", s.StartWebServer, p => (p > 0, $"127.0.0.1:{p}")),
        ChoosePorts = s.ChoosePorts,
        Specs = s.Specs,
        StartServices = (specs, cancellation) =>
        {
            r.Specs = specs;
            return r.TimeAsync("supervisor start", () => s.StartServices(specs, cancellation),
                               string.Join(", ", specs.Select(x => x.Name)));
        },
        StopServices = () => r.TimeAsync("supervisor stop", s.StopServices),
        CloseServices = () => r.TimeAsync("supervisor close", s.CloseServices),
        Statuses = s.Statuses,
        SignIn = (api, user, patient, cancellation) => r.TimeValueAsync("sign in", () => s.SignIn(api, user, patient, cancellation),
            i => (true, $"{i.Subject} (clearance {i.Clearance ?? "none"}, roles {string.Join(",", i.Roles)}), expires {i.ExpiresAt:O}")),
        ResetHome = () => r.Time("home reset", s.ResetHome,
            report => (report.Leftover is null, report.Leftover is null ? "replaced" : $"left {report.Leftover}: {report.Reason}")),
        FinalSweep = () => r.TimeValueAsync("final sweep", s.FinalSweep, o => (o.EndsWith("nothing running", StringComparison.Ordinal), o)),
        StopWebServer = () => r.TimeAsync("web server stop", s.StopWebServer),
        MarkStopping = s.MarkStopping,
        ReleaseInstance = () => r.Time("instance release", () =>
        {
            s.ReleaseInstance();
            return true;
        }, _ => (true, "released")),
        PreparedAnswersUrl = s.PreparedAnswersUrl,
        Now = s.Now,
        Log = s.Log,
        RefreshPoll = s.RefreshPoll,
    };

    /// <summary>The checks after the start, in order.</summary>
    sealed class Driver(CheckReport report, Paths paths, ShellLog log, ShellWiring wiring, string outDir)
    {
        static readonly TimeSpan StartTimeout = TimeSpan.FromMinutes(10);
        readonly List<(string Name, int Pid, string Executable)> pids = [];
        static readonly HttpClient Http = new(new SocketsHttpHandler { UseProxy = false, AllowAutoRedirect = false })
        {
            Timeout = TimeSpan.FromSeconds(30),
        };

        public async Task RunAsync(ShellModel shell, MainForm? form)
        {
            var watch = Stopwatch.StartNew();
            await shell.LaunchAsync().WaitAsync(StartTimeout);
            if (!Ready(shell, "launch", watch)) return;
            var origin = shell.Origin!;
            var gateway = shell.GatewayOrigin!;

            await Fetch("fetch /", origin + "/");
            await Fetch("fetch /heimdall/", origin + "/heimdall/");
            await Picture(gateway);
            await Analyse(gateway);

            if (form is not null)
            {
                await Page(form, "home", origin + "/", TimeSpan.FromSeconds(3));
                await Page(form, "heimdall", origin + "/heimdall/", TimeSpan.FromSeconds(12));
            }

            watch.Restart();
            var offered = shell.ResetDemo(() => true, out var done);
            if (!offered)
            {
                report.Step("reset demo", false, watch.Elapsed, "Reset demo was not offered");
            }
            else
            {
                await done.WaitAsync(StartTimeout);
                if (Ready(shell, "reset demo", watch))
                {
                    await Fetch("fetch / after the reset", origin + "/");
                    if (form is not null)
                    {
                        var loaded = Stopwatch.StartNew();
                        var (ok, detail) = await form.NavigateAndWaitAsync(origin + "/", TimeSpan.FromSeconds(60));
                        report.Step("window: home after the reset", ok, loaded.Elapsed, detail);
                    }
                }
            }

            if (form is not null)
            {
                var blocked = report.Blocked();
                report.Step("no blocked requests", blocked.Count == 0, TimeSpan.Zero,
                            blocked.Count == 0 ? $"{report.RequestCount} requests and navigations, all allowed"
                                : $"{blocked.Count} blocked: {string.Join(" ", blocked.Take(10))}");
            }
        }

        /// <summary>Whether the start (or reset) ended with the window ready, with the services' states.</summary>
        bool Ready(ShellModel shell, string step, Stopwatch watch)
        {
            var state = shell.State;
            var services = string.Join(", ", state.ServiceOrder.Select(n =>
                $"{n} {ServiceNames.StatusLabel(state.Statuses.GetValueOrDefault(n))}"));
            var ok = state.Phase == ShellPhase.Ready;
            report.Step(step, ok, watch.Elapsed, ok
                ? $"ready, signed in as {state.SignedInAs}; {services}"
                : $"{state.Phase}: {state.Failure?.Title} {state.Failure?.Details}; {services}");
            if (report.Specs is { } specs)
            {
                foreach (var spec in specs)
                {
                    if (wiring.Supervisor.PidOf(spec.Name) is { } pid) pids.Add((spec.Name, pid, spec.Executable));
                }
            }
            return ok;
        }

        async Task Fetch(string step, string url)
        {
            var watch = Stopwatch.StartNew();
            try
            {
                using var response = await Http.GetAsync(url);
                var body = await response.Content.ReadAsStringAsync();
                var type = response.Content.Headers.ContentType?.MediaType ?? "";
                var ok = response.StatusCode == System.Net.HttpStatusCode.OK && type == "text/html"
                         && body.Contains("<html", StringComparison.OrdinalIgnoreCase);
                report.Step(step, ok, watch.Elapsed, $"{(int)response.StatusCode} {type}, {body.Length} characters, title \"{Title(body)}\"");
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException)
            {
                report.Step(step, false, watch.Elapsed, e.Message);
            }
        }

        static string Title(string html)
        {
            var start = html.IndexOf("<title>", StringComparison.OrdinalIgnoreCase);
            if (start < 0) return "";
            var end = html.IndexOf("</title>", start, StringComparison.OrdinalIgnoreCase);
            return end < 0 ? "" : html[(start + 7)..end].Trim();
        }

        /// <summary>The gateway's operating picture holds the fixture's layers (as run-suite.py checks).</summary>
        async Task Picture(string gateway)
        {
            var watch = Stopwatch.StartNew();
            var api = new Uri(gateway + "/api/v1");
            string detail;
            try
            {
                var inHome = Path.Join(paths.Home, "demo-users.json");
                var user = DemoUsers.Load(inHome)[0];
                var session = await Session.LoginAsync(api, user, Session.PatientLoopback);
                var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(120);
                while (true)
                {
                    using var request = new HttpRequestMessage(HttpMethod.Get, gateway + "/api/v1/heimdall/picture");
                    request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", session.Token);
                    using var response = await Http.SendAsync(request);
                    var data = await response.Content.ReadAsByteArrayAsync();
                    var layers = new SortedSet<string>(StringComparer.Ordinal);
                    var count = 0;
                    if (response.IsSuccessStatusCode)
                    {
                        using var doc = JsonDocument.Parse(data);
                        if (doc.RootElement.TryGetProperty("entities", out var entities) && entities.ValueKind == JsonValueKind.Array)
                        {
                            foreach (var e in entities.EnumerateArray())
                            {
                                count++;
                                if (e.TryGetProperty("layer", out var l) && l.ValueKind == JsonValueKind.String) layers.Add(l.GetString()!);
                            }
                        }
                    }
                    var missing = Layers.Where(l => !layers.Contains(l)).ToList();
                    detail = $"HTTP {(int)response.StatusCode}, {count} entities, layers {string.Join(",", layers)}"
                             + (missing.Count > 0 ? $"; missing {string.Join(",", missing)}" : "");
                    if (missing.Count == 0)
                    {
                        report.Step("gateway picture has the fixture's layers", true, watch.Elapsed, detail);
                        return;
                    }
                    if (DateTime.UtcNow >= deadline) break;
                    await Task.Delay(TimeSpan.FromSeconds(2));
                }
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException or SessionException
                                          or JsonException or IOException or InvalidDataException)
            {
                detail = $"{e.GetType().Name}: {e.Message}";
            }
            report.Step("gateway picture has the fixture's layers", false, watch.Elapsed, detail);
        }

        /// <summary>The fixture's camera with a stored clip (tools/win-app/make-fixture.py --camera-clip).</summary>
        const string ClipCamera = "JamCams_00000.00001";

        /// <summary>
        /// The camera analytics through the gateway, as the console's Analyse button asks for
        /// them: the service ready (optional, so the start did not wait for it), then
        /// <c>POST /api/v1/heimdall/cameras/{id}/analyse</c> on the fixture's clip camera, timed,
        /// with 40 frames analysed and the result sent to Heimdall; the picture then holds the
        /// <c>AI-{id}</c> land-camera-ai entity, and <c>GET …/trend</c> a point by the analyst.
        /// </summary>
        async Task Analyse(string gateway)
        {
            var watch = Stopwatch.StartNew();
            var spec = report.Specs?.FirstOrDefault(s => s.Name == "analytics");
            if (spec is null)
            {
                report.Step("camera analytics: in the suite", false, TimeSpan.Zero,
                            "no analytics service: the package lacks them or the token could not be written (shell.log)");
                return;
            }
            var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(180);
            var status = wiring.Supervisor.StatusOf("analytics");
            while (status is not (ServiceStatus.Ready or ServiceStatus.Failed) && DateTime.UtcNow < deadline)
            {
                await Task.Delay(500);
                status = wiring.Supervisor.StatusOf("analytics");
            }
            report.Step("camera analytics: ready", status is ServiceStatus.Ready, watch.Elapsed,
                        $"{ServiceNames.StatusLabel(status)} {(status as ServiceStatus.Failed)?.Reason}".Trim());
            if (status is not ServiceStatus.Ready) return;
            if (wiring.Supervisor.PidOf("analytics") is { } pid)
                pids.Add(("analytics", pid, spec.Executable));

            var api = gateway + "/api/v1/heimdall/cameras/" + ClipCamera;
            string detail;
            SessionInfo session;
            try
            {
                var user = DemoUsers.Load(Path.Join(paths.Home, "demo-users.json"))[0];
                session = await Session.LoginAsync(new Uri(gateway + "/api/v1"), user, Session.PatientLoopback);
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException or SessionException
                                          or JsonException or IOException or InvalidDataException)
            {
                report.Step("camera analytics: Analyse through the gateway", false, TimeSpan.Zero, $"sign-in: {e.Message}");
                return;
            }

            watch.Restart();
            try
            {
                using var request = new HttpRequestMessage(HttpMethod.Post, api + "/analyse")
                {
                    Content = new StringContent("", System.Text.Encoding.UTF8, "application/json"),
                };
                request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", session.Token);
                using var response = await Http.SendAsync(request);
                var body = await response.Content.ReadAsStringAsync();
                var took = watch.Elapsed;
                var ok = response.StatusCode == System.Net.HttpStatusCode.OK;
                detail = $"HTTP {(int)response.StatusCode} in {took.TotalSeconds:0.00} s";
                if (ok)
                {
                    var attrs = JsonNode.Parse(body)?["attrs"];
                    var frames = attrs?["ai_frames"]?.GetValue<int>() ?? 0;
                    var vehicles = attrs?["ai_vehicles"]?.GetValue<int>() ?? 0;
                    var sent = JsonNode.Parse(body)?["sent"]?.GetValue<string>() ?? "";
                    detail += $": {frames} frames ({took.TotalMilliseconds / Math.Max(1, frames):0} ms a frame), counts "
                              + $"{attrs?["ai_counts"]}, vehicles {vehicles}, moving {attrs?["ai_moving"]}, model {attrs?["ai_model"]}; sent {sent}";
                    ok = attrs?["camera_id"]?.GetValue<string>() == ClipCamera && frames == 40 && vehicles >= 1
                         && sent.StartsWith("accepted=1 ", StringComparison.Ordinal);
                    report.Facts["analyse"] = new JsonObject
                    {
                        ["camera"] = ClipCamera, ["took_s"] = Math.Round(took.TotalSeconds, 2), ["frames"] = frames,
                        ["ms_per_frame"] = Math.Round(took.TotalMilliseconds / Math.Max(1, frames)),
                        ["counts"] = attrs?["ai_counts"]?.ToString(), ["vehicles"] = vehicles, ["sent"] = sent,
                        ["threads"] = spec.Arguments.SkipWhile(a => a != "--threads").Skip(1).FirstOrDefault(),
                    };
                }
                else
                {
                    detail += $": {body[..Math.Min(body.Length, 300)]}";
                }
                report.Step("camera analytics: Analyse through the gateway", ok, took, detail);
                if (!ok) return;
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException or JsonException
                                          or InvalidOperationException or FormatException)
            {
                report.Step("camera analytics: Analyse through the gateway", false, watch.Elapsed, $"{e.GetType().Name}: {e.Message}");
                return;
            }

            // The result in the picture: Heimdall has the AI- entity.
            watch.Restart();
            detail = "";
            var found = false;
            var until = DateTime.UtcNow + TimeSpan.FromSeconds(60);
            try
            {
                while (!found)
                {
                    using var request = new HttpRequestMessage(HttpMethod.Get, gateway + "/api/v1/heimdall/picture");
                    request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", session.Token);
                    using var response = await Http.SendAsync(request);
                    var entities = JsonNode.Parse(await response.Content.ReadAsStringAsync())?["entities"]?.AsArray();
                    var entity = entities?.FirstOrDefault(e => e?["id"]?.GetValue<string>() == "AI-" + ClipCamera);
                    detail = entity is null ? $"HTTP {(int)response.StatusCode}, {entities?.Count ?? 0} entities, no AI-{ClipCamera}"
                        : $"AI-{ClipCamera}: {entity["kind"]}, layer {entity["layer"]}, sources {entity["sources"]?.ToJsonString()}";
                    found = entity?["kind"]?.GetValue<string>() == "land-camera-ai";
                    if (found || DateTime.UtcNow >= until) break;
                    await Task.Delay(1000);
                }
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException or JsonException or InvalidOperationException)
            {
                detail = $"{e.GetType().Name}: {e.Message}";
            }
            report.Step("camera analytics: the land-camera-ai entity in the picture", found, watch.Elapsed, detail);

            // The trend: the analysis is one point, by the analyst.
            watch.Restart();
            try
            {
                using var request = new HttpRequestMessage(HttpMethod.Get, api + "/trend");
                request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", session.Token);
                using var response = await Http.SendAsync(request);
                var body = await response.Content.ReadAsStringAsync();
                var points = response.IsSuccessStatusCode ? JsonNode.Parse(body)?["points"]?.AsArray() : null;
                var by = points?.Select(p => p?["by"]?.GetValue<string>()).ToList() ?? [];
                report.Step("camera analytics: a trend point", points is { Count: >= 1 } && by.Contains(session.Subject), watch.Elapsed,
                            $"HTTP {(int)response.StatusCode}, {points?.Count ?? 0} points by {string.Join(",", by)}"
                            + (points is { Count: >= 1 } ? $"; last {points[^1]?.ToJsonString()}" : $"; {body[..Math.Min(body.Length, 200)]}"));
            }
            catch (Exception e) when (e is HttpRequestException or TaskCanceledException or JsonException or InvalidOperationException)
            {
                report.Step("camera analytics: a trend point", false, watch.Elapsed, $"{e.GetType().Name}: {e.Message}");
            }
        }

        /// <summary>Loads <paramref name="url"/> in the window, lets it draw, records what it shows and captures it to &lt;name&gt;.png.</summary>
        async Task Page(MainForm form, string name, string url, TimeSpan settle)
        {
            var watch = Stopwatch.StartNew();
            var (ok, detail) = await form.NavigateAndWaitAsync(url, TimeSpan.FromSeconds(60));
            report.Step($"window: {name} loaded", ok, watch.Elapsed, detail);
            if (!ok) return;
            // The document is complete, then a few seconds for the console to fetch and draw.
            var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(30);
            while (DateTime.UtcNow < deadline && await form.EvaluateForResultAsync("document.readyState") != "\"complete\"")
                await Task.Delay(250);
            await Task.Delay(settle);
            var facts = await form.EvaluateForResultAsync(
                "JSON.stringify({title: document.title, url: location.href, canvases: document.querySelectorAll('canvas').length,"
                + " text: (document.body ? document.body.innerText : '').replace(/\\s+/g, ' ').slice(0, 400)})");
            report.Facts[$"page {name}"] = ParsePageFacts(facts);
            watch.Restart();
            var file = Path.Join(outDir, $"{name}.png");
            try
            {
                await form.CaptureAsync(file);
                var size = new FileInfo(file).Length;
                report.Step($"window: {name}.png captured", size > 0, watch.Elapsed,
                            $"{file}, {size} bytes, web view {form.WebViewSize.Width}x{form.WebViewSize.Height}");
                report.Screenshot(file);
            }
            catch (Exception e)
            {
                log.Write($"headless check: capture of {name} failed: {e}");
                report.Step($"window: {name}.png captured", false, watch.Elapsed, $"{e.GetType().Name}: {e.Message}");
            }
        }

        /// <summary>ExecuteScriptAsync gives the JSON of the string JSON.stringify made: unwrap it.</summary>
        static JsonNode? ParsePageFacts(string result)
        {
            try
            {
                return JsonNode.Parse(JsonNode.Parse(result)?.GetValue<string>() ?? "null");
            }
            catch (Exception e) when (e is JsonException or InvalidOperationException or FormatException)
            {
                return result;
            }
        }

        /// <summary>After quitting: the app's job is empty, nothing runs from helpers\, and no service's pid still runs its executable.</summary>
        public void CheckNothingLeft()
        {
            var watch = Stopwatch.StartNew();
            var problems = new List<string>();
            var inJob = JobLauncher.ActiveProcesses;
            if (inJob != 0) problems.Add($"{inJob} processes still in the app's job");
            foreach (var p in Orphans.Running(paths.Helpers)) problems.Add($"pid {p.Pid} runs {p.Executable}");
            foreach (var (name, pid, exe) in pids.Distinct())
            {
                if (WindowsProcesses.Runs(pid, exe)) problems.Add($"{name} (pid {pid}) still runs {exe}");
            }
            report.Step("nothing left running", problems.Count == 0, watch.Elapsed,
                        problems.Count == 0 ? $"app job empty, nothing under {paths.Helpers}, {pids.Distinct().Count()} service pids gone"
                            : string.Join("; ", problems));
        }
    }

    /// <summary>A view that shows nothing, for a run without the window.</summary>
    sealed class NullView : IShellView
    {
        public Task ConfigureAsync(IReadOnlyList<string> scripts, string appOrigin, string? gatewayOrigin) => Task.CompletedTask;
        public void Load(string url) { }
        public void Reload() { }
        public void Evaluate(string script) { }
        public Task ClearBrowsingDataAsync() => Task.CompletedTask;
        public Task CloseAsync() => Task.CompletedTask;
    }
}

/// <summary>What the headless check found, written as JSON.</summary>
sealed class CheckReport
{
    readonly Lock gate = new();
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly DateTimeOffset started = DateTimeOffset.Now;
    readonly JsonArray steps = new();
    readonly JsonArray requests = new();
    readonly JsonArray screenshots = new();
    readonly List<string> blocked = [];
    readonly Dictionary<string, int> seen = [];
    bool failed;
    bool written;

    /// <summary>The PC, the paths, the window, the pages: written as given.</summary>
    public JsonObject Facts { get; } = new();

    /// <summary>The services of the last start.</summary>
    public IReadOnlyList<ServiceSpec>? Specs { get; set; }

    public int RequestCount
    {
        get { lock (gate) return requests.Count; }
    }

    /// <summary>One step; a name used before gets " (2)", " (3)" ….</summary>
    public void Step(string name, bool ok, TimeSpan took, string? detail = null)
    {
        lock (gate)
        {
            var n = seen[name] = seen.GetValueOrDefault(name) + 1;
            steps.Add(new JsonObject
            {
                ["step"] = n == 1 ? name : $"{name} ({n})",
                ["ok"] = ok,
                ["at_s"] = Math.Round(clock.Elapsed.TotalSeconds - took.TotalSeconds, 2),
                ["took_s"] = Math.Round(took.TotalSeconds, 2),
                ["detail"] = detail,
            });
            if (!ok) failed = true;
        }
    }

    public T Time<T>(string name, Func<T> work, Func<T, (bool Ok, string Detail)> judge)
    {
        var watch = Stopwatch.StartNew();
        T value;
        try
        {
            value = work();
        }
        catch (Exception e)
        {
            Step(name, false, watch.Elapsed, $"{e.GetType().Name}: {e.Message}");
            throw;
        }
        var (ok, detail) = judge(value);
        Step(name, ok, watch.Elapsed, detail);
        return value;
    }

    public async Task<T> TimeValueAsync<T>(string name, Func<Task<T>> work, Func<T, (bool Ok, string Detail)> judge)
    {
        var watch = Stopwatch.StartNew();
        T value;
        try
        {
            value = await work().ConfigureAwait(false);
        }
        catch (Exception e)
        {
            Step(name, false, watch.Elapsed, $"{e.GetType().Name}: {e.Message}");
            throw;
        }
        var (ok, detail) = judge(value);
        Step(name, ok, watch.Elapsed, detail);
        return value;
    }

    public async Task TimeAsync(string name, Func<Task> work, string? detail = null)
    {
        var watch = Stopwatch.StartNew();
        try
        {
            await work().ConfigureAwait(false);
        }
        catch (Exception e)
        {
            Step(name, false, watch.Elapsed, $"{e.GetType().Name}: {e.Message}");
            throw;
        }
        Step(name, true, watch.Elapsed, detail);
    }

    /// <summary>A request or navigation the web view made, and whether the offline rules let it through.</summary>
    public void Request(string kind, string url, bool allowed)
    {
        var shown = url.Length > 300 ? url[..300] + "…" : url;
        lock (gate)
        {
            requests.Add(new JsonObject
            {
                ["at_s"] = Math.Round(clock.Elapsed.TotalSeconds, 2), ["kind"] = kind, ["url"] = shown,
                ["decision"] = allowed ? "allowed" : "blocked",
            });
            if (!allowed) blocked.Add($"{kind} {shown}");
        }
    }

    public IReadOnlyList<string> Blocked()
    {
        lock (gate) return [.. blocked];
    }

    public void Screenshot(string file)
    {
        lock (gate) screenshots.Add(Path.GetFileName(file));
    }

    /// <summary>Writes the report; the exit code (0 only when every step passed).</summary>
    public int Finish(string file, ShellLog log)
    {
        int code;
        lock (gate) code = failed || steps.Count == 0 ? 1 : 0;
        Write(file, code);
        log.Write($"headless check: {(code == 0 ? "passed" : "FAILED")}, report {file}");
        return code;
    }

    public void Write(string file, int exitCode)
    {
        string text;
        lock (gate)
        {
            if (written) return;
            written = true;
            var passed = steps.Count(s => s!["ok"]!.GetValue<bool>());
            var doc = new JsonObject
            {
                ["result"] = exitCode == 0 ? "passed" : "failed",
                ["exitCode"] = exitCode,
                ["started"] = started.ToString("O", CultureInfo.InvariantCulture),
                ["took_s"] = Math.Round(clock.Elapsed.TotalSeconds, 2),
                ["passed"] = passed,
                ["failed"] = steps.Count - passed,
                ["failedSteps"] = Strings(steps.Where(s => !s!["ok"]!.GetValue<bool>()).Select(s => s!["step"]!.GetValue<string>())),
                ["facts"] = Facts.DeepClone(),
                ["steps"] = steps.DeepClone(),
                ["screenshots"] = screenshots.DeepClone(),
                ["blockedRequests"] = Strings(blocked),
                ["requests"] = requests.DeepClone(),
            };
            text = doc.ToJsonString(new JsonSerializerOptions { WriteIndented = true });
        }
        File.WriteAllText(file, text + "\n");
    }

    static JsonArray Strings(IEnumerable<string> items)
    {
        var array = new JsonArray();
        foreach (var item in items) array.Add((JsonNode?)JsonValue.Create(item));
        return array;
    }
}
