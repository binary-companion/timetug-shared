using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using TimeTug.Engine;

namespace TimeTug.Engine.Bench;

/// <summary>
/// Usage: <c>TimeTug.Engine.Bench --idle-seconds &lt;n&gt; --out &lt;file&gt;</c>.
/// Creates an engine in a fresh temporary data directory (host platform, not testing, no connectors), sends
/// <c>start</c>, waits for <c>ready</c>, idles for n seconds and writes one JSON object of measurements.
/// </summary>
internal static class Program
{
    private static async Task<int> Main(string[] args)
    {
        if (!TryParse(args, out var idleSeconds, out var outFile))
        {
            Console.Error.WriteLine("usage: TimeTug.Engine.Bench --idle-seconds <n> --out <file>");
            return 2;
        }

        var directory = Path.Combine(Path.GetTempPath(), "tt-engine-bench-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        try
        {
            var result = await MeasureAsync(directory, idleSeconds);
            Write(outFile, result);
            Console.WriteLine(File.ReadAllText(outFile));
            return 0;
        }
        catch (Exception exception) when (exception is not OutOfMemoryException)
        {
            Console.Error.WriteLine(exception);
            return 1;
        }
        finally
        {
            try { Directory.Delete(directory, recursive: true); }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }

    private static bool TryParse(string[] args, out int idleSeconds, out string outFile)
    {
        idleSeconds = -1;
        outFile = "";
        for (var i = 0; i < args.Length; i++)
        {
            if (args[i] == "--idle-seconds" && i + 1 < args.Length && int.TryParse(args[i + 1], out idleSeconds)) i++;
            else if (args[i] == "--out" && i + 1 < args.Length) outFile = args[++i];
            else return false;
        }
        return idleSeconds >= 0 && outFile.Length > 0;
    }

    private static async Task<Measurement> MeasureAsync(string dataDirectory, int idleSeconds)
    {
        var host = new FailingHost();
        var options = new EngineOptions
        {
            DataDirectory = dataDirectory,
            Platform = OperatingSystem.IsWindows() ? "windows" : OperatingSystem.IsMacOS() ? "macos" : "linux",
            Testing = false,
            Host = host,
        };

        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(120));
        var clock = Stopwatch.StartNew();
        var engine = EngineFactory.Create(options);
        await using (engine)
        {
            var ready = new TaskCompletionSource<ReadyInfo>(TaskCreationOptions.RunContinuationsAsynchronously);
            var reader = Task.Run(async () =>
            {
                await foreach (var item in engine.Events.WithCancellation(timeout.Token))
                {
                    if (item is EngineEvent.Ready r)
                    {
                        clock.Stop();
                        ready.TrySetResult(r.Info);
                    }
                }
            });
            await engine.SendAsync(new EngineCommand.Start());
            var info = await ready.Task.WaitAsync(timeout.Token);
            var readyMs = clock.Elapsed.TotalMilliseconds;

            using var process = Process.GetCurrentProcess();
            process.Refresh();
            var cpuStart = process.TotalProcessorTime;
            await Task.Delay(TimeSpan.FromSeconds(idleSeconds));
            process.Refresh();
            var cpuEnd = process.TotalProcessorTime;

            var measurement = new Measurement(
                Engine: info.EngineVersion.Length > 0 ? info.EngineVersion : "unknown",
                Rid: RuntimeInformation.RuntimeIdentifier,
                ReadyMs: Math.Round(readyMs, 1),
                WorkingSetBytes: process.WorkingSet64,
                PrivateBytes: PrivateBytes(process),
                CpuMsIdle: Math.Round((cpuEnd - cpuStart).TotalMilliseconds, 1),
                IdleSeconds: idleSeconds,
                NativeModuleBytes: NativeModuleBytes(process));
            // The engine is disposed on leaving the block; the event stream then ends and the reader finishes.
            _ = reader;
            return measurement;
        }
    }

    /// <summary>
    /// Private memory: <c>PrivateMemorySize64</c> on Windows and Linux. On macOS that property is not available, so the
    /// process's physical footprint (<c>ri_phys_footprint</c> of <c>proc_pid_rusage</c>, the figure Activity Monitor
    /// calls Memory) is used.
    /// </summary>
    private static long PrivateBytes(Process process)
    {
        if (!OperatingSystem.IsMacOS()) return process.PrivateMemorySize64;
        var buffer = new byte[512];
        if (proc_pid_rusage(Environment.ProcessId, 0, buffer) != 0) return -1;
        return (long)BitConverter.ToUInt64(buffer, 72); // rusage_info_v0: ri_phys_footprint
    }

    [DllImport("libproc")]
    private static extern int proc_pid_rusage(int pid, int flavor, byte[] buffer);

    /// <summary>
    /// The sum of the sizes of the native modules loaded from the application directory (.dll, .dylib, .so; managed
    /// assemblies excluded). Where the platform cannot list a process's modules (macOS), the native files of the
    /// application directory are summed instead: the engine library and what ships beside it.
    /// </summary>
    private static long NativeModuleBytes(Process process)
    {
        var appDirectory = Path.GetFullPath(AppContext.BaseDirectory);
        var paths = new List<string>();
        try
        {
            paths.AddRange(process.Modules.Cast<ProcessModule>().Select(module => module.FileName));
        }
        catch (Exception exception) when (exception is PlatformNotSupportedException or InvalidOperationException
                                              or System.ComponentModel.Win32Exception or NotSupportedException)
        {
            // Handled below: no modules listed.
        }
        if (paths.Count == 0)
        {
            Console.Error.WriteLine("nativeModuleBytes: native files of the application directory (modules not listable)");
            paths.AddRange(Directory.EnumerateFiles(appDirectory, "*", SearchOption.AllDirectories));
        }
        else
        {
            Console.Error.WriteLine("nativeModuleBytes: loaded modules");
        }

        long total = 0;
        foreach (var path in paths.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            var extension = Path.GetExtension(path).ToLowerInvariant();
            if (extension is not (".dll" or ".dylib" or ".so")) continue;
            var full = Path.GetFullPath(path);
            if (!full.StartsWith(appDirectory, StringComparison.OrdinalIgnoreCase)) continue;
            if (!File.Exists(full) || IsManaged(full)) continue;
            total += new FileInfo(full).Length;
        }
        return total;
    }

    private static bool IsManaged(string path)
    {
        try
        {
            AssemblyName.GetAssemblyName(path);
            return true;
        }
        catch (Exception exception) when (exception is BadImageFormatException or FileLoadException)
        {
            return false; // a native image (the runtime reports it as one of these two)
        }
    }

    private static void Write(string path, Measurement m)
    {
        using var stream = new MemoryStream();
        using (var writer = new Utf8JsonWriter(stream, new JsonWriterOptions { Indented = true }))
        {
            writer.WriteStartObject();
            writer.WriteString("engine", m.Engine);
            writer.WriteString("rid", m.Rid);
            writer.WriteNumber("readyMs", m.ReadyMs);
            writer.WriteNumber("workingSetBytes", m.WorkingSetBytes);
            writer.WriteNumber("privateBytes", m.PrivateBytes);
            writer.WriteNumber("cpuMsIdle", m.CpuMsIdle);
            writer.WriteNumber("idleSeconds", m.IdleSeconds);
            writer.WriteNumber("nativeModuleBytes", m.NativeModuleBytes);
            writer.WriteEndObject();
        }
        File.WriteAllText(path, Encoding.UTF8.GetString(stream.ToArray()) + "\n");
    }

    private sealed record Measurement(string Engine, string Rid, double ReadyMs, long WorkingSetBytes, long PrivateBytes,
                                      double CpuMsIdle, int IdleSeconds, long NativeModuleBytes);
}

/// <summary>A host with no accounts: it answers every service request with the error kind <c>failed</c>.</summary>
internal sealed class FailingHost : IEngineHost, ICredentialStore, IHttpTransport, IAuthorizationInteraction
{
    public ICredentialStore Credentials => this;
    public IHttpTransport Http => this;
    public IAuthorizationInteraction Authorization => this;
    public ITextGenerator? TextGenerator => null;

    public void Diagnostic(EngineDiagnostic diagnostic) { }

    private static EngineServiceException Failed => new(ServiceErrorKind.Failed, "benchmark host offers no services");

    public ValueTask<IReadOnlyDictionary<string, string>?> GetAsync(string connectionID, CancellationToken cancellationToken) => throw Failed;
    public ValueTask SetAsync(string connectionID, IReadOnlyDictionary<string, string> secrets, CancellationToken cancellationToken) => throw Failed;
    public ValueTask RemoveAsync(string connectionID, CancellationToken cancellationToken) => throw Failed;
    public ValueTask<CredentialSnapshot?> SnapshotAsync(string connectionID, CancellationToken cancellationToken) => throw Failed;
    public ValueTask<bool> UpdateRefreshTokenAsync(string connectionID, string token, string expectedRevision, CancellationToken cancellationToken) => throw Failed;
    public ValueTask<EngineHttpResponse> SendAsync(EngineHttpRequest request, CancellationToken cancellationToken) => throw Failed;
    public ValueTask<RedirectSession> BeginRedirectAsync(CancellationToken cancellationToken) => throw Failed;
    public ValueTask<string> AuthorizeAsync(string sessionID, string url, CancellationToken cancellationToken) => throw Failed;
    public ValueTask CloseRedirectAsync(string sessionID, CancellationToken cancellationToken) => throw Failed;
    public ValueTask<IReadOnlyDictionary<string, string>> PromptCredentialsAsync(IReadOnlyList<CredentialField> fields, CancellationToken cancellationToken) => throw Failed;
    public ValueTask PrepareSignInAsync(SignInForm form, CancellationToken cancellationToken) => throw Failed;
    public ValueTask<IReadOnlyDictionary<string, string>?> LastNonSecretValuesAsync(CancellationToken cancellationToken) => throw Failed;
}
