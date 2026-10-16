using System.Text.Json.Nodes;
using Xunit;

namespace TimeTug.Engine.Tests;

/// <summary>The conformance scenarios, the same files the Swift runner runs, through the wrapper and the native
/// library.</summary>
public class ScenarioTests
{
    public static TheoryData<string> ScenarioNames()
    {
        var data = new TheoryData<string>();
        foreach (var path in ScenarioRunner.ScenarioFiles()) data.Add(Path.GetFileNameWithoutExtension(path));
        return data;
    }

    [Theory, MemberData(nameof(ScenarioNames))]
    public async Task ScenarioPassesThroughTheRealLibrary(string name)
    {
        var result = await new ScenarioRunner(Path.Combine(Conformance.ScenarioDirectory, name + ".json")).RunAsync();
        Assert.True(result.Failures.Count == 0, $"{name}: {string.Join("\n", result.Failures)}");
    }

    /// <summary>The same set as the Swift inventory test: deleting or renaming a scenario fails here too.</summary>
    [Fact]
    public void ScenarioInventoryMatchesSwift()
    {
        var names = ScenarioRunner.ScenarioFiles().Select(Path.GetFileNameWithoutExtension).ToList();
        Assert.Equal(
            [
                "add-account-oauth", "destroy-while-signin-pending", "http-timeout", "invalid-command",
                "late-takeover-after-wake", "settings-persist-across-restart", "snooze-refire", "start-ready",
                "takeover-after-clock", "time-zone-change",
            ],
            names);
    }

    /// <summary>Valid commands in the scenarios go through the typed API, malformed ones as raw text.</summary>
    [Fact]
    public async Task ValidCommandsGoThroughTheTypedApi()
    {
        var result = await new ScenarioRunner(Path.Combine(Conformance.ScenarioDirectory, "invalid-command.json")).RunAsync();
        Assert.Empty(result.Failures);
        // start, the negative snooze and the final refresh are typed; the invalid JSON, the unknown type, the numeric
        // id, v 2 and the misnamed payload member cannot be expressed through the typed API.
        Assert.Equal(3, result.TypedSends);
        Assert.Equal(5, result.RawSends);
    }

    [Fact]
    public async Task RunnerReportsAMismatch()
    {
        var bad = JsonNode.Parse("""
            {"name":"bad","config":{"v":1,"platform":"linux","testing":true},
             "steps":[{"send":{"type":"start"}},{"expect":{"events":[{"type":"nope"}]}}]}
            """)!;
        var result = await new ScenarioRunner(bad, timeout: TimeSpan.FromSeconds(0.5)).RunAsync();
        Assert.Single(result.Failures);
    }

    [Fact]
    public async Task RunnerReportsAStateMismatchAndStrayMessages()
    {
        var bad = JsonNode.Parse("""
            {"config":{"v":1,"platform":"linux","testing":true},
             "steps":[{"send":{"type":"start"}},{"expect":{"state":{"takeoverVisible":true}}}]}
            """)!;
        Assert.Single((await new ScenarioRunner(bad, timeout: TimeSpan.FromSeconds(0.5)).RunAsync()).Failures);
        var stray = JsonNode.Parse("""
            {"config":{"v":1,"platform":"linux","testing":true},
             "steps":[{"send":{"type":"start"}},{"expect":{"noMoreMessagesFor":0.3}}]}
            """)!;
        Assert.Single((await new ScenarioRunner(stray, timeout: TimeSpan.FromSeconds(0.5)).RunAsync()).Failures);
    }

    [Fact]
    public async Task RunnerRefusesAConfigMemberItDoesNotMap()
    {
        var unmapped = JsonNode.Parse("""{"config":{"v":1,"platform":"linux","newField":1},"steps":[]}""")!;
        var result = await new ScenarioRunner(unmapped).RunAsync();
        Assert.Contains("newField", Assert.Single(result.Failures));
    }

    [Fact]
    public void SubsetMatching()
    {
        var actual = JsonNode.Parse("""{"type":"state","payload":{"a":1,"list":[1,2]}}""");
        Assert.True(Json.Matches(JsonNode.Parse("""{"type":"state"}"""), actual));
        Assert.True(Json.Matches(JsonNode.Parse("""{"payload":{"a":1.0}}"""), actual));
        Assert.False(Json.Matches(JsonNode.Parse("""{"payload":{"a":2}}"""), actual));
        Assert.False(Json.Matches(JsonNode.Parse("""{"payload":{"list":[1]}}"""), actual));
        Assert.True(Json.Matches(JsonNode.Parse("""{"missing":null}"""), actual));
        Assert.False(Json.Matches(JsonNode.Parse("""{"missing":1}"""), actual));
        Assert.False(Json.Matches(JsonNode.Parse("""{"type":null}"""), actual));
        var messages = new List<JsonNode> { JsonNode.Parse("""{"type":"a"}""")!, JsonNode.Parse("""{"type":"b"}""")!, JsonNode.Parse("""{"type":"a"}""")! };
        Assert.Equal(3, ScenarioRunner.Subsequence([JsonNode.Parse("""{"type":"b"}"""), JsonNode.Parse("""{"type":"a"}""")], messages, 0));
        Assert.Null(ScenarioRunner.Subsequence([JsonNode.Parse("""{"type":"b"}"""), JsonNode.Parse("""{"type":"b"}""")], messages, 0));
    }
}
