using System.Globalization;
using System.Net;
using System.Text;
using System.Threading.Channels;

namespace Midgard.Core;

/// <summary>What the starting screen shows for one service.</summary>
public abstract record ServiceStatus
{
    ServiceStatus() { }

    public sealed record Starting : ServiceStatus;
    public sealed record Ready : ServiceStatus;

    /// <summary>
    /// Launched again after a back-off (attempt counts from 1): it exited after it was
    /// ready, or (attempt 1 only) before it was ready the first time. <c>Reason</c>: how it
    /// exited, or for a later attempt why the last relaunch failed.
    /// </summary>
    public sealed record Restarting(int Attempt, string? Reason) : ServiceStatus;

    /// <summary>"port in use" when it could not bind, else why, with its last log lines.</summary>
    public sealed record Failed(string Reason) : ServiceStatus;

    public sealed record Stopped : ServiceStatus;
}

/// <summary>One status change.</summary>
public sealed record ServiceEvent(string Service, ServiceStatus Status);

/// <summary>Why <see cref="Supervisor.StartAsync"/> failed.</summary>
public abstract class SupervisorException(string message) : Exception(message);

/// <summary>
/// The service exited before it was ready because a port was taken: choose fresh ports,
/// <see cref="Supervisor.StopAllAsync"/>, and start again.
/// </summary>
public sealed class PortInUseException(string service) : SupervisorException($"{service}: port in use")
{
    public string Service { get; } = service;
}

public sealed class ServiceFailedException(string service, string reason) : SupervisorException($"{service}: {reason}")
{
    public string Service { get; } = service;
    public string Reason { get; } = reason;
    /// <summary>It exited before it was ready (rather than not being ready in time).</summary>
    internal bool ExitedBeforeReady { get; init; }
}

/// <summary><see cref="Supervisor.StopAllAsync"/> was called while starting, or the supervisor is closed.</summary>
public sealed class SupervisorStoppedException() : SupervisorException("stopped");

/// <summary>
/// Starts the suite's services in order, waits for each to be ready, restarts one that
/// exits after it was ready, and stops them all in reverse order. The port of
/// apps/mac/Sources/MidgardShellCore/Supervisor.swift; the differences are Windows':
/// </summary>
/// <remarks>
/// <para><b>Job Objects.</b> The processes come from an <see cref="IProcessLauncher"/>, on
/// Windows <see cref="JobLauncher"/>: every service is in the app's kill-on-close job, so if
/// the shell crashes Windows ends them all, and in a job of its own, so stopping one ends
/// what it started too.</para>
/// <para><b>Stopping.</b> The Mac sends SIGTERM, then SIGKILL after a grace period. None of
/// the services handles SIGTERM (the default action ends the process at once, which is why
/// run-native.sh notes the lock files a stop leaves), and on Windows none has a console
/// control handler or another stop request either, so there is no graceful stop to ask for:
/// a stop ends the service's job (TerminateJobObject), as run-suite.py does. The stores are
/// written for that: the fabric journal and the Heimdall stores flush each acknowledged write
/// before it returns and replace files by rename (a kill leaves the old file or the new,
/// a torn history tail is dropped on open), and a lock left by a dead process is taken over
/// (services/runtime/src/lock.rs). <see cref="Timing.StopGrace"/> is how long the stop waits
/// for the job to empty before saying so in the log.</para>
/// <para><b>One retry.</b> A service that exits before it is ready (not for a taken port,
/// which <c>SuiteStart</c> answers with fresh ports, and not after its timeout) is launched
/// once more after the first back-off before the start fails, published as
/// <see cref="ServiceStatus.Restarting"/> attempt 1. A first start on a cold PC can trip
/// over itself (an antivirus scan holding a file); the Mac fails at once.</para>
/// </remarks>
public sealed class Supervisor : IAsyncDisposable
{
    public sealed record Timing
    {
        /// <summary>The first restart waits this long, each later one twice as long …</summary>
        public TimeSpan BackoffBase { get; init; } = TimeSpan.FromSeconds(2);
        /// <summary>… but never longer than this.</summary>
        public TimeSpan BackoffCap { get; init; } = TimeSpan.FromSeconds(60);
        /// <summary>A service up this long since it was last ready starts again from the first delay.</summary>
        public TimeSpan StableAfter { get; init; } = TimeSpan.FromSeconds(300);
        /// <summary>How often a health URL is asked.</summary>
        public TimeSpan HealthInterval { get; init; } = TimeSpan.FromSeconds(0.5);
        /// <summary>How often the log is read for the ready line and the process checked.</summary>
        public TimeSpan PollInterval { get; init; } = TimeSpan.FromSeconds(0.1);
        /// <summary>How long a stop waits for the service and what it started to be gone.</summary>
        public TimeSpan StopGrace { get; init; } = TimeSpan.FromSeconds(15);

        /// <summary>The delay before restart <paramref name="attempt"/> (1, 2, …): 2, 4, 8 … s up to the cap.</summary>
        public TimeSpan Backoff(int attempt)
        {
            var seconds = BackoffBase.TotalSeconds * Math.Pow(2, Math.Max(1, attempt) - 1);
            return TimeSpan.FromSeconds(Math.Min(BackoffCap.TotalSeconds, seconds));
        }
    }

    internal const int TailLines = 20;
    internal const int TailBytes = 64 * 1024;
    /// <summary>A log longer than this at start is moved to <c>&lt;name&gt;.log.1</c> first.</summary>
    internal const long LogLimit = 20L * 1024 * 1024;
    /// <summary>Launches of a service that exits before it is ready, after the first.</summary>
    public const int StartRetries = 1;

    readonly Timing timing;
    readonly IProcessLauncher launcher;
    readonly HttpClient http;
    readonly Channel<ServiceEvent> channel =
        Channel.CreateUnbounded<ServiceEvent>(new UnboundedChannelOptions { SingleReader = true });
    readonly Lock gate = new();
    // Guarded by `gate`.
    List<Service> services = [];
    readonly Dictionary<string, ServiceStatus> latest = [];
    int generation;
    bool closed;
    Task? lastStop;

    /// <param name="launcher">Defaults to <see cref="JobLauncher"/> (Windows only).</param>
    public Supervisor(Timing? timing = null, IProcessLauncher? launcher = null)
    {
        this.timing = timing ?? new Timing();
        this.launcher = launcher ?? DefaultLauncher();
        http = new HttpClient(new SocketsHttpHandler { UseProxy = false, AllowAutoRedirect = false })
        {
            Timeout = TimeSpan.FromSeconds(Math.Max(0.5, this.timing.HealthInterval.TotalSeconds * 2)),
        };
    }

    static IProcessLauncher DefaultLauncher()
    {
        if (OperatingSystem.IsWindows()) return new JobLauncher();
        throw new PlatformNotSupportedException("the supervisor's default launcher uses Windows Job Objects");
    }

    /// <summary>Every status change, in order. One reader only (the starting screen).</summary>
    public IAsyncEnumerable<ServiceEvent> Statuses(CancellationToken cancellation = default) =>
        channel.Reader.ReadAllAsync(cancellation);

    /// <summary>The last status published for the service called <paramref name="name"/>.</summary>
    public ServiceStatus? StatusOf(string name)
    {
        lock (gate) return latest.GetValueOrDefault(name);
    }

    /// <summary>The pid of the service called <paramref name="name"/> while its process runs.</summary>
    public int? PidOf(string name)
    {
        lock (gate)
        {
            var run = services.LastOrDefault(s => s.Spec.Name == name)?.Run;
            return run is null || run.HasExited ? null : run.Process.Pid;
        }
    }

    /// <summary>
    /// Launches each spec in order and waits for it to be ready. On the first that fails,
    /// stops it, starts no more and throws (those already ready keep running; call
    /// <see cref="StopAllAsync"/>). An optional spec is launched and not waited for: it
    /// becomes ready in the background, or is published failed (noted in its log) without
    /// throwing or holding up the rest; once ready it restarts like any other. A log over
    /// 20 MB is moved to <c>&lt;name&gt;.log.1</c> first. Cancelling stops the waiting
    /// (<see cref="OperationCanceledException"/>); what was launched is left for
    /// <see cref="StopAllAsync"/>. May be called again after <see cref="StopAllAsync"/>.
    /// </summary>
    /// <exception cref="PortInUseException"/>
    /// <exception cref="ServiceFailedException"/>
    /// <exception cref="SupervisorStoppedException"/>
    public async Task StartAsync(IEnumerable<ServiceSpec> specs, CancellationToken cancellation = default)
    {
        int generation;
        lock (gate) generation = this.generation;
        foreach (var spec in specs)
        {
            var service = new Service(spec);
            lock (gate)
            {
                if (generation != this.generation || closed) throw new SupervisorStoppedException();
                services.Add(service);
            }
            Publish(service, new ServiceStatus.Starting());
            Rotate(spec.LogFile);
            if (spec.Optional)
            {
                StartOptional(service);
                continue;
            }
            try
            {
                await LaunchWithRetry(service, cancellation).ConfigureAwait(false);
            }
            catch (PortInUseException)
            {
                Publish(service, new ServiceStatus.Failed("port in use"));
                throw;
            }
            catch (ServiceFailedException e)
            {
                Publish(service, new ServiceStatus.Failed(e.Reason));
                throw;
            }
        }
    }

    /// <summary>Launches and waits; one exit before ready is launched again after the first back-off.</summary>
    async Task LaunchWithRetry(Service service, CancellationToken cancellation)
    {
        for (var retry = 0; ; retry++)
        {
            try
            {
                await LaunchAndWait(service, cancellation).ConfigureAwait(false);
                return;
            }
            catch (ServiceFailedException e) when (e.ExitedBeforeReady && retry < StartRetries)
            {
                var attempt = retry + 1;
                var delay = timing.Backoff(attempt);
                Note(service.Spec.LogFile,
                     $"{service.Spec.Name}: {FirstLine(e.Reason)} Launching it again in {Seconds(delay)} s (attempt {attempt})");
                Publish(service, new ServiceStatus.Restarting(attempt, e.Reason));
                await Sleep(delay, service, cancellation).ConfigureAwait(false);
            }
        }
    }

    /// <summary>
    /// Waits for an optional service in the background (<see cref="StopAllAsync"/> cancels
    /// the wait and ends the process); a failure is published, never thrown.
    /// </summary>
    void StartOptional(Service service)
    {
        var spec = service.Spec;
        service.Background = Task.Run(async () =>
        {
            string reason;
            try
            {
                await LaunchWithRetry(service, service.Stopping.Token).ConfigureAwait(false);
                return;
            }
            catch (Exception e) when (e is SupervisorStoppedException or OperationCanceledException)
            {
                return;
            }
            catch (PortInUseException)
            {
                reason = "port in use";
            }
            catch (ServiceFailedException e)
            {
                reason = e.Reason;
            }
            catch (Exception e)
            {
                reason = e.Message;
            }
            lock (gate)
            {
                if (service.Stopped) return;
            }
            Note(spec.LogFile, $"{spec.Name} (optional) did not start: {FirstLine(reason)}; carrying on without it");
            Publish(service, new ServiceStatus.Failed(reason));
        });
    }

    /// <summary>
    /// Stops every service in reverse start order, each with everything it started. Nothing
    /// restarts afterwards. Safe to call again or concurrently: a later call returns once
    /// the earlier one has finished.
    /// </summary>
    public Task StopAllAsync()
    {
        List<Service> list;
        Task task;
        lock (gate)
        {
            generation++;
            list = services;
            services = [];
            foreach (var service in list) service.Stopped = true;
            var previous = lastStop;
            task = Task.Run(async () =>
            {
                if (previous is not null) await previous.ConfigureAwait(false);
                await Stop(Enumerable.Reverse(list)).ConfigureAwait(false);
            });
            lastStop = task;
        }
        // Outside the lock: cancelling may run a waiting start's continuation on this thread.
        foreach (var service in list) service.Stopping.Cancel();
        return task;
    }

    /// <summary>
    /// <see cref="StopAllAsync"/> for good: a start running now or called later throws
    /// <see cref="SupervisorStoppedException"/> without launching anything. For quitting.
    /// </summary>
    public Task CloseAsync()
    {
        lock (gate) closed = true;
        return StopAllAsync();
    }

    /// <summary>True once <see cref="CloseAsync"/> has been called.</summary>
    public bool IsClosed
    {
        get { lock (gate) return closed; }
    }

    public async ValueTask DisposeAsync()
    {
        await CloseAsync().ConfigureAwait(false);
        channel.Writer.TryComplete();
        http.Dispose();
    }

    // Running one service

    /// <summary>One service from a start; guarded by the supervisor's lock.</summary>
    sealed class Service(ServiceSpec spec)
    {
        public ServiceSpec Spec { get; } = spec;
        public Run? Run;
        /// <summary>The current run has been declared ready (and has not exited since).</summary>
        public bool IsReady;
        public DateTime? ReadyAt;
        public int Attempt;
        public bool Stopped;
        /// <summary>Cancelled by <see cref="StopAllAsync"/>: ends waits, back-offs and restarts.</summary>
        public readonly CancellationTokenSource Stopping = new();
        public Task? Background;
    }

    /// <summary>One launch of a service; guarded by the supervisor's lock.</summary>
    sealed class Run(IServiceProcess process, long logOffset)
    {
        public IServiceProcess Process { get; } = process;
        /// <summary>Where the log ended at launch: only output after it counts.</summary>
        public long LogOffset { get; } = logOffset;
        public bool HasExited;
        public string ExitDescription = "";
    }

    /// <summary>Waits; <see cref="SupervisorStoppedException"/> if the service is stopped meanwhile.</summary>
    static async Task Sleep(TimeSpan delay, Service service, CancellationToken cancellation)
    {
        try
        {
            await Task.Delay(delay, Combined(service, cancellation)).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (!cancellation.IsCancellationRequested)
        {
            throw new SupervisorStoppedException();
        }
    }

    static CancellationToken Combined(Service service, CancellationToken cancellation) =>
        cancellation.CanBeCanceled
            ? CancellationTokenSource.CreateLinkedTokenSource(service.Stopping.Token, cancellation).Token
            : service.Stopping.Token;

    Run Launch(Service service)
    {
        var spec = service.Spec;
        long offset;
        try
        {
            if (Path.GetDirectoryName(Path.GetFullPath(spec.LogFile)) is { } dir) Directory.CreateDirectory(dir);
            offset = File.Exists(spec.LogFile) ? new FileInfo(spec.LogFile).Length : 0;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            throw new ServiceFailedException(spec.Name, $"cannot open {spec.LogFile}: {e.Message}");
        }
        Run run;
        // Under the lock, so StopAll either sees this run or this launch sees StopAll.
        lock (gate)
        {
            if (service.Stopped) throw new SupervisorStoppedException();
            IServiceProcess process;
            try
            {
                process = launcher.Start(spec.Executable, spec.Arguments, spec.Environment, spec.LogFile);
            }
            catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception
                                          or UnauthorizedAccessException)
            {
                throw new ServiceFailedException(spec.Name, $"cannot start {spec.Executable}: {e.Message}");
            }
            run = new Run(process, offset);
            service.Run = run;
            service.IsReady = false;
        }
        _ = run.Process.Exited.ContinueWith(t => OnExited(service, run, t.Result), CancellationToken.None,
                                            TaskContinuationOptions.None, TaskScheduler.Default);
        return run;
    }

    /// <summary>Launches the service and returns once it is ready (status ready published).</summary>
    async Task LaunchAndWait(Service service, CancellationToken cancellation)
    {
        var spec = service.Spec;
        var run = Launch(service);
        var deadline = DateTime.UtcNow + spec.ReadyTimeout;
        var scanner = new LogScanner(spec.LogFile, run.LogOffset);
        var nextHealth = DateTime.UtcNow;
        while (true)
        {
            lock (gate)
            {
                if (service.Stopped) throw new SupervisorStoppedException();  // StopAll ends it
            }
            cancellation.ThrowIfCancellationRequested();
            var isReady = false;
            switch (spec.Readiness)
            {
                case Readiness.LogLine line:
                    isReady = scanner.Contains(line.Text);
                    break;
                case Readiness.Health health when DateTime.UtcNow >= nextHealth:
                    isReady = await Healthy(health.Url, service.Stopping.Token).ConfigureAwait(false);
                    nextHealth = DateTime.UtcNow + timing.HealthInterval;
                    break;
            }
            if (isReady && MarkReady(service, run)) return;

            string? how;
            lock (gate) how = run.HasExited ? run.ExitDescription : null;
            if (how is not null)
            {
                var output = Read(spec.LogFile, run.LogOffset);
                // Closing its job ends anything it started.
                run.Process.Dispose();
                var lower = output.ToLowerInvariant();
                if (PortInUseMarks.Any(lower.Contains)) throw new PortInUseException(spec.Name);
                throw new ServiceFailedException(spec.Name, $"{spec.Name} {how} before it was ready." + Tail(output))
                {
                    ExitedBeforeReady = true,
                };
            }
            if (DateTime.UtcNow >= deadline)
            {
                await Terminate(service, run).ConfigureAwait(false);
                var output = Read(spec.LogFile, run.LogOffset);
                throw new ServiceFailedException(spec.Name,
                    $"{spec.Name} was not ready after {Seconds(spec.ReadyTimeout)} s." + Tail(output));
            }
            await Sleep(timing.PollInterval, service, cancellation).ConfigureAwait(false);
        }
    }

    /// <summary>What a service writes when its port is taken (Rust's io::Error on Windows: "Only one usage of each socket address …", os error 10048).</summary>
    static readonly string[] PortInUseMarks =
    [
        "address already in use", "address in use", "couldn't bind", "could not bind",
        "only one usage of each socket address", "os error 10048",
    ];

    /// <summary>Declares <paramref name="run"/> ready unless it has exited or the service was stopped meanwhile.</summary>
    bool MarkReady(Service service, Run run) =>
        Publish(service, new ServiceStatus.Ready(), () =>
        {
            if (service.Run != run || run.HasExited) return false;
            service.IsReady = true;
            service.ReadyAt = DateTime.UtcNow;
            return true;
        });

    async Task<bool> Healthy(Uri url, CancellationToken token)
    {
        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, url);
            request.Headers.CacheControl = new System.Net.Http.Headers.CacheControlHeaderValue { NoCache = true };
            using var response = await http.SendAsync(request, token).ConfigureAwait(false);
            return response.StatusCode == HttpStatusCode.OK;
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException or IOException)
        {
            // Not answering yet (or stopped: the loop sees that next).
            return false;
        }
    }

    /// <summary>How a process exited, as the Mac says it.</summary>
    internal static string Describe(int code) =>
        // NTSTATUS-like codes (an access violation, a TerminateProcess from elsewhere) read better in hex.
        code < 0 ? $"exited with status 0x{unchecked((uint)code):X8}" : $"exited with status {code}";

    /// <summary>The exit callback: after ready, and unless stopped, schedules the restarts.</summary>
    void OnExited(Service service, Run run, int code)
    {
        var how = Describe(code);
        lock (gate)
        {
            run.HasExited = true;
            run.ExitDescription = how;
            if (service.Run != run || !service.IsReady || service.Stopped) return;
            service.IsReady = false;
            if (service.ReadyAt is { } readyAt && DateTime.UtcNow - readyAt >= timing.StableAfter) service.Attempt = 0;
            service.Background = Task.Run(() => Restart(service, how));
        }
    }

    async Task Restart(Service service, string how)
    {
        var spec = service.Spec;
        var why = how;
        while (true)
        {
            int attempt;
            lock (gate)
            {
                if (service.Stopped) return;
                attempt = ++service.Attempt;
            }
            var delay = timing.Backoff(attempt);
            Note(spec.LogFile, $"{spec.Name}: {FirstLine(why)}; restarting in {Seconds(delay)} s (attempt {attempt})");
            Publish(service, new ServiceStatus.Restarting(attempt, why));
            try
            {
                // The previous run's job is closed first: anything it left running ends.
                Run? previous;
                lock (gate) previous = service.Run;
                previous?.Process.Dispose();
                await Task.Delay(delay, service.Stopping.Token).ConfigureAwait(false);
                await LaunchAndWait(service, CancellationToken.None).ConfigureAwait(false);
                return;
            }
            catch (Exception e) when (e is SupervisorStoppedException or OperationCanceledException)
            {
                return;
            }
            catch (PortInUseException)
            {
                why = "port in use";
            }
            catch (ServiceFailedException e)
            {
                why = e.Reason;
            }
            catch (Exception e)
            {
                why = e.Message;
            }
        }
    }

    // Stopping

    async Task Stop(IEnumerable<Service> list)
    {
        foreach (var service in list)
        {
            Run? run;
            lock (gate) run = service.Run;
            if (run is not null) await Terminate(service, run).ConfigureAwait(false);
            if (service.Background is { } background)
            {
                try
                {
                    await background.ConfigureAwait(false);
                }
                catch (Exception)
                {
                    // Its outcome was published already; stopping is what matters now.
                }
            }
            // A restart may have launched a run between the lookup and the cancel.
            lock (gate) run = service.Run;
            if (run is not null) await Terminate(service, run).ConfigureAwait(false);
            Publish(service, new ServiceStatus.Stopped());
        }
    }

    /// <summary>
    /// Ends the service's job (the service and everything it started) and waits up to
    /// <see cref="Timing.StopGrace"/> for it to be gone, then closes its handles (which ends
    /// anything still in the job).
    /// </summary>
    async Task Terminate(Service service, Run run)
    {
        var process = run.Process;
        process.Kill();
        var deadline = DateTime.UtcNow + timing.StopGrace;
        while (!process.Exited.IsCompleted || process.ActiveProcesses > 0)
        {
            if (DateTime.UtcNow >= deadline)
            {
                Note(service.Spec.LogFile,
                     $"{service.Spec.Name} did not exit within {Seconds(timing.StopGrace)} s of being ended");
                break;
            }
            await Task.Delay(20).ConfigureAwait(false);
            process.Kill();
        }
        process.Dispose();
        lock (gate)
        {
            run.HasExited = true;
            if (run.ExitDescription.Length == 0) run.ExitDescription = "was stopped";
        }
    }

    // Helpers

    /// <summary>
    /// Records and writes <paramref name="status"/> unless the service was stopped (only
    /// Stopped follows that) or <paramref name="condition"/> (run under the lock) says no.
    /// Written under the lock, so <see cref="Statuses"/> sees changes in the order they happened.
    /// </summary>
    bool Publish(Service service, ServiceStatus status, Func<bool>? condition = null)
    {
        lock (gate)
        {
            if ((status is not ServiceStatus.Stopped && service.Stopped) || (condition is not null && !condition()))
                return false;
            latest[service.Spec.Name] = status;
            channel.Writer.TryWrite(new ServiceEvent(service.Spec.Name, status));
            return true;
        }
    }

    /// <summary>Moves a log over <see cref="LogLimit"/> to <c>&lt;name&gt;.log.1</c>, replacing an older one.</summary>
    internal static void Rotate(string file)
    {
        try
        {
            var info = new FileInfo(file);
            if (!info.Exists || info.Length <= LogLimit) return;
            File.Move(file, file + ".1", overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // Still open elsewhere: appended to instead.
        }
    }

    /// <summary>Appends one line from the shell to a service's log.</summary>
    internal static void Note(string file, string line)
    {
        try
        {
            using var stream = new FileStream(file, FileMode.Append, FileAccess.Write,
                                              FileShare.ReadWrite | FileShare.Delete);
            var bytes = Encoding.UTF8.GetBytes($"[midgard shell] {line}\n");
            stream.Write(bytes);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    /// <summary>The log from <paramref name="offset"/> on (at most the last <see cref="TailBytes"/>).</summary>
    internal static string Read(string file, long offset)
    {
        try
        {
            using var stream = new FileStream(file, FileMode.Open, FileAccess.Read,
                                              FileShare.ReadWrite | FileShare.Delete);
            var end = stream.Length;
            var start = Math.Max(offset, end > TailBytes ? end - TailBytes : 0);
            if (start >= end) return "";
            stream.Seek(start, SeekOrigin.Begin);
            var buffer = new byte[end - start];
            stream.ReadExactly(buffer);
            return Encoding.UTF8.GetString(buffer);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return "";
        }
    }

    /// <summary>The last <see cref="TailLines"/> lines, for a failure message.</summary>
    internal static string Tail(string output)
    {
        var lines = output.Split('\n').Select(l => l.EndsWith('\r') ? l[..^1] : l).ToList();
        if (lines.Count > 0 && lines[^1].Length == 0) lines.RemoveAt(lines.Count - 1);
        if (lines.Count == 0) return " It wrote nothing.";
        return " Last log lines:\n" + string.Join("\n", lines.TakeLast(TailLines));
    }

    static string FirstLine(string text) => text.Split('\n')[0];

    internal static string Seconds(TimeSpan value) => value.TotalSeconds.ToString("G", CultureInfo.InvariantCulture);
}

/// <summary>Reads a log as it grows and looks for a line, including across two reads.</summary>
sealed class LogScanner(string file, long start)
{
    long offset = start;
    byte[] carry = [];

    public bool Contains(string text)
    {
        var pattern = Encoding.UTF8.GetBytes(text);
        if (pattern.Length == 0) return true;
        byte[] data;
        try
        {
            using var stream = new FileStream(file, FileMode.Open, FileAccess.Read,
                                              FileShare.ReadWrite | FileShare.Delete);
            if (stream.Length <= offset) return false;
            stream.Seek(offset, SeekOrigin.Begin);
            data = new byte[stream.Length - offset];
            var read = stream.ReadAtLeast(data, data.Length, throwOnEndOfStream: false);
            if (read == 0) return false;
            if (read < data.Length) data = data[..read];
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return false;
        }
        offset += data.Length;
        byte[] window = [.. carry, .. data];
        if (window.AsSpan().IndexOf(pattern) >= 0) return true;
        carry = window[Math.Max(0, window.Length - (pattern.Length - 1))..];
        return false;
    }
}
