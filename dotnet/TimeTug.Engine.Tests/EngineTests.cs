using System.Text.Json.Nodes;
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

        // The first state may be published before the account kinds are registered; wait for them.
        var state = await Wait.WithTimeout(FirstWhere(engine.State, s => s.AvailableAccountKinds.Count > 0));
        Assert.Contains(state.AvailableAccountKinds, kind => kind.Id == "google" && !kind.IsConfigured);
        Assert.Equal(60, state.Settings.Takeover.LeadTime);
    }

    /// <summary>The release workflow stamps the tag into the library and sets TIMETUG_EXPECTED_ENGINE_VERSION: the
    /// packaged engine must report it in <c>ready</c>. Without the variable (CI and local runs) any version passes.</summary>
    [Fact]
    public async Task ReadyReportsTheStampedEngineVersion()
    {
        using var directory = new TemporaryDirectory();
        await using var engine = EngineFactory.Create(Options(directory.Path, new QuietHost()));
        await engine.SendAsync(new EngineCommand.Start());
        var ready = Assert.IsType<EngineEvent.Ready>(await Wait.WithTimeout(First(engine.Events)));
        var expected = Environment.GetEnvironmentVariable("TIMETUG_EXPECTED_ENGINE_VERSION");
        if (!string.IsNullOrEmpty(expected)) Assert.Equal(expected, ready.Info.EngineVersion);
        else Assert.NotEmpty(ready.Info.EngineVersion);
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

    // MARK: - External tokens

    private static EngineOptions TokenOptions(string directory, IEngineHost host) => new()
    {
        DataDirectory = directory,
        Platform = "windows",
        TimeZone = "Europe/Berlin",
        Testing = true,
        TestClockStart = ClockStart,
        OAuthClients = new Dictionary<string, OAuthClient> { ["microsoft"] = new("ms-client") },
        Host = host,
    };

    private static JsonNode? FixturePayload(string type) =>
        Conformance.Load(Path.Combine(Conformance.FixtureDirectory, type + ".json"))["payload"];

    private static string ErrorKind(string reply) => Json.String(JsonNode.Parse(reply)!["error"]!["kind"])!;

    [Fact]
    public async Task HostWithoutTheTokenInterfaceSendsNoFlagAndAnswersNotFound()
    {
        using var directory = new TemporaryDirectory();
        await using var engine = EngineFactory.Create(Options(directory.Path, new QuietHost()));
        await engine.SendAsync(new EngineCommand.Start());
        var ready = Assert.IsType<EngineEvent.Ready>(await Wait.WithTimeout(First(engine.Events)));
        Assert.False(ready.Info.Capabilities.ExternalTokens);
        var state = await Wait.WithTimeout(FirstWhere(engine.State, s => s.AvailableAccountKinds.Count > 0));
        Assert.DoesNotContain(state.AvailableAccountKinds, kind => kind.Id == "microsoft-brokered");

        using var nativeDirectory = new TemporaryDirectory();
        await using var native = NativeEngine.Create(Options(nativeDirectory.Path, new QuietHost()), null);
        foreach (var type in new[] { "service.token.get", "service.token.signIn", "service.token.cancelSignIn" })
        {
            Assert.Equal("notFound", ErrorKind(await native.ReplyAsync(type, FixturePayload(type))));
        }
    }

    [Fact]
    public async Task HostWithTheInterfaceSetsTheFlagAndReadyCapabilityIsTrue()
    {
        using var directory = new TemporaryDirectory();
        await using var engine = EngineFactory.Create(TokenOptions(directory.Path, new TokenHost()));
        await engine.SendAsync(new EngineCommand.Start());
        var ready = Assert.IsType<EngineEvent.Ready>(await Wait.WithTimeout(First(engine.Events)));
        Assert.True(ready.Info.Capabilities.ExternalTokens);
        var state = await Wait.WithTimeout(FirstWhere(engine.State, s => s.AvailableAccountKinds.Count > 0));
        Assert.Contains(state.AvailableAccountKinds, kind => kind.Id == "microsoft-brokered" && kind.IsConfigured);
    }

    [Fact]
    public async Task GetTokenOkIsSerializedWithMillisecondUtcExpiry()
    {
        using var directory = new TemporaryDirectory();
        var host = new TokenHost { Token = ExternalTokenResult.Ok("tok-1", new DateTimeOffset(2026, 10, 9, 9, 53, 20, 123, TimeSpan.FromHours(2))) };
        await using var engine = NativeEngine.Create(TokenOptions(directory.Path, host), null);
        var reply = await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get"));
        Assert.Equal("""{"v":1,"result":{"status":"ok","accessToken":"tok-1","expiresAt":"2026-10-09T07:53:20.123Z"}}""", reply);
        var request = Assert.IsType<ExternalTokenRequest>(host.LastGet);
        Assert.Equal("1", request.ConnectionID);
        Assert.Equal("acct-1", request.AccountID);
        Assert.Equal("me@example.com", request.Email);
        Assert.Equal(["User.Read", "Calendars.Read"], request.Scopes);
        Assert.False(request.ForceRefresh);
    }

    [Fact]
    public async Task InteractionRequiredAndUnavailableAreSerialized()
    {
        using var directory = new TemporaryDirectory();
        var host = new TokenHost { Token = ExternalTokenResult.InteractionRequired("signedOut") };
        await using var engine = NativeEngine.Create(TokenOptions(directory.Path, host), null);
        Assert.Equal("""{"v":1,"result":{"status":"interactionRequired","reason":"signedOut"}}""",
                     await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get")));
        host.Token = ExternalTokenResult.Unavailable("brokerUnavailable");
        Assert.Equal("""{"v":1,"result":{"status":"unavailable","reason":"brokerUnavailable"}}""",
                     await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get")));
        host.SignIn = ExternalSignInResult.Blocked("adminConsentRequired");
        Assert.Equal("""{"v":1,"result":{"status":"blocked","reason":"adminConsentRequired"}}""",
                     await engine.ReplyAsync("service.token.signIn", FixturePayload("service.token.signIn")));
    }

    [Fact]
    public async Task SignInReceivesTheScopesAndTheOptionalAccount()
    {
        using var directory = new TemporaryDirectory();
        var expires = new DateTimeOffset(2026, 10, 9, 9, 53, 20, TimeSpan.Zero);
        var host = new TokenHost { SignIn = ExternalSignInResult.Ok("acct-2", "tok-2", expires) };
        await using var engine = NativeEngine.Create(TokenOptions(directory.Path, host), null);

        var first = await engine.ReplyAsync("service.token.signIn", FixturePayload("service.token.signIn"));
        Assert.Equal("""{"v":1,"result":{"status":"ok","accountID":"acct-2","accessToken":"tok-2","expiresAt":"2026-10-09T09:53:20.000Z"}}""", first);
        var request = Assert.IsType<ExternalSignInRequest>(host.LastSignIn);
        Assert.Equal("signin-1", request.SignInID);
        Assert.Equal(["User.Read", "Calendars.Read"], request.Scopes);
        Assert.Null(request.AccountID);
        Assert.Null(request.Email);

        await engine.ReplyAsync("service.token.signIn", JsonNode.Parse(
            """{"signInID":"signin-2","scopes":["User.Read"],"accountID":"acct-1","email":"me@example.com"}"""));
        request = Assert.IsType<ExternalSignInRequest>(host.LastSignIn);
        Assert.Equal("signin-2", request.SignInID);
        Assert.Equal("acct-1", request.AccountID);
        Assert.Equal("me@example.com", request.Email);
    }

    [Fact]
    public async Task CancelSignInIsCalledOnAccountCancelWithTheSameId()
    {
        using var directory = new TemporaryDirectory();
        var host = new TokenHost { WaitForCancellation = true };
        await using var engine = EngineFactory.Create(TokenOptions(directory.Path, host));
        await engine.SendAsync(new EngineCommand.Start());
        await Wait.WithTimeout(FirstWhere(engine.State, s => s.AvailableAccountKinds.Any(kind => kind.Id == "microsoft-brokered")));
        await engine.SendAsync(new EngineCommand.AddAccount("microsoft-brokered"));
        var started = await Wait.WithTimeout(host.SignInStarted.Task);
        Assert.False(string.IsNullOrEmpty(started.SignInID));
        Assert.Equal(["User.Read", "MailboxSettings.Read", "Calendars.Read", "Calendars.Read.Shared"], started.Scopes);

        await engine.SendAsync(new EngineCommand.CancelAuthorization());
        Assert.Equal(started.SignInID, await Wait.WithTimeout(host.Cancelled.Task));
        // The sign-in's own token was cancelled as well, so a host that only watches the token also stops.
        Assert.True(await Wait.WithTimeout(host.TokenCancelled.Task));
    }

    [Fact]
    public async Task HostExceptionMapsToFailedAndCancellationToCancelled()
    {
        using var directory = new TemporaryDirectory();
        var host = new TokenHost { Throw = new InvalidOperationException("boom SENTINEL-TOKEN-9 me@example.com") };
        await using var engine = NativeEngine.Create(TokenOptions(directory.Path, host), null);
        var failed = await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get"));
        Assert.Equal("failed", ErrorKind(failed));
        Assert.DoesNotContain("SENTINEL-TOKEN-9", failed, StringComparison.Ordinal);
        Assert.DoesNotContain("me@example.com", failed, StringComparison.Ordinal);

        host.Throw = new OperationCanceledException();
        Assert.Equal("cancelled", ErrorKind(await engine.ReplyAsync("service.token.signIn", FixturePayload("service.token.signIn"))));

        host.Throw = new EngineServiceException(ServiceErrorKind.Network, "offline SENTINEL-TOKEN-9");
        var network = await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get"));
        Assert.Equal("network", ErrorKind(network));
        Assert.DoesNotContain("SENTINEL-TOKEN-9", network, StringComparison.Ordinal);

        host.Throw = new System.Net.Http.HttpRequestException("dns SENTINEL-TOKEN-9");
        var http = await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get"));
        Assert.Equal("network", ErrorKind(http));
        Assert.DoesNotContain("SENTINEL-TOKEN-9", http, StringComparison.Ordinal);
    }

    [Fact]
    public async Task HostExceptionMessageWithTheTokenIsNotEchoedInADiagnostic()
    {
        using var directory = new TemporaryDirectory();
        var host = new TokenHost { Throw = new InvalidOperationException("boom SENTINEL-TOKEN-9") };
        await using var engine = NativeEngine.Create(TokenOptions(directory.Path, host), null);
        await engine.ReplyAsync("service.token.get", FixturePayload("service.token.get"));
        await engine.ReplyAsync("service.token.signIn", FixturePayload("service.token.signIn"));
        Assert.DoesNotContain(host.Diagnostics, diagnostic =>
            diagnostic.Message.Contains("SENTINEL-TOKEN-9", StringComparison.Ordinal)
            || diagnostic.Category.Contains("SENTINEL-TOKEN-9", StringComparison.Ordinal));
    }

    [Fact]
    public async Task CancelSignInThatThrowsDoesNotFailTheRequest()
    {
        using var directory = new TemporaryDirectory();
        var host = new TokenHost { CancelThrows = true };
        await using var engine = NativeEngine.Create(TokenOptions(directory.Path, host), null);
        var reply = await engine.ReplyAsync("service.token.cancelSignIn", FixturePayload("service.token.cancelSignIn"));
        Assert.Equal("""{"v":1,"result":{}}""", reply);
        Assert.Equal("signin-1", host.CancelledIds.Single());
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
internal class QuietHost : IEngineHost, ICredentialStore, IHttpTransport, IAuthorizationInteraction
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

/// <summary>A host that also supplies external tokens, scripted per test.</summary>
internal sealed class TokenHost : QuietHost, IExternalTokenHost
{
    private readonly List<string> _cancelled = [];

    public ExternalTokenResult Token { get; set; } = ExternalTokenResult.Unavailable();
    public ExternalSignInResult SignIn { get; set; } = ExternalSignInResult.Unavailable();
    public Exception? Throw { get; set; }
    public bool WaitForCancellation { get; set; }
    public bool CancelThrows { get; set; }
    public ExternalTokenRequest? LastGet { get; private set; }
    public ExternalSignInRequest? LastSignIn { get; private set; }
    public TaskCompletionSource<ExternalSignInRequest> SignInStarted { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public TaskCompletionSource<string> Cancelled { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public TaskCompletionSource<bool> TokenCancelled { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public IReadOnlyList<string> CancelledIds { get { lock (_cancelled) return _cancelled.ToList(); } }

    public ValueTask<ExternalTokenResult> GetTokenAsync(ExternalTokenRequest request, CancellationToken cancellationToken)
    {
        LastGet = request;
        if (Throw is { } failure) throw failure;
        return ValueTask.FromResult(Token);
    }

    public async ValueTask<ExternalSignInResult> SignInAsync(ExternalSignInRequest request, CancellationToken cancellationToken)
    {
        LastSignIn = request;
        SignInStarted.TrySetResult(request);
        if (WaitForCancellation)
        {
            try { await Task.Delay(Timeout.Infinite, cancellationToken).ConfigureAwait(false); }
            catch (OperationCanceledException)
            {
                TokenCancelled.TrySetResult(true);
                throw;
            }
        }
        if (Throw is { } failure) throw failure;
        return SignIn;
    }

    public void CancelSignIn(string signInID)
    {
        lock (_cancelled) _cancelled.Add(signInID);
        Cancelled.TrySetResult(signInID);
        if (CancelThrows) throw new InvalidOperationException("the dialog is gone");
    }
}
