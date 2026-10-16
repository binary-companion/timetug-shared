using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace TimeTug.Engine.Tests;

/// <summary>
/// Runs one conformance scenario (<c>Conformance/Scenarios/*.json</c>) through the managed wrapper and the real
/// native library, with a scripted host behind the wrapper's typed host interfaces. The format and semantics are
/// those of the Swift runner (<c>Tests/TimeTugEngineCTests/ScenarioRunner.swift</c>), which documents them:
/// <c>config</c> without <c>dataDirectory</c> (a fresh directory per scenario, kept across <c>restart</c>),
/// <c>files</c>, <c>host.http</c> / <c>credentials.seed</c> / <c>auth</c> / <c>prompt</c>, and the steps
/// <c>send</c>, <c>sendRaw</c>, <c>expect</c> (<c>events</c>: an ordered subsequence of every message since the
/// previous <c>events</c> expectation; <c>state</c>: the latest state; <c>noMoreMessagesFor</c>), <c>restart</c>,
/// <c>destroy</c> and <c>replyToPending</c>.
///
/// A <c>send</c> whose envelope the wrapper can express (a known command, a string id or none, v 1, and a payload
/// that its record writes back unchanged) goes through <see cref="IEngine.SendAsync"/>; anything else goes in as raw
/// text. Messages are recorded as the native callback delivers them, before the wrapper dispatches them.
/// </summary>
internal sealed class ScenarioRunner
{
    public sealed record Result(string Name, IReadOnlyList<string> Failures, int TypedSends, int RawSends);

    private readonly JsonNode _scenario;
    private readonly string _name;
    private readonly TimeSpan _timeout;

    public ScenarioRunner(string path, TimeSpan? timeout = null)
        : this(Conformance.Load(path), Path.GetFileNameWithoutExtension(path), timeout) { }

    public ScenarioRunner(JsonNode scenario, string? name = null, TimeSpan? timeout = null)
    {
        _scenario = scenario;
        _name = name ?? Json.String(scenario["name"]) ?? "inline";
        _timeout = timeout ?? TimeSpan.FromSeconds(5);
    }

    public async Task<Result> RunAsync()
    {
        using var directory = new TemporaryDirectory();
        var failures = new List<string>();
        int typed = 0, raw = 0;
        try
        {
            WriteFiles(directory.Path);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            return new Result(_name, [$"cannot write the scenario files: {exception.Message}"], 0, 0);
        }
        var host = new ScriptedHost(_scenario["host"] as JsonObject ?? new JsonObject());
        EngineOptions options;
        try
        {
            options = Options(directory.Path, host);
        }
        catch (ArgumentException exception)
        {
            return new Result(_name, [exception.Message], 0, 0);
        }

        NativeEngine? engine = null;
        bool Create()
        {
            try
            {
                engine = NativeEngine.Create(options, host.Receive);
                host.Attach(engine);
                return true;
            }
            catch (EngineConfigurationException exception)
            {
                failures.Add($"create failed: {exception.Message}");
                return false;
            }
        }

        if (!Create()) return new Result(_name, failures, 0, 0);
        if (_scenario["steps"] is not JsonArray steps) return new Result(_name, ["the scenario has no steps"], 0, 0);

        var cursor = 0;
        for (var index = 0; index < steps.Count; index++)
        {
            var step = steps[index]!;
            var label = $"step {index + 1}";
            if (step["send"] is JsonObject command)
            {
                var envelope = command.DeepClone().AsObject();
                if (!envelope.ContainsKey("v")) envelope["v"] = 1;
                if (await SendTypedAsync(engine, envelope)) typed++;
                else
                {
                    Send(engine, envelope.ToJsonString());
                    raw++;
                }
            }
            else if (Json.String(step["sendRaw"]) is { } text)
            {
                Send(engine, text);
                raw++;
            }
            else if (IsTrue(step["restart"]))
            {
                await host.DestroyAsync();
                if (!Create()) break;
            }
            else if (IsTrue(step["destroy"]))
            {
                await host.DestroyAsync();
            }
            else if (IsTrue(step["replyToPending"]))
            {
                host.ReplyToPending();
            }
            else if (step["expect"] is JsonObject expectation)
            {
                if (expectation["events"] is JsonArray events)
                {
                    var expected = events.ToList();
                    int? next = null;
                    await Wait.UntilAsync(() => (next = Subsequence(expected, host.Messages, cursor)) is not null, _timeout);
                    if (next is { } found)
                    {
                        cursor = found;
                    }
                    else
                    {
                        failures.Add($"{label}: expected events {Json.Text(events)} in order; received {host.Summary(cursor)}");
                        break;
                    }
                }
                if (expectation.TryGetPropertyValue("state", out var state))
                {
                    if (!await Wait.UntilAsync(() => Json.Matches(state, host.LatestState), _timeout))
                    {
                        failures.Add($"{label}: expected state {Json.Text(state)}; latest state {Json.Text(host.LatestState)}; "
                                     + $"received since the last events expectation: {host.Summary(cursor)}");
                        break;
                    }
                }
                if (expectation["noMoreMessagesFor"] is JsonValue quiet)
                {
                    var before = host.MessageCount;
                    await Task.Delay(TimeSpan.FromSeconds(Json.Number(quiet)));
                    if (host.MessageCount != before)
                    {
                        failures.Add($"{label}: {host.MessageCount - before} message(s) arrived: {host.Summary(before)}");
                        break;
                    }
                }
            }
            else
            {
                failures.Add($"{label}: unknown step {Json.Text(step)}");
                break;
            }
        }
        await host.DestroyAsync();
        return new Result(_name, failures, typed, raw);
    }

    private static bool IsTrue(JsonNode? node) =>
        node is JsonValue value && value.GetValueKind() == JsonValueKind.True;

    /// <summary>A send step after the engine is destroyed is dropped, as the Swift runner's is.</summary>
    private static void Send(NativeEngine? engine, string text)
    {
        try { engine?.SendRaw(text); }
        catch (ObjectDisposedException) { }
    }

    /// <summary>Sends through the typed API when the wrapper writes exactly this envelope; false otherwise.</summary>
    private static async Task<bool> SendTypedAsync(NativeEngine? engine, JsonObject envelope)
    {
        if (engine is null || envelope["v"]?.GetValueKind() != JsonValueKind.Number || Json.Number(envelope["v"]!) != 1) return false;
        var idNode = envelope["id"];
        if (idNode is not null && Json.String(idNode) is null) return false;
        var command = Commands.FromEnvelope(envelope);
        if (command is null) return false;
        var id = Json.String(idNode);
        if (!Json.Equal(envelope, JsonNode.Parse(NativeEngine.WriteCommand(command, id)))) return false;
        try { await engine.SendAsync(command, id); }
        catch (ObjectDisposedException) { }
        return true;
    }

    private void WriteFiles(string directory)
    {
        if (_scenario["files"] is not JsonObject files) return;
        foreach (var (name, content) in files)
        {
            var text = Json.String(content) ?? Json.Text(content);
            File.WriteAllText(Path.Combine(directory, name), text, new UTF8Encoding(false));
        }
    }

    /// <summary>The scenario's config as <see cref="EngineOptions"/>. A config member the wrapper does not map fails the
    /// scenario, so a new config field cannot be silently dropped here.</summary>
    private EngineOptions Options(string directory, ScriptedHost host)
    {
        var config = _scenario["config"] as JsonObject ?? new JsonObject();
        var clients = new Dictionary<string, OAuthClient>();
        string platform = "windows";
        string? timeZone = null;
        bool testing = false;
        DateTimeOffset? clockStart = null;
        foreach (var (key, value) in config)
        {
            switch (key)
            {
                case "v":
                    if (value is null || Json.Number(value) != 1) throw new ArgumentException($"config v {Json.Text(value)}");
                    break;
                case "platform": platform = Json.String(value)!; break;
                case "timeZone": timeZone = Json.String(value); break;
                case "testing": testing = IsTrue(value); break;
                case "testClockStart":
                    clockStart = value is null ? null : DateTimeOffset.Parse(Json.String(value)!, System.Globalization.CultureInfo.InvariantCulture);
                    break;
                case "textGeneration": host.TextGenerationEnabled = IsTrue(value); break;
                case "connectors":
                    foreach (var (family, client) in value!.AsObject())
                    {
                        clients[family] = new OAuthClient(Json.String(client!["clientID"])!, Json.String(client["clientSecret"]));
                    }
                    break;
                default:
                    throw new ArgumentException($"the C# runner does not map the config member {key}");
            }
        }
        return new EngineOptions
        {
            DataDirectory = directory,
            Platform = platform,
            TimeZone = timeZone,
            Testing = testing,
            TestClockStart = clockStart,
            OAuthClients = clients,
            Host = host,
        };
    }

    /// <summary>Index after the last matched message, or null when not all of them are there.</summary>
    public static int? Subsequence(IReadOnlyList<JsonNode?> expected, IReadOnlyList<JsonNode> messages, int start)
    {
        var index = start;
        foreach (var pattern in expected)
        {
            var found = -1;
            for (var i = index; i < messages.Count; i++)
            {
                if (Json.Matches(pattern, messages[i]))
                {
                    found = i;
                    break;
                }
            }
            if (found < 0) return null;
            index = found + 1;
        }
        return index;
    }

    /// <summary>Scenario files, sorted by name.</summary>
    public static IReadOnlyList<string> ScenarioFiles() =>
        Directory.EnumerateFiles(Conformance.ScenarioDirectory, "*.json").OrderBy(p => Path.GetFileName(p), StringComparer.Ordinal).ToList();
}

/// <summary>
/// The scripted host: answers the engine's service requests from the scenario's <c>host</c> script through the
/// wrapper's typed interfaces, and records every message the engine sends. Requests the script leaves unanswered
/// (<c>noReply</c>, <c>authorize: "never"</c>) stay pending, ignoring cancellation like the Swift host, until
/// <see cref="ReplyToPending"/>.
/// </summary>
internal sealed class ScriptedHost : IEngineHost, ICredentialStore, IHttpTransport, IAuthorizationInteraction, ITextGenerator
{
    private readonly JsonObject _script;
    private readonly object _lock = new();
    private readonly List<JsonNode> _received = [];
    private readonly Dictionary<string, (Dictionary<string, string> Secrets, string Revision)> _credentials = [];
    private readonly List<Action> _pending = [];
    private readonly List<ulong> _requestIds = [];
    private JsonNode? _latest;
    private NativeEngine? _engine;
    private nint _lastHandle;
    private int _redirects;

    public ScriptedHost(JsonObject script)
    {
        _script = script;
        if (script["credentials"]?["seed"] is JsonObject seed)
        {
            foreach (var (id, secrets) in seed)
            {
                if (secrets is not JsonObject values) continue;
                _credentials[id] = (values.Where(v => Json.String(v.Value) is not null)
                                          .ToDictionary(v => v.Key, v => Json.String(v.Value)!), Guid.NewGuid().ToString());
            }
        }
    }

    public bool TextGenerationEnabled { get; set; }

    public IReadOnlyList<JsonNode> Messages { get { lock (_lock) return _received.ToList(); } }

    public int MessageCount { get { lock (_lock) return _received.Count; } }

    public JsonNode? LatestState { get { lock (_lock) return _latest; } }

    public void Attach(NativeEngine engine)
    {
        lock (_lock)
        {
            _engine = engine;
            _lastHandle = engine.Handle;
        }
    }

    /// <summary>The tap: every message as the callback delivers it.</summary>
    public void Receive(string text)
    {
        JsonNode? message;
        try { message = JsonNode.Parse(text); }
        catch (JsonException) { return; }
        if (message is null) return;
        lock (_lock)
        {
            _received.Add(message);
            var type = Json.String(message["type"]);
            if (type == "state") _latest = message["payload"];
            if (type?.StartsWith("service.", StringComparison.Ordinal) == true && message["id"] is JsonValue id
                && id.TryGetValue(out ulong requestId))
            {
                _requestIds.Add(requestId);
            }
        }
    }

    public string Summary(int start)
    {
        var all = Messages;
        if (start >= all.Count) return "nothing";
        return string.Join(", ", all.Skip(start).Select(message =>
        {
            var type = Json.String(message["type"]) ?? "?";
            if (type == "event.diagnostic") return $"{type}({Json.String(message["payload"]?["message"])})";
            if (type == "service.http.send") return $"{type}({Json.String(message["payload"]?["url"])})";
            return type;
        }));
    }

    public async Task DestroyAsync()
    {
        NativeEngine? engine;
        lock (_lock)
        {
            engine = _engine;
            _engine = null;
        }
        if (engine is not null) await engine.DisposeAsync();
    }

    /// <summary>Answers every pending request. After a destroy the wrapper drops these answers; the runner also replies
    /// to every request id it saw through the native function on the last (stale) handle, which the engine must
    /// ignore.</summary>
    public void ReplyToPending()
    {
        List<Action> pending;
        List<ulong> ids;
        nint handle;
        lock (_lock)
        {
            pending = [.. _pending];
            _pending.Clear();
            ids = [.. _requestIds];
            handle = _lastHandle;
        }
        foreach (var answer in pending) answer();
        foreach (var id in ids) NativeMethods.Reply(handle, id, """{"v":1,"result":{}}""");
    }

    private Task<T> Pending<T>(T answer)
    {
        var source = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);
        lock (_lock) _pending.Add(() => source.TrySetResult(answer));
        return source.Task;
    }

    // MARK: IEngineHost

    public ICredentialStore Credentials => this;
    public IHttpTransport Http => this;
    public IAuthorizationInteraction Authorization => this;
    public ITextGenerator? TextGenerator => TextGenerationEnabled ? this : null;
    public void Diagnostic(EngineDiagnostic diagnostic) { }

    // MARK: Credentials

    public ValueTask<IReadOnlyDictionary<string, string>?> GetAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_lock)
        {
            return ValueTask.FromResult<IReadOnlyDictionary<string, string>?>(
                _credentials.TryGetValue(connectionID, out var entry) ? new Dictionary<string, string>(entry.Secrets) : null);
        }
    }

    public ValueTask SetAsync(string connectionID, IReadOnlyDictionary<string, string> secrets, CancellationToken cancellationToken)
    {
        lock (_lock) _credentials[connectionID] = (new Dictionary<string, string>(secrets), Guid.NewGuid().ToString());
        return ValueTask.CompletedTask;
    }

    public ValueTask RemoveAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_lock) _credentials.Remove(connectionID);
        return ValueTask.CompletedTask;
    }

    public ValueTask<CredentialSnapshot?> SnapshotAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_lock)
        {
            return ValueTask.FromResult(_credentials.TryGetValue(connectionID, out var entry)
                ? new CredentialSnapshot(new Dictionary<string, string>(entry.Secrets), entry.Revision)
                : null);
        }
    }

    public ValueTask<bool> UpdateRefreshTokenAsync(string connectionID, string token, string expectedRevision,
                                                   CancellationToken cancellationToken)
    {
        lock (_lock)
        {
            if (!_credentials.TryGetValue(connectionID, out var entry) || entry.Revision != expectedRevision)
            {
                return ValueTask.FromResult(false);
            }
            entry.Secrets["refresh_token"] = token;
            return ValueTask.FromResult(true);
        }
    }

    // MARK: HTTP

    public ValueTask<EngineHttpResponse> SendAsync(EngineHttpRequest request, CancellationToken cancellationToken)
    {
        if (_script["http"] is not JsonArray rules) return ValueTask.FromResult(NotFound);
        foreach (var rule in rules)
        {
            if (Json.String(rule?["match"]) is not { } match || !request.Url.StartsWith(match, StringComparison.Ordinal)) continue;
            if (Json.String(rule!["method"]) is { } method && method != request.Method) continue;
            if (rule["noReply"] is JsonValue noReply && noReply.GetValueKind() == JsonValueKind.True)
            {
                return new ValueTask<EngineHttpResponse>(Pending(new EngineHttpResponse(200)));
            }
            byte[] body = [];
            if (Json.String(rule["bodyText"]) is { } text) body = Encoding.UTF8.GetBytes(text);
            else if (Json.String(rule["bodyFile"]) is { } file)
            {
                var path = Path.Combine(Conformance.ScenarioDataDirectory, file);
                body = File.Exists(path) ? File.ReadAllBytes(path) : [];
            }
            else if (rule.AsObject().TryGetPropertyValue("body", out var value)) body = Encoding.UTF8.GetBytes(Json.Text(value));
            var status = rule["status"] is JsonValue code ? (int)Json.Number(code) : 200;
            var headers = rule["headers"] is JsonObject given
                ? given.ToDictionary(h => h.Key, h => Json.String(h.Value) ?? "")
                : new Dictionary<string, string> { ["content-type"] = "application/json" };
            return ValueTask.FromResult(new EngineHttpResponse(status, headers, body));
        }
        return ValueTask.FromResult(NotFound);
    }

    private static EngineHttpResponse NotFound => new(404, new Dictionary<string, string>(), []);

    // MARK: Sign-in

    public ValueTask<RedirectSession> BeginRedirectAsync(CancellationToken cancellationToken)
    {
        var number = Interlocked.Increment(ref _redirects);
        return ValueTask.FromResult(new RedirectSession($"session-{number}",
            Json.String(_script["auth"]?["redirectURI"]) ?? "http://127.0.0.1:53682/"));
    }

    public ValueTask<string> AuthorizeAsync(string sessionID, string url, CancellationToken cancellationToken)
    {
        if (Json.String(_script["auth"]?["authorize"]) == "never") return new ValueTask<string>(Pending(""));
        var query = Query(url);
        if (!query.TryGetValue("state", out var state) || !query.TryGetValue("redirect_uri", out var redirect))
        {
            throw new EngineServiceException(ServiceErrorKind.Failed, "no state or redirect in the authorization URL");
        }
        var code = Json.String(_script["auth"]?["code"]) ?? "test-code";
        var bare = redirect.Split('?', 2)[0];
        return ValueTask.FromResult($"{bare}?code={Uri.EscapeDataString(code)}&state={Uri.EscapeDataString(state)}");
    }

    private static Dictionary<string, string> Query(string url)
    {
        var result = new Dictionary<string, string>();
        var question = url.IndexOf('?', StringComparison.Ordinal);
        if (question < 0) return result;
        foreach (var part in url[(question + 1)..].Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var pair = part.Split('=', 2);
            result.TryAdd(Uri.UnescapeDataString(pair[0]), pair.Length > 1 ? Uri.UnescapeDataString(pair[1]) : "");
        }
        return result;
    }

    public ValueTask CloseRedirectAsync(string sessionID, CancellationToken cancellationToken) => ValueTask.CompletedTask;

    public ValueTask<IReadOnlyDictionary<string, string>> PromptCredentialsAsync(IReadOnlyList<CredentialField> fields,
                                                                                 CancellationToken cancellationToken)
    {
        if (_script["prompt"] is not JsonObject values) throw new EngineServiceException(ServiceErrorKind.Cancelled, "no form answer");
        return ValueTask.FromResult<IReadOnlyDictionary<string, string>>(
            values.ToDictionary(v => v.Key, v => Json.String(v.Value) ?? ""));
    }

    public ValueTask PrepareSignInAsync(SignInForm form, CancellationToken cancellationToken) => ValueTask.CompletedTask;

    public ValueTask<IReadOnlyDictionary<string, string>?> LastNonSecretValuesAsync(CancellationToken cancellationToken) =>
        ValueTask.FromResult<IReadOnlyDictionary<string, string>?>(null);

    // MARK: Text generation

    public ValueTask<TextAvailability> AvailabilityAsync(CancellationToken cancellationToken) =>
        ValueTask.FromResult(new TextAvailability("unavailable", "scripted host"));

    public ValueTask<string> GenerateAsync(string instructions, string prompt, CancellationToken cancellationToken) =>
        throw new EngineServiceException(ServiceErrorKind.NotFound, "the scripted host has no service.text.generate");
}
