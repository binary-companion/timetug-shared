using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization.Metadata;
using Xunit;

namespace TimeTug.Engine.Tests;

/// <summary>The golden fixtures (the contract) against the wrapper's records: every message reads into its record
/// and writes back to the same parsed JSON, so a record that drifts from the Swift DTOs fails here.</summary>
public class FixtureRoundTripTests
{
    private static IEnumerable<string> Fixtures(bool edge) =>
        Directory.EnumerateFiles(Conformance.FixtureDirectory, "*.json")
            .Where(path => Path.GetFileName(path).StartsWith("edge-", StringComparison.Ordinal) == edge)
            .OrderBy(path => path, StringComparer.Ordinal);

    [Fact]
    public void TheFixturesAreThere()
    {
        Assert.True(Fixtures(edge: false).Count() >= 40, $"fixtures not found under {Conformance.FixtureDirectory}");
        Assert.NotEmpty(Fixtures(edge: true));
    }

    [Fact]
    public void EveryFixtureRoundTripsAsParsedJson()
    {
        foreach (var path in Fixtures(edge: false))
        {
            var original = Conformance.Load(path);
            var envelope = JsonSerializer.Deserialize(original.ToJsonString(), WireJson.Default.Envelope)!;
            var again = JsonNode.Parse(JsonSerializer.Serialize(envelope, WireJson.Default.Envelope))!;
            Assert.True(Json.Equal(original, again), Path.GetFileName(path));
        }
    }

    /// <summary>Each message type has exactly one fixture named after it, and the wrapper knows every type.</summary>
    [Fact]
    public void TheCatalogMatchesTheFixtures()
    {
        var fixtureTypes = Fixtures(edge: false).Select(Path.GetFileNameWithoutExtension).OrderBy(n => n, StringComparer.Ordinal);
        var catalog = WireCatalog.HostToEngine.Keys.Concat(WireCatalog.EngineToHost.Keys).Concat(WireCatalog.Services.Keys)
            .OrderBy(n => n, StringComparer.Ordinal);
        Assert.Equal(catalog, fixtureTypes);
        foreach (var path in Fixtures(edge: false))
        {
            Assert.Equal(Path.GetFileNameWithoutExtension(path), Json.String(Conformance.Load(path)["type"]));
        }
    }

    [Fact]
    public void EveryPayloadRoundTripsThroughItsRecord()
    {
        foreach (var path in Fixtures(edge: false).Concat(Fixtures(edge: true).Where(IsTypedMessage)))
        {
            var original = Conformance.Load(path);
            var type = Json.String(original["type"])!;
            Assert.True(WireCatalog.TryGetPayloadType(type, out var typeInfo), $"{type} is not in the catalog");
            var payload = original["payload"];
            if (typeInfo is null)
            {
                Assert.True(payload is null, $"{Path.GetFileName(path)}: {type} has no payload record but the fixture has one");
                continue;
            }
            Assert.NotNull(payload);
            var record = payload.Deserialize(typeInfo);
            var again = JsonNode.Parse(JsonSerializer.Serialize(record, typeInfo));
            Assert.True(Json.Equal(payload, again),
                $"{Path.GetFileName(path)}:\nfixture {Json.Text(payload)}\nrecord  {Json.Text(again)}");
        }
    }

    /// <summary>Commands written by <see cref="IEngine.SendAsync"/> are exactly the fixtures.</summary>
    [Fact]
    public void TheWrapperWritesEveryCommandAsItsFixture()
    {
        var commands = Fixtures(edge: false).Concat(Fixtures(edge: true))
            .Select(path => (path, json: Conformance.Load(path)))
            .Where(item => IsCommandFixture(item.json))
            .ToList();
        Assert.True(commands.Count >= WireCatalog.HostToEngine.Count, "a command fixture is missing");
        foreach (var (path, json) in commands)
        {
            var command = Commands.FromEnvelope(json);
            Assert.NotNull(command);
            var written = JsonNode.Parse(NativeEngine.WriteCommand(command, Json.String(json["id"])));
            Assert.True(Json.Equal(json, written), $"{Path.GetFileName(path)}:\nfixture {Json.Text(json)}\nwritten {Json.Text(written)}");
        }
    }

    /// <summary>The reply fixtures are what the wrapper writes for a host's answer.</summary>
    [Theory]
    [InlineData("edge-reply-auth-authorize.json", "AuthAuthorizeReply")]
    [InlineData("edge-reply-auth-begin.json", "RedirectSession")]
    [InlineData("edge-reply-auth-prompt.json", "AuthPromptCredentialsReply")]
    [InlineData("edge-reply-credentials-get-none.json", "CredentialsGetReply")]
    [InlineData("edge-reply-credentials-get.json", "CredentialsGetReply")]
    [InlineData("edge-reply-credentials-snapshot.json", "CredentialsSnapshotReply")]
    [InlineData("edge-reply-credentials-update.json", "CredentialsUpdateRefreshTokenReply")]
    [InlineData("edge-reply-http-send.json", "HttpSendReply")]
    [InlineData("edge-reply-signin-last.json", "SignInLastValuesReply")]
    [InlineData("edge-reply-text-availability.json", "TextAvailability")]
    [InlineData("edge-reply-text-generate.json", "TextGenerateReply")]
    public void ReplyResultsRoundTrip(string file, string record)
    {
        var original = Conformance.Load(Path.Combine(Conformance.FixtureDirectory, file));
        var written = record switch
        {
            "AuthAuthorizeReply" => Again(original, WireJson.Default.AuthAuthorizeReply),
            "RedirectSession" => Again(original, WireJson.Default.RedirectSession),
            "AuthPromptCredentialsReply" => Again(original, WireJson.Default.AuthPromptCredentialsReply),
            "CredentialsGetReply" => Again(original, WireJson.Default.CredentialsGetReply),
            "CredentialsSnapshotReply" => Again(original, WireJson.Default.CredentialsSnapshotReply),
            "CredentialsUpdateRefreshTokenReply" => Again(original, WireJson.Default.CredentialsUpdateRefreshTokenReply),
            "HttpSendReply" => Again(original, WireJson.Default.HttpSendReply),
            "SignInLastValuesReply" => Again(original, WireJson.Default.SignInLastValuesReply),
            "TextAvailability" => Again(original, WireJson.Default.TextAvailability),
            "TextGenerateReply" => Again(original, WireJson.Default.TextGenerateReply),
            _ => throw new ArgumentException(record),
        };
        Assert.True(Json.Equal(original, JsonNode.Parse(written)), $"{file}: wrote {written}");

        static string Again<T>(JsonNode original, JsonTypeInfo<T> typeInfo) =>
            NativeEngine.Result(original["result"]!.Deserialize(typeInfo)!, typeInfo);
    }

    [Theory]
    [InlineData("network", ServiceErrorKind.Network)]
    [InlineData("cancelled", ServiceErrorKind.Cancelled)]
    [InlineData("notFound", ServiceErrorKind.NotFound)]
    [InlineData("denied", ServiceErrorKind.Denied)]
    [InlineData("failed", ServiceErrorKind.Failed)]
    public void ReplyErrorsAreTheFixtures(string kind, ServiceErrorKind value)
    {
        var original = Conformance.Load(Path.Combine(Conformance.FixtureDirectory, $"edge-reply-error-{kind}.json"));
        var written = NativeEngine.Error(value, $"{kind} example");
        Assert.True(Json.Equal(original, JsonNode.Parse(written)), written);
    }

    [Fact]
    public void UnknownMembersAreIgnored()
    {
        var original = Conformance.Load(Path.Combine(Conformance.FixtureDirectory, "edge-unknown-field.json"));
        var envelope = JsonSerializer.Deserialize(original.ToJsonString(), WireJson.Default.Envelope)!;
        Assert.Equal("refresh", envelope.Type);
        Assert.Equal("cmd-y", Json.String(envelope.Id));

        var state = JsonNode.Parse("""{"revision":3,"future":{"x":1},"takeoverVisible":true}""")!.Deserialize(WireJson.Default.EngineState)!;
        Assert.Equal(3UL, state.Revision);
        Assert.True(state.TakeoverVisible);
    }

    [Fact]
    public void InstantsAreWrittenInUtcWithMilliseconds()
    {
        var snapshot = new WidgetSnapshot { GeneratedAt = new DateTimeOffset(2025, 10, 9, 10, 53, 20, 123, TimeSpan.FromHours(2)) };
        var json = JsonNode.Parse(JsonSerializer.Serialize(snapshot, WireJson.Default.WidgetSnapshot))!;
        Assert.Equal("2025-10-09T08:53:20.123Z", Json.String(json["generatedAt"]));
        var read = json.Deserialize(WireJson.Default.WidgetSnapshot)!;
        Assert.Equal(snapshot.GeneratedAt, read.GeneratedAt);
    }

    /// <summary>A well-formed v1 command: a known type, only the envelope members, a payload exactly when the
    /// command has one.</summary>
    private static bool IsCommandFixture(JsonNode json) =>
        Json.String(json["type"]) is { } type && WireCatalog.HostToEngine.TryGetValue(type, out var typeInfo)
        && json["v"] is { } v && Json.Number(v) == 1
        && json.AsObject().All(member => member.Key is "v" or "type" or "id" or "payload")
        && (json["payload"] is not null) == (typeInfo is not null);

    private static bool IsTypedMessage(string path) =>
        Conformance.Load(path) is JsonObject json && Json.String(json["type"]) is { } type
        && WireCatalog.TryGetPayloadType(type, out _) && json["v"] is { } v && Json.Number(v) == 1
        && !Path.GetFileName(path).Equals("edge-unknown-field.json", StringComparison.Ordinal);
}

/// <summary>Builds the wrapper's command record from a command envelope (the inverse of the wrapper's writer).</summary>
internal static class Commands
{
    private static readonly Dictionary<string, Func<EngineCommand>> WithoutPayload = new()
    {
        ["start"] = () => new EngineCommand.Start(),
        ["refresh"] = () => new EngineCommand.Refresh(),
        ["account.cancel"] = () => new EngineCommand.CancelAuthorization(),
        ["dedup.forget"] = () => new EngineCommand.ForgetCorrections(),
    };

    /// <summary>The record, or null when the envelope has no command record (unknown type, bad payload).</summary>
    public static EngineCommand? FromEnvelope(JsonNode envelope)
    {
        var type = Json.String(envelope["type"]);
        if (type is null || !WireCatalog.HostToEngine.TryGetValue(type, out var typeInfo)) return null;
        if (typeInfo is null) return WithoutPayload[type]();
        try
        {
            return (envelope["payload"] ?? new JsonObject()).Deserialize(typeInfo) as EngineCommand;
        }
        catch (JsonException)
        {
            return null;
        }
    }
}
