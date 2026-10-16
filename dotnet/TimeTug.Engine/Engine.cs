using System;
using System.Buffers;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization.Metadata;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;

namespace TimeTug.Engine;

/// <summary>
/// A running TimeTug engine. Send <see cref="EngineCommand.Start"/> first; the first event is
/// <see cref="EngineEvent.Ready"/>. Dispose it to stop the engine.
/// </summary>
public interface IEngine : IAsyncDisposable
{
    /// <summary>The engine states, newest last. A slow reader skips intermediate states (the newest wins). One reader
    /// at a time; ends when the engine is disposed.</summary>
    IAsyncEnumerable<EngineState> State { get; }

    /// <summary>The engine's events, in order, never dropped. One reader at a time; ends when the engine is
    /// disposed.</summary>
    IAsyncEnumerable<EngineEvent> Events { get; }

    /// <summary>Sends a command. Never blocks. <paramref name="id"/> comes back as
    /// <see cref="EngineDiagnostic.CommandId"/> on any diagnostic the command causes.</summary>
    /// <exception cref="ObjectDisposedException">The engine has been disposed.</exception>
    ValueTask SendAsync(EngineCommand command, string? id = null);
}

/// <summary>How to create an engine.</summary>
public sealed class EngineOptions
{
    /// <summary>Where the engine keeps its files (settings, accounts, ledger). One engine per directory.</summary>
    public required string DataDirectory { get; init; }

    /// <summary><c>windows</c>, <c>linux</c> or <c>macos</c>: selects the account kinds that apply.</summary>
    public string Platform { get; init; } = "windows";

    /// <summary>OAuth clients by connector family (<c>google</c>, <c>microsoft</c>). A kind without a client is listed
    /// as not configured.</summary>
    public IReadOnlyDictionary<string, OAuthClient> OAuthClients { get; init; } = new Dictionary<string, OAuthClient>();

    /// <summary>An IANA zone id; null uses the process zone. Later changes go through
    /// <see cref="EngineCommand.SystemEvent"/> with <see cref="SystemEventKind.TimeZoneChanged"/>.</summary>
    public string? TimeZone { get; init; }

    /// <summary>When given, host services and <see cref="IEngineHost.Diagnostic"/> are called through it (for
    /// example a UI thread's dispatcher); otherwise they are called on the engine's delivery thread.</summary>
    public Action<Action>? Dispatcher { get; init; }

    /// <summary>Test mode: a manual clock moved by <see cref="EngineCommand.AdvanceTestClock"/>.</summary>
    public bool Testing { get; init; }

    /// <summary>With <see cref="Testing"/>: the manual clock's first instant (default: now).</summary>
    public DateTimeOffset? TestClockStart { get; init; }

    /// <summary>The host services.</summary>
    public required IEngineHost Host { get; init; }
}

/// <summary>An OAuth client registration. Google desktop clients have a secret (not confidential for an installed
/// app); Microsoft clients have none.</summary>
public sealed record OAuthClient(string ClientID, string? ClientSecret = null);

/// <summary>Thrown by <see cref="EngineFactory.Create"/> when the engine rejects the configuration.</summary>
public sealed class EngineConfigurationException : Exception
{
    /// <summary>Creates the exception.</summary>
    public EngineConfigurationException(string message) : base(message) { }
}

/// <summary>Creates engines.</summary>
public static class EngineFactory
{
    /// <summary>Creates an engine (not started: send <see cref="EngineCommand.Start"/>).</summary>
    /// <exception cref="EngineConfigurationException">The engine rejected the configuration (an empty data
    /// directory, an unknown platform or time zone).</exception>
    public static IEngine Create(EngineOptions options) => NativeEngine.Create(options, null);
}

/// <summary>
/// The engine behind the C interface. Lifetime: a <see cref="GCHandle"/> to this object is the callback context and is
/// held until <c>tt_engine_destroy</c> has returned (after which no callback starts). The callback is a static
/// <see cref="UnmanagedCallersOnlyAttribute"/> function, so no delegate has to be kept alive.
/// </summary>
internal sealed class NativeEngine : IEngine
{
    [ThreadStatic] private static NativeEngine? t_delivering;

    private readonly IEngineHost _host;
    private readonly Action<Action>? _dispatcher;
    private readonly Action<string>? _tap;
    private readonly Channel<EngineState> _states = Channel.CreateBounded<EngineState>(
        new BoundedChannelOptions(1) { FullMode = BoundedChannelFullMode.DropOldest, SingleWriter = true });
    private readonly Channel<EngineEvent> _events = Channel.CreateUnbounded<EngineEvent>(
        new UnboundedChannelOptions { SingleWriter = true });
    private readonly CancellationTokenSource _lifetime = new();
    private readonly object _gate = new();
    private GCHandle _self;
    private nint _handle;
    private int _disposed;
    private Task? _disposal;

    private NativeEngine(EngineOptions options, Action<string>? tap)
    {
        _host = options.Host;
        _dispatcher = options.Dispatcher;
        _tap = tap;
    }

    /// <summary>The native handle, kept after disposal (tests use it to prove a stale handle is ignored).</summary>
    internal nint Handle => _handle;

    /// <param name="options">The options.</param>
    /// <param name="tap">Tests only: sees every message text before it is dispatched, on the delivery thread.</param>
    internal static unsafe NativeEngine Create(EngineOptions options, Action<string>? tap)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentNullException.ThrowIfNull(options.Host);
        NativeMethods.EnsureResolver();
        var config = new WireConfig
        {
            DataDirectory = options.DataDirectory,
            Platform = options.Platform,
            Connectors = options.OAuthClients.ToDictionary(
                pair => pair.Key, pair => new WireConnectorClient(pair.Value.ClientID, pair.Value.ClientSecret)),
            TextGeneration = options.Host.TextGenerator is not null,
            TimeZone = options.TimeZone,
            Testing = options.Testing,
            TestClockStart = options.TestClockStart,
        };
        var engine = new NativeEngine(options, tap);
        engine._self = GCHandle.Alloc(engine);
        var handle = NativeMethods.Create(JsonSerializer.Serialize(config, WireJson.Default.WireConfig), &OnMessage,
                                          GCHandle.ToIntPtr(engine._self));
        if (handle == 0)
        {
            engine._self.Free();
            throw new EngineConfigurationException(
                "The engine rejected the configuration (check the data directory, platform and time zone).");
        }
        engine._handle = handle;
        return engine;
    }

    public IAsyncEnumerable<EngineState> State => _states.Reader.ReadAllAsync();

    public IAsyncEnumerable<EngineEvent> Events => _events.Reader.ReadAllAsync();

    public ValueTask SendAsync(EngineCommand command, string? id = null)
    {
        ArgumentNullException.ThrowIfNull(command);
        SendRaw(WriteCommand(command, id));
        return ValueTask.CompletedTask;
    }

    /// <summary>Sends any text as a command (tests send malformed input through it).</summary>
    internal void SendRaw(string json)
    {
        ObjectDisposedException.ThrowIf(Volatile.Read(ref _disposed) != 0, this);
        NativeMethods.Send(_handle, json);
    }

    /// <summary>
    /// Destroys the engine once; later calls return the same task. Outside a callback it waits (off the caller's
    /// thread) until <c>tt_engine_destroy</c> returns, after which no callback runs. Inside a callback (a host
    /// service or diagnostic sink running on the delivery thread) it destroys without waiting for its own delivery and
    /// completes at once; no further callback starts after the current one returns.
    /// </summary>
    public ValueTask DisposeAsync()
    {
        TaskCompletionSource? done = null;
        lock (_gate)
        {
            if (_disposal is null)
            {
                done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                _disposal = done.Task;
            }
        }
        if (done is null)
        {
            // A later call. From inside a callback while an outside disposal waits for that very callback, it must not
            // wait for it.
            return t_delivering == this ? ValueTask.CompletedTask : new ValueTask(_disposal!);
        }
        Volatile.Write(ref _disposed, 1);
        // Host operations still running see their token cancelled; their replies are dropped.
        try { _lifetime.Cancel(); }
        catch (AggregateException) { /* a host's cancellation callback threw; disposal goes on */ }
        if (t_delivering == this)
        {
            Destroy();
            done.SetResult();
            return ValueTask.CompletedTask;
        }
        _ = Task.Run(() =>
        {
            try { Destroy(); }
            finally { done.SetResult(); }
        });
        return new ValueTask(done.Task);
    }

    private void Destroy()
    {
        NativeMethods.Destroy(_handle);
        _states.Writer.TryComplete();
        _events.Writer.TryComplete();
        _self.Free();
    }

    // MARK: - Messages from the engine

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static unsafe void OnMessage(nint context, byte* json)
    {
        // Nothing may unwind into native code.
        try
        {
            if (context == 0 || json is null) return;
            if (GCHandle.FromIntPtr(context).Target is not NativeEngine engine) return;
            var text = Marshal.PtrToStringUTF8((nint)json) ?? "";
            var previous = t_delivering;
            t_delivering = engine;
            try { engine.Receive(text); }
            finally { t_delivering = previous; }
        }
        catch (Exception)
        {
            // Receive reports its own failures; this is the last guard.
        }
    }

    private void Receive(string text)
    {
        _tap?.Invoke(text);
        Envelope? envelope;
        try { envelope = JsonSerializer.Deserialize(text, WireJson.Default.Envelope); }
        catch (JsonException) { envelope = null; }
        if (envelope is null || envelope.Type.Length == 0)
        {
            Report("the engine sent a message that is not an envelope");
            return;
        }
        if (envelope.V != 1)
        {
            Report($"the engine sent protocol version {envelope.V}");
            return;
        }
        try
        {
            Dispatch(envelope);
        }
        catch (Exception exception) when (exception is JsonException or InvalidOperationException or FormatException)
        {
            Report($"unreadable {envelope.Type} payload");
        }
    }

    private void Dispatch(Envelope envelope)
    {
        switch (envelope.Type)
        {
            case "state":
                _states.Writer.TryWrite(Read(envelope.Payload, WireJson.Default.EngineState));
                break;
            case "ready":
                _events.Writer.TryWrite(new EngineEvent.Ready(Read(envelope.Payload, WireJson.Default.ReadyInfo)));
                break;
            case "event.takeoverDue":
                _events.Writer.TryWrite(new EngineEvent.TakeoverDue(Read(envelope.Payload, WireJson.Default.TakeoverRequest)));
                break;
            case "event.takeoverWithdrawn":
                _events.Writer.TryWrite(Read(envelope.Payload, WireJson.Default.EventTakeoverWithdrawn));
                break;
            case "event.widgetSnapshot":
                _events.Writer.TryWrite(new EngineEvent.WidgetSnapshotUpdated(Read(envelope.Payload, WireJson.Default.WidgetSnapshot)));
                break;
            case "event.joinRequested":
                _events.Writer.TryWrite(Read(envelope.Payload, WireJson.Default.EventJoinRequested));
                break;
            case "event.diagnostic":
                var diagnostic = Read(envelope.Payload, WireJson.Default.EngineDiagnostic) with { CommandId = IdText(envelope.Id) };
                _events.Writer.TryWrite(new EngineEvent.Diagnostic(diagnostic));
                Notify(diagnostic);
                break;
            default:
                if (envelope.Type.StartsWith("service.", StringComparison.Ordinal))
                {
                    Serve(envelope);
                }
                else
                {
                    Report($"unknown message type {envelope.Type}");
                }
                break;
        }
    }

    private static T Read<T>(JsonNode? payload, JsonTypeInfo<T> typeInfo) =>
        (payload ?? new JsonObject()).Deserialize(typeInfo) ?? throw new JsonException("null payload");

    private static string? IdText(JsonNode? id) => id switch
    {
        null => null,
        JsonValue value when value.TryGetValue(out string? text) => text,
        _ => id.ToJsonString(),
    };

    private void Report(string message) =>
        Notify(new EngineDiagnostic { Level = "warning", Category = "wrapper", Message = message });

    private void Notify(EngineDiagnostic diagnostic) => OnHost(() => _host.Diagnostic(diagnostic));

    private void OnHost(Action action)
    {
        void Guarded()
        {
            try { action(); }
            catch (Exception) { /* a host sink that throws must not stop delivery */ }
        }

        if (_dispatcher is { } dispatcher) dispatcher(Guarded);
        else Guarded();
    }

    // MARK: - Host services

    private void Serve(Envelope envelope)
    {
        if (envelope.Id is not JsonValue idValue || !idValue.TryGetValue(out ulong requestId))
        {
            Report($"{envelope.Type} without a request id");
            return;
        }
        if (_dispatcher is { } dispatcher) dispatcher(() => _ = ServeAsync(envelope.Type, requestId, envelope.Payload));
        else _ = ServeAsync(envelope.Type, requestId, envelope.Payload);
    }

    private async Task ServeAsync(string type, ulong requestId, JsonNode? payload)
    {
        string reply;
        try
        {
            reply = await AnswerAsync(type, payload, _lifetime.Token).ConfigureAwait(false);
        }
        catch (EngineServiceException exception)
        {
            reply = Error(exception.Kind, exception.Message);
        }
        catch (OperationCanceledException)
        {
            reply = Error(ServiceErrorKind.Cancelled, "cancelled");
        }
        catch (System.Net.Http.HttpRequestException exception)
        {
            reply = Error(ServiceErrorKind.Network, exception.Message);
        }
        catch (Exception exception)
        {
            reply = Error(ServiceErrorKind.Failed, $"{exception.GetType().Name}: {exception.Message}");
        }
        // A reply after disposal is dropped (the native side would ignore it too).
        if (Volatile.Read(ref _disposed) == 0) NativeMethods.Reply(_handle, requestId, reply);
    }

    private async ValueTask<string> AnswerAsync(string type, JsonNode? payload, CancellationToken token)
    {
        var credentials = _host.Credentials;
        var authorization = _host.Authorization;
        switch (type)
        {
            case "service.credentials.get":
            {
                var request = Read(payload, WireJson.Default.CredentialsKeyRequest);
                var secrets = await credentials.GetAsync(request.ConnectionID, token).ConfigureAwait(false);
                return Result(new CredentialsGetReply(secrets), WireJson.Default.CredentialsGetReply);
            }
            case "service.credentials.set":
            {
                var request = Read(payload, WireJson.Default.CredentialsSetRequest);
                await credentials.SetAsync(request.ConnectionID, request.Secrets, token).ConfigureAwait(false);
                return Empty;
            }
            case "service.credentials.remove":
            {
                var request = Read(payload, WireJson.Default.CredentialsKeyRequest);
                await credentials.RemoveAsync(request.ConnectionID, token).ConfigureAwait(false);
                return Empty;
            }
            case "service.credentials.snapshot":
            {
                var request = Read(payload, WireJson.Default.CredentialsKeyRequest);
                var snapshot = await credentials.SnapshotAsync(request.ConnectionID, token).ConfigureAwait(false);
                return Result(new CredentialsSnapshotReply(snapshot), WireJson.Default.CredentialsSnapshotReply);
            }
            case "service.credentials.updateRefreshToken":
            {
                var request = Read(payload, WireJson.Default.CredentialsUpdateRefreshTokenRequest);
                var updated = await credentials.UpdateRefreshTokenAsync(request.ConnectionID, request.Token,
                                                                        request.ExpectedRevision, token).ConfigureAwait(false);
                return Result(new CredentialsUpdateRefreshTokenReply(updated), WireJson.Default.CredentialsUpdateRefreshTokenReply);
            }
            case "service.http.send":
            {
                var request = Read(payload, WireJson.Default.HttpSendRequest);
                var body = request.Body is null ? null : Convert.FromBase64String(request.Body);
                var response = await _host.Http.SendAsync(
                    new EngineHttpRequest(request.Method, request.Url,
                                          request.Headers ?? new Dictionary<string, string>(), body), token).ConfigureAwait(false);
                var encoded = response.Body is { Length: > 0 } bytes ? Convert.ToBase64String(bytes) : null;
                return Result(new HttpSendReply(response.Status, response.Headers, encoded), WireJson.Default.HttpSendReply);
            }
            case "service.auth.beginRedirect":
            {
                var session = await authorization.BeginRedirectAsync(token).ConfigureAwait(false);
                return Result(session, WireJson.Default.RedirectSession);
            }
            case "service.auth.authorize":
            {
                var request = Read(payload, WireJson.Default.AuthAuthorizeRequest);
                var url = await authorization.AuthorizeAsync(request.SessionID, request.Url, token).ConfigureAwait(false);
                return Result(new AuthAuthorizeReply(url), WireJson.Default.AuthAuthorizeReply);
            }
            case "service.auth.closeRedirect":
            {
                var request = Read(payload, WireJson.Default.AuthCloseRedirectRequest);
                await authorization.CloseRedirectAsync(request.SessionID, token).ConfigureAwait(false);
                return Empty;
            }
            case "service.auth.promptCredentials":
            {
                var request = Read(payload, WireJson.Default.AuthPromptCredentialsRequest);
                var values = await authorization.PromptCredentialsAsync(request.Fields, token).ConfigureAwait(false);
                return Result(new AuthPromptCredentialsReply(values), WireJson.Default.AuthPromptCredentialsReply);
            }
            case "service.signIn.prepare":
            {
                var form = Read(payload, WireJson.Default.SignInForm);
                await authorization.PrepareSignInAsync(form, token).ConfigureAwait(false);
                return Empty;
            }
            case "service.signIn.lastNonSecretValues":
            {
                var values = await authorization.LastNonSecretValuesAsync(token).ConfigureAwait(false);
                return Result(new SignInLastValuesReply(values), WireJson.Default.SignInLastValuesReply);
            }
            case "service.text.availability":
            {
                var availability = await TextGenerator().AvailabilityAsync(token).ConfigureAwait(false);
                return Result(availability, WireJson.Default.TextAvailability);
            }
            case "service.text.generate":
            {
                var request = Read(payload, WireJson.Default.TextGenerateRequest);
                var text = await TextGenerator().GenerateAsync(request.Instructions, request.Prompt, token).ConfigureAwait(false);
                return Result(new TextGenerateReply(text), WireJson.Default.TextGenerateReply);
            }
            default:
                Report($"unknown service request {type}");
                throw new EngineServiceException(ServiceErrorKind.NotFound, $"the host has no {type}");
        }
    }

    private ITextGenerator TextGenerator() =>
        _host.TextGenerator ?? throw new EngineServiceException(ServiceErrorKind.NotFound, "the host has no text generator");

    // MARK: - Writing

    private static readonly string Empty = Result(new EmptyResult(), WireJson.Default.EmptyResult);

    internal static string Result<T>(T value, JsonTypeInfo<T> typeInfo) =>
        Write(writer =>
        {
            writer.WritePropertyName("result");
            JsonSerializer.Serialize(writer, value, typeInfo);
        });

    internal static string Error(ServiceErrorKind kind, string message) =>
        Write(writer =>
        {
            writer.WritePropertyName("error");
            JsonSerializer.Serialize(writer, new ReplyError(KindText(kind), message), WireJson.Default.ReplyError);
        });

    internal static string KindText(ServiceErrorKind kind) => kind switch
    {
        ServiceErrorKind.Network => "network",
        ServiceErrorKind.Cancelled => "cancelled",
        ServiceErrorKind.NotFound => "notFound",
        ServiceErrorKind.Denied => "denied",
        _ => "failed",
    };

    /// <summary>The command envelope: <c>v</c>, <c>type</c>, the optional <c>id</c> and the payload (the command
    /// record's own members).</summary>
    internal static string WriteCommand(EngineCommand command, string? id) =>
        Write(writer =>
        {
            writer.WriteString("type", command.WireType);
            if (id is not null) writer.WriteString("id", id);
            if (WireCatalog.HostToEngine.TryGetValue(command.WireType, out var typeInfo) && typeInfo is not null)
            {
                writer.WritePropertyName("payload");
                JsonSerializer.Serialize(writer, command, typeInfo);
            }
        });

    private static string Write(Action<Utf8JsonWriter> members)
    {
        var buffer = new ArrayBufferWriter<byte>();
        using (var writer = new Utf8JsonWriter(buffer))
        {
            writer.WriteStartObject();
            writer.WriteNumber("v", 1);
            members(writer);
            writer.WriteEndObject();
        }
        return Encoding.UTF8.GetString(buffer.WrittenSpan);
    }
}
