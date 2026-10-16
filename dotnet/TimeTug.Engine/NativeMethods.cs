using System;
using System.IO;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;

namespace TimeTug.Engine;

/// <summary>
/// The four functions of <c>include/timetug_engine.h</c>. Strings are UTF-8; the engine copies what it keeps before
/// <c>send</c> and <c>reply</c> return.
///
/// The library is <c>TimeTugEngineC</c> (<c>TimeTugEngineC.dll</c>, <c>libTimeTugEngineC.dylib</c>,
/// <c>libTimeTugEngineC.so</c>). It is found, in order: through the environment variable
/// <c>TIMETUG_ENGINE_LIBRARY</c> (the library file, or the directory that holds it); in the application directory; under
/// <c>runtimes/&lt;rid&gt;/native</c> there; then by the runtime's default probing (which covers the
/// NuGet package's native assets in an application's deps.json). A library found by path loads its own dependencies
/// (the Swift runtime DLLs shipped beside it) from its own directory.
/// </summary>
internal static unsafe partial class NativeMethods
{
    internal const string LibraryName = "TimeTugEngineC";
    internal const string LibraryVariable = "TIMETUG_ENGINE_LIBRARY";

    [LibraryImport(LibraryName, EntryPoint = "tt_engine_create", StringMarshalling = StringMarshalling.Utf8)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial nint Create(string configJson, delegate* unmanaged[Cdecl]<nint, byte*, void> onMessage,
                                        nint context);

    [LibraryImport(LibraryName, EntryPoint = "tt_engine_send", StringMarshalling = StringMarshalling.Utf8)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void Send(nint engine, string commandJson);

    [LibraryImport(LibraryName, EntryPoint = "tt_engine_reply", StringMarshalling = StringMarshalling.Utf8)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void Reply(nint engine, ulong requestId, string replyJson);

    [LibraryImport(LibraryName, EntryPoint = "tt_engine_destroy")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void Destroy(nint engine);

    private static int s_resolverInstalled;

    /// <summary>Installs the resolver once per process; called before the first native call.</summary>
    internal static void EnsureResolver()
    {
        if (Interlocked.Exchange(ref s_resolverInstalled, 1) == 0)
        {
            NativeLibrary.SetDllImportResolver(typeof(NativeMethods).Assembly, Resolve);
        }
    }

    private static nint Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (libraryName != LibraryName) return 0;
        foreach (var candidate in Candidates())
        {
            if (File.Exists(candidate)
                && NativeLibrary.TryLoad(candidate, assembly, DllImportSearchPath.UseDllDirectoryForDependencies
                                                               | DllImportSearchPath.SafeDirectories, out var handle))
            {
                return handle;
            }
        }
        return 0; // The runtime's default probing.
    }

    private static string FileName =>
        OperatingSystem.IsWindows() ? "TimeTugEngineC.dll"
        : OperatingSystem.IsMacOS() ? "libTimeTugEngineC.dylib"
        : "libTimeTugEngineC.so";

    private static System.Collections.Generic.IEnumerable<string> Candidates()
    {
        var configured = Environment.GetEnvironmentVariable(LibraryVariable);
        if (!string.IsNullOrEmpty(configured))
        {
            yield return Directory.Exists(configured) ? Path.Combine(configured, FileName) : configured;
        }
        // The application directory: where a referenced package's assembly and, for a RID-specific build, its native
        // assets land. (Assembly.Location is empty in single-file apps, so it is not used.)
        var directory = AppContext.BaseDirectory;
        yield return Path.Combine(directory, FileName);
        yield return Path.Combine(directory, "runtimes", RuntimeInformation.RuntimeIdentifier, "native", FileName);
        var portable = PortableRid();
        if (portable is not null) yield return Path.Combine(directory, "runtimes", portable, "native", FileName);
    }

    private static string? PortableRid()
    {
        var os = OperatingSystem.IsWindows() ? "win" : OperatingSystem.IsMacOS() ? "osx" : OperatingSystem.IsLinux() ? "linux" : null;
        var arch = RuntimeInformation.ProcessArchitecture switch
        {
            Architecture.X64 => "x64",
            Architecture.Arm64 => "arm64",
            Architecture.X86 => "x86",
            _ => null,
        };
        return os is null || arch is null ? null : $"{os}-{arch}";
    }
}
