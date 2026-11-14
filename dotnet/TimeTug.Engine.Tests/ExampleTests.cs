using Xunit;

namespace TimeTug.Engine.Tests;

/// <summary>Runs the example that docs/api.md shows (dotnet/TimeTug.Engine.Example) against the real native library.</summary>
public class ExampleTests
{
    [Fact]
    public async Task TheDocumentedExampleRuns()
    {
        using var directory = new TemporaryDirectory();
        var leadTime = await Wait.WithTimeout(TimeTug.Engine.Example.EngineExample.RunAsync(directory.Path, TimeSpan.FromSeconds(20)), seconds: 30);
        Assert.Equal(120, leadTime);
    }

    [Fact]
    public void TheApiReferenceShowsTheExampleAsItIs()
    {
        // docs/api.md embeds Program.cs; the two must not drift apart. The repository root is five levels above the
        // test assembly (dotnet/TimeTug.Engine.Tests/bin/<configuration>/<framework>/).
        var root = new DirectoryInfo(AppContext.BaseDirectory);
        while (root is not null && !File.Exists(Path.Combine(root.FullName, "docs", "api.md"))) root = root.Parent;
        Assert.NotNull(root);
        var document = File.ReadAllText(Path.Combine(root!.FullName, "docs", "api.md")).ReplaceLineEndings("\n");
        var source = File.ReadAllText(Path.Combine(root.FullName, "dotnet", "TimeTug.Engine.Example", "Program.cs"))
            .ReplaceLineEndings("\n").TrimEnd();
        Assert.Contains(source, document);
    }
}
