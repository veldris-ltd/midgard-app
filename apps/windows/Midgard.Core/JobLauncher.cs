using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using Microsoft.Win32.SafeHandles;

namespace Midgard.Core;

/// <summary>A Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: closing its last handle ends every process in it.</summary>
[SupportedOSPlatform("windows")]
sealed unsafe class JobObject : IDisposable
{
    internal readonly Win32Handle Handle;

    public JobObject()
    {
        Handle = Kernel32.CreateJobObject(0, null);
        if (Handle.IsInvalid) throw Kernel32.LastError("CreateJobObjectW");
        var info = new Kernel32.JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
        info.BasicLimitInformation.LimitFlags = Kernel32.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if (!Kernel32.SetInformationJobObject(Handle, Kernel32.JobObjectExtendedLimitInformation, &info,
                                              (uint)sizeof(Kernel32.JOBOBJECT_EXTENDED_LIMIT_INFORMATION)))
        {
            var error = Kernel32.LastError("SetInformationJobObject");
            Handle.Dispose();
            throw error;
        }
    }

    public void Assign(nint process)
    {
        if (!Kernel32.AssignProcessToJobObject(Handle, process)) throw Kernel32.LastError("AssignProcessToJobObject");
    }

    /// <summary>Ends every process in the job (and in the jobs nested in it).</summary>
    public void Terminate() => Kernel32.TerminateJobObject(Handle, 1);

    /// <summary>The processes in the job (and in the jobs nested in it) still running; 0 once it is closed.</summary>
    public int ActiveProcesses
    {
        get
        {
            if (Handle.IsClosed) return 0;
            var info = new Kernel32.JOBOBJECT_BASIC_ACCOUNTING_INFORMATION();
            return Kernel32.QueryInformationJobObject(Handle, Kernel32.JobObjectBasicAccountingInformation, &info,
                                                      (uint)sizeof(Kernel32.JOBOBJECT_BASIC_ACCOUNTING_INFORMATION), null)
                ? (int)info.ActiveProcesses
                : 0;
        }
    }

    public void Dispose() => Handle.Dispose();
}

/// <summary>
/// Starts services inside Job Objects, so none outlives the app. Every service is in the
/// app's job, one per process and held open for the process's whole life (a static that
/// is never closed): when Midgard.exe exits or crashes Windows closes the handle and,
/// because the job is JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, ends every service and anything
/// a service started. Each service also gets a job of its own, nested in the app's, so it
/// can be stopped with everything it started (a replay's heimdall-source runs) and nothing
/// else: tools/win-app/run-suite.py's one job per service.
/// </summary>
/// <remarks>
/// <para>The process is created suspended (CreateProcessW with CREATE_SUSPENDED), put in
/// both jobs, and only then resumed. A process started and assigned afterwards could run
/// for a moment outside its jobs, and a child it started in that moment (replay starting
/// heimdall-source) would never be in them. A process created in a job is in it from its
/// first instruction, and so is every process it creates.</para>
/// <para>Only the null device (stdin) and the log (stdout and stderr) are inherited, through
/// PROC_THREAD_ATTRIBUTE_HANDLE_LIST; no other handle of the shell leaks into a service.
/// CREATE_NO_WINDOW: the services are console programs, and a GUI parent has no console to
/// share, so each would otherwise open a console window.</para>
/// <para>If the shell itself runs inside a job (a CI runner, a test host), the app's job
/// nests inside that one, which Windows 8 and later allow.</para>
/// </remarks>
[SupportedOSPlatform("windows")]
public sealed unsafe class JobLauncher : IProcessLauncher
{
    static readonly Lazy<JobObject> appJob = new(() => new JobObject());

    /// <summary>How many processes the app's job holds (every service of every supervisor, and their children).</summary>
    public static int ActiveProcesses => appJob.Value.ActiveProcesses;

    public IServiceProcess Start(string executable, IReadOnlyList<string> arguments,
                                 IReadOnlyDictionary<string, string>? environment, string logFile)
    {
        var app = appJob.Value;
        var inheritable = new Kernel32.SECURITY_ATTRIBUTES
        {
            nLength = sizeof(Kernel32.SECURITY_ATTRIBUTES),
            bInheritHandle = 1,
        };
        using var input = Kernel32.CreateFile("NUL", Kernel32.GENERIC_READ,
                                              Kernel32.FILE_SHARE_READ | Kernel32.FILE_SHARE_WRITE,
                                              &inheritable, Kernel32.OPEN_EXISTING, Kernel32.FILE_ATTRIBUTE_NORMAL, 0);
        if (input.IsInvalid) throw new IOException($"cannot open NUL: {Kernel32.LastError("CreateFileW").Message}");
        // Appending: every write goes to the end, whoever else writes (the shell's notes).
        using var output = Kernel32.CreateFile(logFile, Kernel32.FILE_APPEND_DATA | Kernel32.SYNCHRONIZE,
                                               Kernel32.FILE_SHARE_READ | Kernel32.FILE_SHARE_WRITE | Kernel32.FILE_SHARE_DELETE,
                                               &inheritable, Kernel32.OPEN_ALWAYS, Kernel32.FILE_ATTRIBUTE_NORMAL, 0);
        if (output.IsInvalid)
            throw new IOException($"cannot open {logFile}: {Kernel32.LastError("CreateFileW").Message}");

        var job = new JobObject();
        try
        {
            var info = Create(executable, arguments, environment, input, output);
            var process = new Win32Handle(info.hProcess);
            using var thread = new Win32Handle(info.hThread);
            try
            {
                app.Assign(info.hProcess);
                job.Assign(info.hProcess);
                if (Kernel32.ResumeThread(info.hThread) == uint.MaxValue) throw Kernel32.LastError("ResumeThread");
            }
            catch (Exception e)
            {
                Kernel32.TerminateProcess(info.hProcess, 1);
                process.Dispose();
                throw new IOException(e.Message, e);
            }
            return new JobProcess(info.dwProcessId, process, job);
        }
        catch
        {
            job.Dispose();
            throw;
        }
    }

    static Kernel32.PROCESS_INFORMATION Create(string executable, IReadOnlyList<string> arguments,
                                               IReadOnlyDictionary<string, string>? environment,
                                               Win32Handle input, Win32Handle output)
    {
        nuint size = 0;
        Kernel32.InitializeProcThreadAttributeList(0, 1, 0, &size);
        var list = Marshal.AllocHGlobal((nint)size);
        var handles = stackalloc nint[2];
        handles[0] = input.DangerousGetHandle();
        handles[1] = output.DangerousGetHandle();
        var initialised = false;
        try
        {
            if (!Kernel32.InitializeProcThreadAttributeList(list, 1, 0, &size))
                throw Kernel32.LastError("InitializeProcThreadAttributeList");
            initialised = true;
            if (!Kernel32.UpdateProcThreadAttribute(list, 0, Kernel32.PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles,
                                                    (nuint)(2 * sizeof(nint)), 0, 0))
                throw Kernel32.LastError("UpdateProcThreadAttribute");

            var startup = new Kernel32.STARTUPINFOEXW();
            startup.StartupInfo.cb = sizeof(Kernel32.STARTUPINFOEXW);
            startup.StartupInfo.dwFlags = Kernel32.STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = handles[0];
            startup.StartupInfo.hStdOutput = handles[1];
            startup.StartupInfo.hStdError = handles[1];
            startup.lpAttributeList = list;

            var commandLine = (CommandLine.Build(executable, arguments) + "\0").ToCharArray();
            var block = environment is null ? null : CommandLine.EnvironmentBlock(CurrentEnvironment(), environment);
            var flags = Kernel32.CREATE_SUSPENDED | Kernel32.CREATE_NO_WINDOW | Kernel32.EXTENDED_STARTUPINFO_PRESENT
                        | Kernel32.CREATE_UNICODE_ENVIRONMENT;
            var info = new Kernel32.PROCESS_INFORMATION();
            bool created;
            fixed (char* app = executable)
            fixed (char* line = commandLine)
            fixed (char* env = block)
            {
                created = Kernel32.CreateProcess(app, line, 0, 0, true, flags, env, null, &startup, &info);
            }
            if (!created) throw new IOException(Kernel32.LastError("CreateProcessW").Message);
            GC.KeepAlive(input);
            GC.KeepAlive(output);
            return info;
        }
        finally
        {
            if (initialised) Kernel32.DeleteProcThreadAttributeList(list);
            Marshal.FreeHGlobal(list);
        }
    }

    static Dictionary<string, string> CurrentEnvironment()
    {
        var current = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (System.Collections.DictionaryEntry entry in System.Environment.GetEnvironmentVariables())
        {
            if (entry.Key is string key && entry.Value is string value) current[key] = value;
        }
        return current;
    }

    /// <summary>A service process and its own job.</summary>
    sealed class JobProcess : IServiceProcess
    {
        readonly Win32Handle process;
        readonly JobObject job;
        readonly TaskCompletionSource<int> exited = new(TaskCreationOptions.RunContinuationsAsynchronously);
        readonly RegisteredWaitHandle registration;
        readonly ProcessWaitHandle wait;
        int disposed;

        public JobProcess(int pid, Win32Handle process, JobObject job)
        {
            Pid = pid;
            this.process = process;
            this.job = job;
            wait = new ProcessWaitHandle(process);
            registration = ThreadPool.RegisterWaitForSingleObject(wait, (_, _) => OnExit(), null, Timeout.Infinite,
                                                                  executeOnlyOnce: true);
        }

        public int Pid { get; }

        public Task<int> Exited => exited.Task;

        void OnExit()
        {
            uint code = 0;
            var ok = false;
            var add = false;
            try
            {
                process.DangerousAddRef(ref add);
                ok = Kernel32.GetExitCodeProcess(process.DangerousGetHandle(), &code);
            }
            catch (ObjectDisposedException)
            {
            }
            finally
            {
                if (add) process.DangerousRelease();
            }
            exited.TrySetResult(ok ? unchecked((int)code) : -1);
        }

        public void Kill()
        {
            if (Volatile.Read(ref disposed) != 0) return;
            // The service's own job: it and everything it started, nothing else.
            job.Terminate();
        }

        public int ActiveProcesses => Volatile.Read(ref disposed) != 0 ? 0 : job.ActiveProcesses;

        /// <summary>Closes the handles; closing the service's job also ends anything still in it.</summary>
        public void Dispose()
        {
            if (Interlocked.Exchange(ref disposed, 1) != 0) return;
            registration.Unregister(null);
            job.Dispose();
            if (!exited.Task.IsCompleted)
            {
                // Ended by closing the job: the exit is not waited for any more.
                exited.TrySetResult(-1);
            }
            wait.Dispose();
            process.Dispose();
        }
    }

    /// <summary>Waits on a process handle the caller keeps (the wait handle does not own it).</summary>
    sealed class ProcessWaitHandle : WaitHandle
    {
        public ProcessWaitHandle(SafeHandle process) =>
            SafeWaitHandle = new SafeWaitHandle(process.DangerousGetHandle(), ownsHandle: false);
    }
}
