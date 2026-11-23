using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace CwCompare;

public sealed class AllowlistFile
{
    [JsonPropertyName("version")]
    public int Version { get; set; }

    [JsonPropertyName("entries")]
    public List<AllowlistEntry> Entries { get; set; } = new();
}

public sealed class AllowlistEntry
{
    [JsonPropertyName("pathPattern")]
    public string PathPattern { get; set; } = "";

    [JsonPropertyName("regex")]
    public bool Regex { get; set; }

    [JsonPropertyName("classification")]
    public string Classification { get; set; } = "allowed_divergence";

    [JsonPropertyName("note")]
    public string? Note { get; set; }
}

public sealed class Allowlist
{
    private readonly List<(AllowlistEntry Entry, Regex? Rx)> _compiled = new();

    public static Allowlist Load(string? path)
    {
        if (string.IsNullOrWhiteSpace(path) || !File.Exists(path))
            return new Allowlist();

        var json = File.ReadAllText(path);
        var file = JsonSerializer.Deserialize<AllowlistFile>(json, new JsonSerializerOptions
        {
            PropertyNameCaseInsensitive = true,
            ReadCommentHandling = JsonCommentHandling.Skip,
            AllowTrailingCommas = true,
        }) ?? new AllowlistFile();

        var list = new Allowlist();
        foreach (var e in file.Entries)
        {
            Regex? rx = null;
            if (e.Regex)
            {
                try
                {
                    rx = new Regex(e.PathPattern, RegexOptions.CultureInvariant | RegexOptions.Compiled);
                }
                catch
                {
                    continue;
                }
            }
            list._compiled.Add((e, rx));
        }
        return list;
    }

    /// <summary>
    /// Returns classification override if path matches an entry; otherwise null.
    /// </summary>
    public string? Match(string path)
    {
        foreach (var (entry, rx) in _compiled)
        {
            bool ok = rx != null
                ? rx.IsMatch(path)
                : path.StartsWith(entry.PathPattern, StringComparison.OrdinalIgnoreCase);
            if (ok)
                return entry.Classification;
        }
        return null;
    }

    public string? MatchNote(string path)
    {
        foreach (var (entry, rx) in _compiled)
        {
            bool ok = rx != null
                ? rx.IsMatch(path)
                : path.StartsWith(entry.PathPattern, StringComparison.OrdinalIgnoreCase);
            if (ok)
                return entry.Note;
        }
        return null;
    }
}
