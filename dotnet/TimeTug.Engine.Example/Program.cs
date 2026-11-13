using System.Net;
using TimeTug.Engine;

namespace TimeTug.Engine.Example;

/// <summary>Starts the TimeTug engine, changes a setting, reads the state it publishes and shuts the engine down.</summary>
public static class EngineExample
{
    /// <summary>Runs the example against <paramref name="dataDirectory"/> and returns the lead time the engine reports
    /// after the settings change (120 seconds).</summary>
    public static async Task<double> RunAsync(string dataDirectory, TimeSpan timeout)
    {
        using var timeoutSource = new CancellationTokenSource(timeout);
        var cancellation = timeoutSource.Token;

        // 1. Configuration. The engine keeps its files in DataDirectory (one engine per directory). OAuth clients are
        //    your own registrations with Google and Microsoft; without one, that account kind is listed as not
        //    configured. Host supplies the services the engine asks for.
        var options = new EngineOptions
        {
            DataDirectory = dataDirectory,
            Platform = "windows",
            TimeZone = "Europe/Berlin", // omit to use the process time zone
            OAuthClients = new Dictionary<string, OAuthClient>
            {
                // ["google"] = new OAuthClient("<your client id>", "<your client secret>"),
                // ["microsoft"] = new OAuthClient("<your client id>"),
            },
            Host = new ExampleHost(),
        };

        // 2. Create, then start. Create throws EngineConfigurationException for a configuration the engine rejects.
        //    DisposeAsync (here through "await using") stops the engine; no callback runs after it completes.
        await using var engine = EngineFactory.Create(options);
        await engine.SendAsync(new EngineCommand.Start());

        // 3. The first event is Ready. Events arrive in order and are never dropped.
        await foreach (var engineEvent in engine.Events.WithCancellation(cancellation))
        {
            if (engineEvent is EngineEvent.Ready ready)
            {
                Console.WriteLine($"engine {ready.Info.EngineVersion} on {ready.Info.Platform}, protocol {string.Join(",", ready.Info.ProtocolVersions)}");
                break;
            }
        }

        // 4. States are the whole picture each time; a slow reader skips the intermediate ones (the newest wins).
        //    Wait for the one that lists the account kinds this platform offers.
        EngineState state = await FirstStateAsync(engine, s => s.AvailableAccountKinds.Count > 0, cancellation);
        foreach (var kind in state.AvailableAccountKinds)
        {
            Console.WriteLine($"account kind {kind.Id}: {kind.DisplayName} (configured: {kind.IsConfigured})");
        }

        // 5. Commands never block. A settings patch changes only the members you set. The id comes back on any
        //    diagnostic the command causes.
        await engine.SendAsync(new EngineCommand.ApplySettings { LeadTime = 120 }, id: "lead-time");
        state = await FirstStateAsync(engine, s => s.Settings.Takeover.LeadTime == 120, cancellation);
        Console.WriteLine($"lead time is now {state.Settings.Takeover.LeadTime} seconds");

        return state.Settings.Takeover.LeadTime;
    }

    private static async Task<EngineState> FirstStateAsync(IEngine engine, Func<EngineState, bool> wanted, CancellationToken cancellation)
    {
        await foreach (var state in engine.State.WithCancellation(cancellation))
        {
            if (wanted(state)) return state;
        }
        throw new InvalidOperationException("the engine stopped before the state arrived");
    }
}

/// <summary>The host services. This host keeps secrets in memory (a real one uses the Windows Credential Locker), sends
/// HTTP with <see cref="HttpClient"/>, and has no sign-in window, so it refuses sign-in requests.</summary>
internal sealed class ExampleHost : IEngineHost
{
    public ICredentialStore Credentials { get; } = new MemoryCredentials();
    public IHttpTransport Http { get; } = new HttpClientTransport();
    public IAuthorizationInteraction Authorization { get; } = new NoSignIn();
    public ITextGenerator? TextGenerator => null; // no on-device text model: the engine will not ask

    public void Diagnostic(EngineDiagnostic diagnostic) =>
        Console.WriteLine($"[{diagnostic.Level}] {diagnostic.Category}: {diagnostic.Message}");
}

internal sealed class MemoryCredentials : ICredentialStore
{
    private readonly Dictionary<string, (IReadOnlyDictionary<string, string> Secrets, string Revision)> _items = new();
    private readonly object _gate = new();

    public ValueTask<IReadOnlyDictionary<string, string>?> GetAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_gate) return new(_items.TryGetValue(connectionID, out var item) ? item.Secrets : null);
    }

    public ValueTask SetAsync(string connectionID, IReadOnlyDictionary<string, string> secrets, CancellationToken cancellationToken)
    {
        lock (_gate) _items[connectionID] = (secrets, Guid.NewGuid().ToString("N"));
        return ValueTask.CompletedTask;
    }

    public ValueTask RemoveAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_gate) _items.Remove(connectionID);
        return ValueTask.CompletedTask;
    }

    public ValueTask<CredentialSnapshot?> SnapshotAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_gate)
        {
            return new(_items.TryGetValue(connectionID, out var item) ? new CredentialSnapshot(item.Secrets, item.Revision) : null);
        }
    }

    public ValueTask<bool> UpdateRefreshTokenAsync(string connectionID, string token, string expectedRevision, CancellationToken cancellationToken)
    {
        lock (_gate)
        {
            if (!_items.TryGetValue(connectionID, out var item) || item.Revision != expectedRevision) return new(false);
            var secrets = new Dictionary<string, string>(item.Secrets) { ["refresh_token"] = token };
            _items[connectionID] = (secrets, Guid.NewGuid().ToString("N"));
            return new(true);
        }
    }
}

/// <summary>HTTP for the connectors. 4xx and 5xx are results, not errors. Redirects are not followed (the engine checks
/// each hop for CalDAV and iCal links before it sends credentials), and nothing is cached or kept in cookies.</summary>
internal sealed class HttpClientTransport : IHttpTransport
{
    private static readonly HttpClient Client = new(new SocketsHttpHandler
    {
        AllowAutoRedirect = false,
        UseCookies = false,
        AutomaticDecompression = DecompressionMethods.All,
    })
    {
        Timeout = TimeSpan.FromSeconds(60),
    };

    public async ValueTask<EngineHttpResponse> SendAsync(EngineHttpRequest request, CancellationToken cancellationToken)
    {
        try
        {
            using var message = new HttpRequestMessage(new HttpMethod(request.Method), request.Url);
            if (request.Body is not null) message.Content = new ByteArrayContent(request.Body);
            foreach (var (name, value) in request.Headers)
            {
                // Content headers (Content-Type, ...) belong to the content, the rest to the message.
                if (!message.Headers.TryAddWithoutValidation(name, value)) message.Content?.Headers.TryAddWithoutValidation(name, value);
            }
            using var response = await Client.SendAsync(message, cancellationToken).ConfigureAwait(false);
            var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (var header in response.Headers) headers[header.Key] = string.Join(", ", header.Value);
            foreach (var header in response.Content.Headers) headers[header.Key] = string.Join(", ", header.Value);
            var body = await response.Content.ReadAsByteArrayAsync(cancellationToken).ConfigureAwait(false);
            return new EngineHttpResponse((int)response.StatusCode, headers, body);
        }
        catch (HttpRequestException exception)
        {
            // A transport failure is Network. Keep addresses and tokens out of the message.
            throw new EngineServiceException(ServiceErrorKind.Network, "the request did not complete", exception);
        }
    }
}

/// <summary>A host without sign-in windows: every sign-in request is refused as <see cref="ServiceErrorKind.Denied"/>.</summary>
internal sealed class NoSignIn : IAuthorizationInteraction
{
    private static EngineServiceException Refuse() => new(ServiceErrorKind.Denied, "this host cannot sign in");

    public ValueTask<RedirectSession> BeginRedirectAsync(CancellationToken cancellationToken) => throw Refuse();
    public ValueTask<string> AuthorizeAsync(string sessionID, string url, CancellationToken cancellationToken) => throw Refuse();
    public ValueTask CloseRedirectAsync(string sessionID, CancellationToken cancellationToken) => ValueTask.CompletedTask;
    public ValueTask<IReadOnlyDictionary<string, string>> PromptCredentialsAsync(IReadOnlyList<CredentialField> fields, CancellationToken cancellationToken) => throw Refuse();
    public ValueTask PrepareSignInAsync(SignInForm form, CancellationToken cancellationToken) => throw Refuse();
    public ValueTask<IReadOnlyDictionary<string, string>?> LastNonSecretValuesAsync(CancellationToken cancellationToken) => new((IReadOnlyDictionary<string, string>?)null);
}
