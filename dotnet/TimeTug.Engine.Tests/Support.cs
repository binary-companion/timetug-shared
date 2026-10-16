using System.Text.Json;
using System.Text.Json.Nodes;

namespace TimeTug.Engine.Tests;

/// <summary>Where the linked conformance files are, and JSON comparison by value (never by bytes).</summary>
internal static class Conformance
{
    public static string Root => Path.Combine(AppContext.BaseDirectory, "Conformance");
    public static string FixtureDirectory => Path.Combine(Root, "Fixtures", "golden");
    public static string ScenarioDirectory => Path.Combine(Root, "Scenarios");
    public static string ScenarioDataDirectory => Path.Combine(ScenarioDirectory, "data");

    public static JsonNode Load(string path) => JsonNode.Parse(File.ReadAllText(path))!;
}

internal static class Json
{
    /// <summary>Equal as parsed JSON: objects ignore member order, numbers compare as numbers.</summary>
    public static bool Equal(JsonNode? a, JsonNode? b)
    {
        if (a is null || b is null) return a is null && b is null;
        switch (a)
        {
            case JsonObject objectA when b is JsonObject objectB:
                return objectA.Count == objectB.Count
                    && objectA.All(member => objectB.TryGetPropertyValue(member.Key, out var other) && Equal(member.Value, other));
            case JsonArray arrayA when b is JsonArray arrayB:
                return arrayA.Count == arrayB.Count && arrayA.Zip(arrayB).All(pair => Equal(pair.First, pair.Second));
            case JsonValue valueA when b is JsonValue valueB:
                return ValueEqual(valueA, valueB);
            default:
                return false;
        }
    }

    /// <summary>The scenario subset match: every member the expected object names must match (others are ignored;
    /// an expected null also matches a missing member); arrays must have the same length and match element by
    /// element; other values must be equal (numbers as numbers).</summary>
    public static bool Matches(JsonNode? expected, JsonNode? actual)
    {
        switch (expected)
        {
            case JsonObject members:
                if (actual is not JsonObject actualObject) return false;
                return members.All(member =>
                {
                    actualObject.TryGetPropertyValue(member.Key, out var value);
                    return member.Value is null ? value is null : Matches(member.Value, value);
                });
            case JsonArray elements:
                return actual is JsonArray values && values.Count == elements.Count
                    && elements.Zip(values).All(pair => Matches(pair.First, pair.Second));
            case null:
                return actual is null;
            default:
                return actual is JsonValue && Equal(expected, actual);
        }
    }

    private static bool ValueEqual(JsonValue a, JsonValue b)
    {
        var kind = a.GetValueKind();
        if (kind != b.GetValueKind()) return false;
        return kind switch
        {
            JsonValueKind.Number => Number(a) == Number(b),
            JsonValueKind.String => a.GetValue<string>() == b.GetValue<string>(),
            _ => true, // true, false, null: equal kinds are equal values
        };
    }

    /// <summary>Any JSON number as a double (parsed values and values built in code alike).</summary>
    public static double Number(JsonNode node) =>
        double.Parse(node.ToJsonString(), System.Globalization.CultureInfo.InvariantCulture);

    public static string Text(JsonNode? node) => node?.ToJsonString() ?? "null";

    public static string? String(JsonNode? node) =>
        node is JsonValue value && value.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : null;
}

internal static class Wait
{
    public static async Task<bool> UntilAsync(Func<bool> condition, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (!condition())
        {
            if (DateTime.UtcNow >= deadline) return false;
            await Task.Delay(2);
        }
        return true;
    }

    public static async Task<T> WithTimeout<T>(Task<T> task, int seconds = 10)
    {
        if (await Task.WhenAny(task, Task.Delay(TimeSpan.FromSeconds(seconds))) != task)
        {
            throw new TimeoutException($"no result within {seconds} s");
        }
        return await task;
    }

    public static async Task WithTimeout(Task task, int seconds = 10)
    {
        if (await Task.WhenAny(task, Task.Delay(TimeSpan.FromSeconds(seconds))) != task)
        {
            throw new TimeoutException($"not finished within {seconds} s");
        }
        await task;
    }
}

/// <summary>A fresh directory per engine, removed on dispose.</summary>
internal sealed class TemporaryDirectory : IDisposable
{
    public string Path { get; } = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "tt-engine-" + Guid.NewGuid().ToString("N"));

    public TemporaryDirectory() => Directory.CreateDirectory(Path);

    public void Dispose()
    {
        try { Directory.Delete(Path, recursive: true); }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
