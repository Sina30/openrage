using System.Text.Json.Serialization;

namespace CwCompare.Reports;

public sealed class CompareReport
{
    [JsonPropertyName("originalPath")]
    public string OriginalPath { get; set; } = "";

    [JsonPropertyName("convertedPath")]
    public string ConvertedPath { get; set; } = "";

    [JsonPropertyName("format")]
    public string Format { get; set; } = "";

    [JsonPropertyName("epsilon")]
    public double Epsilon { get; set; }

    [JsonPropertyName("originalParse")]
    public ParseSide? OriginalParse { get; set; }

    [JsonPropertyName("convertedParse")]
    public ParseSide? ConvertedParse { get; set; }

    [JsonPropertyName("issues")]
    public List<DiffIssue> Issues { get; set; } = new();

    [JsonPropertyName("unexpectedMismatchCount")]
    public int UnexpectedMismatchCount => Issues.Count(i => i.Classification == "unexpected_mismatch");

    [JsonPropertyName("allowedDivergenceCount")]
    public int AllowedDivergenceCount => Issues.Count(i => i.Classification == "allowed_divergence");
}

public sealed class ParseSide
{
    [JsonPropertyName("success")]
    public bool Success { get; set; }

    [JsonPropertyName("error")]
    public string? Error { get; set; }
}

public sealed class DiffIssue
{
    [JsonPropertyName("path")]
    public string Path { get; set; } = "";

    [JsonPropertyName("kind")]
    public string Kind { get; set; } = "";

    [JsonPropertyName("original")]
    public string? Original { get; set; }

    [JsonPropertyName("converted")]
    public string? Converted { get; set; }

    [JsonPropertyName("classification")]
    public string Classification { get; set; } = "";

    [JsonPropertyName("note")]
    public string? Note { get; set; }
}
