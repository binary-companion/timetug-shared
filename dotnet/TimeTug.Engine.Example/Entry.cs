namespace TimeTug.Engine.Example;

internal static class Entry
{
    private static async Task<int> Main(string[] args)
    {
        var directory = args.Length > 0 ? args[0] : Path.Combine(Path.GetTempPath(), "timetug-engine-example");
        await EngineExample.RunAsync(directory, TimeSpan.FromSeconds(30));
        return 0;
    }
}
