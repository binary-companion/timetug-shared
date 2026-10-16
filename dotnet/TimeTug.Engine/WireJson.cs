using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using System.Text.Json.Serialization.Metadata;

namespace TimeTug.Engine;

/// <summary><c>{"v":1,"type":...,"id":...,"payload":...}</c>. The id is a number or a string; unknown members are
/// ignored.</summary>
internal sealed record Envelope
{
    public int V { get; init; }
    public string Type { get; init; } = "";
    public JsonNode? Id { get; init; }
    public JsonNode? Payload { get; init; }
}

// Service request payloads (engine to host) and reply results (host to engine) that have no public record.

internal sealed record CredentialsKeyRequest(string ConnectionID);

internal sealed record CredentialsSetRequest(string ConnectionID, Dictionary<string, string> Secrets);

internal sealed record CredentialsUpdateRefreshTokenRequest(string ConnectionID, string Token, string ExpectedRevision);

/// <summary><see cref="Body"/> is base64, absent for no body.</summary>
internal sealed record HttpSendRequest(string Method, string Url, Dictionary<string, string>? Headers, string? Body);

internal sealed record AuthAuthorizeRequest(string SessionID, string Url);

internal sealed record AuthCloseRedirectRequest(string SessionID);

internal sealed record AuthPromptCredentialsRequest(List<CredentialField> Fields);

internal sealed record TextGenerateRequest(string Instructions, string Prompt);

internal sealed record EmptyResult;

internal sealed record CredentialsGetReply(IReadOnlyDictionary<string, string>? Secrets);

internal sealed record CredentialsSnapshotReply(CredentialSnapshot? Snapshot);

internal sealed record CredentialsUpdateRefreshTokenReply(bool Updated);

/// <summary><see cref="Body"/> is base64, absent for an empty body.</summary>
internal sealed record HttpSendReply(int Status, IReadOnlyDictionary<string, string>? Headers, string? Body);

internal sealed record AuthAuthorizeReply(string Url);

internal sealed record AuthPromptCredentialsReply(IReadOnlyDictionary<string, string> Values);

internal sealed record SignInLastValuesReply(IReadOnlyDictionary<string, string>? Values);

internal sealed record TextGenerateReply(string Text);

internal sealed record ReplyError(string Kind, string Message);

/// <summary>The <c>tt_engine_create</c> configuration.</summary>
internal sealed record WireConfig
{
    public int V { get; init; } = 1;
    public string DataDirectory { get; init; } = "";
    public string Platform { get; init; } = "";
    public Dictionary<string, WireConnectorClient> Connectors { get; init; } = new();
    public bool TextGeneration { get; init; }
    public string? TimeZone { get; init; }
    public bool Testing { get; init; }
    public DateTimeOffset? TestClockStart { get; init; }
}

internal sealed record WireConnectorClient(string ClientID, string? ClientSecret);

/// <summary>Instants as the wire writes them: <c>2025-10-09T08:53:20.123Z</c> (UTC, milliseconds). Reads any ISO 8601
/// instant with a zone designator.</summary>
internal sealed class WireInstantConverter : JsonConverter<DateTimeOffset>
{
    public override DateTimeOffset Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        var text = reader.GetString();
        if (text is null || !DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out var value))
        {
            throw new JsonException("expected an ISO 8601 instant");
        }
        return value;
    }

    public override void Write(Utf8JsonWriter writer, DateTimeOffset value, JsonSerializerOptions options)
    {
        var utc = value.ToUniversalTime();
        // Round to the millisecond like the engine does.
        var ticks = (utc.Ticks + TimeSpan.TicksPerMillisecond / 2) / TimeSpan.TicksPerMillisecond * TimeSpan.TicksPerMillisecond;
        writer.WriteStringValue(new DateTimeOffset(ticks, TimeSpan.Zero)
            .ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", CultureInfo.InvariantCulture));
    }
}

/// <summary>The source-generated serializer for every wire type (no reflection, trimming and AOT safe).</summary>
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    UnmappedMemberHandling = JsonUnmappedMemberHandling.Skip,
    Converters = [typeof(WireInstantConverter)])]
[JsonSerializable(typeof(Envelope))]
[JsonSerializable(typeof(WireConfig))]
[JsonSerializable(typeof(EngineState))]
[JsonSerializable(typeof(ReadyInfo))]
[JsonSerializable(typeof(TakeoverRequest))]
[JsonSerializable(typeof(WidgetSnapshot))]
[JsonSerializable(typeof(EngineDiagnostic))]
[JsonSerializable(typeof(EngineEvent.TakeoverWithdrawn), TypeInfoPropertyName = "EventTakeoverWithdrawn")]
[JsonSerializable(typeof(EngineEvent.JoinRequested), TypeInfoPropertyName = "EventJoinRequested")]
[JsonSerializable(typeof(EngineCommand.AddAccount), TypeInfoPropertyName = "CommandAddAccount")]
[JsonSerializable(typeof(EngineCommand.ReconnectAccount), TypeInfoPropertyName = "CommandReconnectAccount")]
[JsonSerializable(typeof(EngineCommand.RemoveAccount), TypeInfoPropertyName = "CommandRemoveAccount")]
[JsonSerializable(typeof(EngineCommand.ApplySettings), TypeInfoPropertyName = "CommandApplySettings")]
[JsonSerializable(typeof(EngineCommand.SetCountdownVisible), TypeInfoPropertyName = "CommandSetCountdownVisible")]
[JsonSerializable(typeof(EngineCommand.TakeoverPresented), TypeInfoPropertyName = "CommandTakeoverPresented")]
[JsonSerializable(typeof(EngineCommand.TakeoverClosed), TypeInfoPropertyName = "CommandTakeoverClosed")]
[JsonSerializable(typeof(EngineCommand.Join), TypeInfoPropertyName = "CommandJoin")]
[JsonSerializable(typeof(EngineCommand.Dismiss), TypeInfoPropertyName = "CommandDismiss")]
[JsonSerializable(typeof(EngineCommand.Snooze), TypeInfoPropertyName = "CommandSnooze")]
[JsonSerializable(typeof(EngineCommand.TestTakeover), TypeInfoPropertyName = "CommandTestTakeover")]
[JsonSerializable(typeof(EngineCommand.Merge), TypeInfoPropertyName = "CommandMerge")]
[JsonSerializable(typeof(EngineCommand.Unmerge), TypeInfoPropertyName = "CommandUnmerge")]
[JsonSerializable(typeof(EngineCommand.Separate), TypeInfoPropertyName = "CommandSeparate")]
[JsonSerializable(typeof(EngineCommand.SystemEvent), TypeInfoPropertyName = "CommandSystemEvent")]
[JsonSerializable(typeof(EngineCommand.AdvanceTestClock), TypeInfoPropertyName = "CommandAdvanceTestClock")]
[JsonSerializable(typeof(CredentialsKeyRequest))]
[JsonSerializable(typeof(CredentialsSetRequest))]
[JsonSerializable(typeof(CredentialsUpdateRefreshTokenRequest))]
[JsonSerializable(typeof(HttpSendRequest))]
[JsonSerializable(typeof(AuthAuthorizeRequest))]
[JsonSerializable(typeof(AuthCloseRedirectRequest))]
[JsonSerializable(typeof(AuthPromptCredentialsRequest))]
[JsonSerializable(typeof(SignInForm))]
[JsonSerializable(typeof(TextGenerateRequest))]
[JsonSerializable(typeof(EmptyResult))]
[JsonSerializable(typeof(CredentialsGetReply))]
[JsonSerializable(typeof(CredentialsSnapshotReply))]
[JsonSerializable(typeof(CredentialsUpdateRefreshTokenReply))]
[JsonSerializable(typeof(HttpSendReply))]
[JsonSerializable(typeof(RedirectSession))]
[JsonSerializable(typeof(AuthAuthorizeReply))]
[JsonSerializable(typeof(AuthPromptCredentialsReply))]
[JsonSerializable(typeof(SignInLastValuesReply))]
[JsonSerializable(typeof(TextAvailability))]
[JsonSerializable(typeof(TextGenerateReply))]
[JsonSerializable(typeof(ReplyError))]
internal sealed partial class WireJson : JsonSerializerContext
{
}

/// <summary>Every message type of protocol v1 and the record its payload reads into (null: no payload). The same
/// list as the Swift <c>MessageCatalog</c>; a test compares it with the golden fixtures.</summary>
internal static class WireCatalog
{
    public static readonly IReadOnlyDictionary<string, JsonTypeInfo?> HostToEngine = new Dictionary<string, JsonTypeInfo?>
    {
        ["start"] = null,
        ["refresh"] = null,
        ["account.add"] = WireJson.Default.CommandAddAccount,
        ["account.reconnect"] = WireJson.Default.CommandReconnectAccount,
        ["account.remove"] = WireJson.Default.CommandRemoveAccount,
        ["account.cancel"] = null,
        ["settings.apply"] = WireJson.Default.CommandApplySettings,
        ["countdown.visible"] = WireJson.Default.CommandSetCountdownVisible,
        ["takeover.presented"] = WireJson.Default.CommandTakeoverPresented,
        ["takeover.closed"] = WireJson.Default.CommandTakeoverClosed,
        ["takeover.join"] = WireJson.Default.CommandJoin,
        ["takeover.dismiss"] = WireJson.Default.CommandDismiss,
        ["takeover.snooze"] = WireJson.Default.CommandSnooze,
        ["takeover.test"] = WireJson.Default.CommandTestTakeover,
        ["dedup.merge"] = WireJson.Default.CommandMerge,
        ["dedup.unmerge"] = WireJson.Default.CommandUnmerge,
        ["dedup.separate"] = WireJson.Default.CommandSeparate,
        ["dedup.forget"] = null,
        ["system.event"] = WireJson.Default.CommandSystemEvent,
        ["test.advanceClock"] = WireJson.Default.CommandAdvanceTestClock,
    };

    public static readonly IReadOnlyDictionary<string, JsonTypeInfo?> EngineToHost = new Dictionary<string, JsonTypeInfo?>
    {
        ["ready"] = WireJson.Default.ReadyInfo,
        ["state"] = WireJson.Default.EngineState,
        ["event.takeoverDue"] = WireJson.Default.TakeoverRequest,
        ["event.takeoverWithdrawn"] = WireJson.Default.EventTakeoverWithdrawn,
        ["event.widgetSnapshot"] = WireJson.Default.WidgetSnapshot,
        ["event.joinRequested"] = WireJson.Default.EventJoinRequested,
        ["event.diagnostic"] = WireJson.Default.EngineDiagnostic,
    };

    public static readonly IReadOnlyDictionary<string, JsonTypeInfo?> Services = new Dictionary<string, JsonTypeInfo?>
    {
        ["service.credentials.get"] = WireJson.Default.CredentialsKeyRequest,
        ["service.credentials.set"] = WireJson.Default.CredentialsSetRequest,
        ["service.credentials.remove"] = WireJson.Default.CredentialsKeyRequest,
        ["service.credentials.snapshot"] = WireJson.Default.CredentialsKeyRequest,
        ["service.credentials.updateRefreshToken"] = WireJson.Default.CredentialsUpdateRefreshTokenRequest,
        ["service.http.send"] = WireJson.Default.HttpSendRequest,
        ["service.auth.beginRedirect"] = null,
        ["service.auth.authorize"] = WireJson.Default.AuthAuthorizeRequest,
        ["service.auth.closeRedirect"] = WireJson.Default.AuthCloseRedirectRequest,
        ["service.auth.promptCredentials"] = WireJson.Default.AuthPromptCredentialsRequest,
        ["service.signIn.prepare"] = WireJson.Default.SignInForm,
        ["service.signIn.lastNonSecretValues"] = null,
        ["service.text.availability"] = null,
        ["service.text.generate"] = WireJson.Default.TextGenerateRequest,
    };

    /// <summary>The payload record of any message type, or null for a type without payload.</summary>
    public static bool TryGetPayloadType(string type, out JsonTypeInfo? typeInfo) =>
        HostToEngine.TryGetValue(type, out typeInfo) || EngineToHost.TryGetValue(type, out typeInfo)
        || Services.TryGetValue(type, out typeInfo);
}
