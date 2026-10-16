using Xunit;

// The callbacks below block on purpose: they run on the engine's delivery thread, which is what is under test.
#pragma warning disable xUnit1031

namespace TimeTug.Engine.Tests;

/// <summary>The wrapper's own behaviour through its public API: creation, the two streams, typed commands, the
/// dispatcher, and disposal (idempotent, and safe from inside a host callback).</summary>
public class EngineTests
{
    private static readonly DateTimeOffset ClockStart = new(2026, 10, 16, 13, 0, 0, TimeSpan.Zero);

    private static EngineOptions Options(string directory, IEngineHost host, Action<Action>? dispatcher = null) => new()
    {
        DataDirectory = directory,
        Platform = "windows",
        TimeZone = "Europe/Berlin",
        Testing = true,
        TestClockStart = ClockStart,
        Dispatcher = dispatcher,
        Host = host,
    };

    [Fact]
    public void CreateRejectsAnInvalidConfiguration()
    {
        using var directory = new TemporaryDirectory();
        var host = new QuietHost();
        Assert.Throws<EngineConfigurationException>(() => EngineFactory.Create(Options("", host)));
        Assert.Throws<EngineConfigurationException>(() => EngineFactory.Create(new EngineOptions
        {
            DataDirectory = directory.Path, Platform = "amiga", Host = host,
        }));
        Assert.Throws<EngineConfigurationException>(() => EngineFactory.Create(new EngineOptions
        {
            DataDirectory = directory.Path, TimeZone = "Mars/Olympus", Host = host,
        }));
    }

    [Fact]
    public async Task StartDeliversReadyAndAState()
    {
        using var directory = new TemporaryDirectory();
        await using var engine = EngineFactory.Create(Options(directory.Path, new QuietHost()));
        await engine.SendAsync(new EngineCommand.Start());

        var ready = Assert.IsType<EngineEvent.Ready>(await Wait.WithTimeout(First(engine.Events)));
        Assert.Equal([1], ready.Info.ProtocolVersions);
        Assert.Equal("windows", ready.Info.Platform);
        Assert.False(ready.Info.Capabilities.SystemCalendar);
        Assert.False(ready.Info.Capabilities.TextGeneration);

        var state = await Wait.WithTimeout(First(engine.State));
        Assert.Contains(state.AvailableAccountKinds, kind => kind.Id == "google" && !kind.IsConfigured);
        Assert.Equal(60, state.Settings.Takeover.LeadTime);
    }

    [Fact]
    public async Task TypedCommandsReachTheEngine()
    {
        using var directory = new TemporaryDirectory();
        await using var engine = EngineFactory.Create(Options(directory.Path, new QuietHost()));
        await engine.SendAsync(new EngineCommand.Start());
        await engine.SendAsync(new EngineCommand.ApplySettings { LeadTime = 300, HiddenCalendarKeys = ["x/y"] });
        var state = await Wait.WithTimeout(FirstWhere(engine.State, s => s.Settings.Takeover.LeadTime == 300));
        Assert.Equal(["x/y"], state.Settings.Takeover.HiddenCalendarKeys);
    }

    [Fact]
    public async Task ADiagnosticCarriesTheCommandIdOnBothChannels()
    {
        using var directory = new TemporaryDirectory();
        var host = new QuietHost();
        await using var engine = EngineFactory.Create(Options(directory.Path, host));
        await engine.SendAsync(new EngineCommand.Start());
        await engine.SendAsync(new EngineCommand.Merge(["gone-1", "gone-2"]), id: "merge-1");
        var diagnostic = Assert.IsType<EngineEvent.Diagnostic>(await Wait.WithTimeout(
            FirstWhere(engine.Events, e => e is EngineEvent.Diagnostic { Info.CommandId: "merge-1" })));
        Assert.Equal("unknown event id", diagnostic.Info.Message);
        Assert.True(await Wait.UntilAsync(() => host.Diagnostics.Any(d => d.CommandId == "merge-1"), TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public async Task TheDispatcherCarriesHostCalls()
    {
        using var directory = new TemporaryDirectory();
        var host = new QuietHost();
        var dispatched = 0;
        await using var engine = EngineFactory.Create(Options(directory.Path, host, action =>
        {
            Interlocked.Increment(ref dispatched);
            ThreadPool.QueueUserWorkItem(_ => action());
        }));
        await engine.SendAsync(new EngineCommand.Start());
        await engine.SendAsync(new EngineCommand.Unmerge("gone"), id: "u");
        Assert.True(await Wait.UntilAsync(() => host.Diagnostics.Any(d => d.CommandId == "u"), TimeSpan.FromSeconds(5)));
        Assert.True(Volatile.Read(ref dispatched) > 0);
    }

    [Fact]
    public async Task DisposeIsIdempotentAndEndsTheStreams()
    {
        using var directory = new TemporaryDirectory();
        var engine = EngineFactory.Create(Options(directory.Path, new QuietHost()));
        await engine.SendAsync(new EngineCommand.Start());
        await Wait.WithTimeout(First(engine.State));
        var events = Drain(engine.Events);
        var states = Drain(engine.State);

        await Wait.WithTimeout(Task.WhenAll(engine.DisposeAsync().AsTask(), engine.DisposeAsync().AsTask()));
        await Wait.WithTimeout(engine.DisposeAsync().AsTask());
        Assert.Contains(await Wait.WithTimeout(events), e => e is EngineEvent.Ready);
        await Wait.WithTimeout(states);
        await Assert.ThrowsAsync<ObjectDisposedException>(async () => await engine.SendAsync(new EngineCommand.Refresh()));
    }

    /// <summary>A host callback (here the diagnostic sink, on the delivery thread) that disposes the engine and blocks
    /// until disposal completes must not wait for its own delivery.</summary>
    [Fact]
    public async Task DisposeFromInsideACallbackDoesNotDeadlock()
    {
        using var directory = new TemporaryDirectory();
        var host = new QuietHost();
        IEngine? engine = null;
        var disposedInside = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        host.OnDiagnostic = diagnostic =>
        {
            if (diagnostic.CommandId != "boom") return;
            var completed = engine!.DisposeAsync().AsTask().Wait(TimeSpan.FromSeconds(5));
            var again = engine.DisposeAsync().AsTask().Wait(TimeSpan.FromSeconds(5));
            disposedInside.TrySetResult(completed && again);
        };
        engine = EngineFactory.Create(Options(directory.Path, host));
        var events = Drain(engine.Events);
        await engine.SendAsync(new EngineCommand.Start());
        await engine.SendAsync(new EngineCommand.Unmerge("gone"), id: "boom");

        Assert.True(await Wait.WithTimeout(disposedInside.Task));
        await Wait.WithTimeout(engine.DisposeAsync().AsTask());
        await Wait.WithTimeout(events);
        await Assert.ThrowsAsync<ObjectDisposedException>(async () => await engine.SendAsync(new EngineCommand.Refresh()));
    }

    /// <summary>An outside disposal waits for a running callback; that callback disposing too must not wait for the
    /// outside one (which waits for it).</summary>
    [Fact]
    public async Task DisposeInsideACallbackWhileAnOutsideDisposeWaits()
    {
        using var directory = new TemporaryDirectory();
        var host = new QuietHost();
        IEngine? engine = null;
        var inside = new ManualResetEventSlim();
        var release = new ManualResetEventSlim();
        var innerCompleted = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        host.OnDiagnostic = diagnostic =>
        {
            if (diagnostic.CommandId != "hold") return;
            inside.Set();
            release.Wait(TimeSpan.FromSeconds(5));
            innerCompleted.TrySetResult(engine!.DisposeAsync().AsTask().Wait(TimeSpan.FromSeconds(5)));
        };
        engine = EngineFactory.Create(Options(directory.Path, host));
        await engine.SendAsync(new EngineCommand.Start());
        await engine.SendAsync(new EngineCommand.Unmerge("gone"), id: "hold");
        Assert.True(await Task.Run(() => inside.Wait(TimeSpan.FromSeconds(10))));
        var outside = engine.DisposeAsync().AsTask();
        release.Set();
        Assert.True(await Wait.WithTimeout(innerCompleted.Task));
        await Wait.WithTimeout(outside);
    }

    [Fact]
    public async Task SeveralEnginesRunInOneProcess()
    {
        var directories = Enumerable.Range(0, 3).Select(_ => new TemporaryDirectory()).ToList();
        try
        {
            var engines = directories.Select(d => EngineFactory.Create(Options(d.Path, new QuietHost()))).ToList();
            foreach (var engine in engines) await engine.SendAsync(new EngineCommand.Start());
            foreach (var engine in engines)
            {
                Assert.IsType<EngineEvent.Ready>(await Wait.WithTimeout(First(engine.Events)));
            }
            await Wait.WithTimeout(Task.WhenAll(engines.Select(e => e.DisposeAsync().AsTask())));
        }
        finally
        {
            foreach (var directory in directories) directory.Dispose();
        }
    }

    [Fact]
    public async Task AThrowingDiagnosticSinkDoesNotStopDelivery()
    {
        using var directory = new TemporaryDirectory();
        var host = new QuietHost { OnDiagnostic = _ => throw new InvalidOperationException("sink failed") };
        await using var engine = EngineFactory.Create(Options(directory.Path, host));
        await engine.SendAsync(new EngineCommand.Start());
        await engine.SendAsync(new EngineCommand.Unmerge("gone"), id: "first");
        await engine.SendAsync(new EngineCommand.Unmerge("gone"), id: "second");
        await Wait.WithTimeout(FirstWhere(engine.Events, e => e is EngineEvent.Diagnostic { Info.CommandId: "second" }));
    }

    private static async Task<T> First<T>(IAsyncEnumerable<T> stream) => await FirstWhere(stream, _ => true);

    private static async Task<T> FirstWhere<T>(IAsyncEnumerable<T> stream, Func<T, bool> predicate)
    {
        await foreach (var item in stream)
        {
            if (predicate(item)) return item;
        }
        throw new InvalidOperationException("the stream ended");
    }

    private static Task<List<T>> Drain<T>(IAsyncEnumerable<T> stream) => Task.Run(async () =>
    {
        var items = new List<T>();
        await foreach (var item in stream) items.Add(item);
        return items;
    });
}

/// <summary>A host with no accounts: every service fails with notFound; diagnostics are recorded.</summary>
internal sealed class QuietHost : IEngineHost, ICredentialStore, IHttpTransport, IAuthorizationInteraction
{
    private readonly List<EngineDiagnostic> _diagnostics = [];

    public Action<EngineDiagnostic>? OnDiagnostic { get; set; }

    public IReadOnlyList<EngineDiagnostic> Diagnostics { get { lock (_diagnostics) return _diagnostics.ToList(); } }

    public ICredentialStore Credentials => this;
    public IHttpTransport Http => this;
    public IAuthorizationInteraction Authorization => this;
    public ITextGenerator? TextGenerator => null;

    public void Diagnostic(EngineDiagnostic diagnostic)
    {
        lock (_diagnostics) _diagnostics.Add(diagnostic);
        OnDiagnostic?.Invoke(diagnostic);
    }

    private static EngineServiceException None => new(ServiceErrorKind.NotFound, "quiet host");

    public ValueTask<IReadOnlyDictionary<string, string>?> GetAsync(string connectionID, CancellationToken cancellationToken) => throw None;
    public ValueTask SetAsync(string connectionID, IReadOnlyDictionary<string, string> secrets, CancellationToken cancellationToken) => throw None;
    public ValueTask RemoveAsync(string connectionID, CancellationToken cancellationToken) => throw None;
    public ValueTask<CredentialSnapshot?> SnapshotAsync(string connectionID, CancellationToken cancellationToken) => throw None;
    public ValueTask<bool> UpdateRefreshTokenAsync(string connectionID, string token, string expectedRevision, CancellationToken cancellationToken) => throw None;
    public ValueTask<EngineHttpResponse> SendAsync(EngineHttpRequest request, CancellationToken cancellationToken) => throw None;
    public ValueTask<RedirectSession> BeginRedirectAsync(CancellationToken cancellationToken) => throw None;
    public ValueTask<string> AuthorizeAsync(string sessionID, string url, CancellationToken cancellationToken) => throw None;
    public ValueTask CloseRedirectAsync(string sessionID, CancellationToken cancellationToken) => throw None;
    public ValueTask<IReadOnlyDictionary<string, string>> PromptCredentialsAsync(IReadOnlyList<CredentialField> fields, CancellationToken cancellationToken) => throw None;
    public ValueTask PrepareSignInAsync(SignInForm form, CancellationToken cancellationToken) => throw None;
    public ValueTask<IReadOnlyDictionary<string, string>?> LastNonSecretValuesAsync(CancellationToken cancellationToken) => throw None;
}
