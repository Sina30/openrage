using System.Text.Json;
using System.Text.Json.Serialization;
using System.Xml.Linq;
using CwCompare.Reports;

namespace CwCompare;

internal static class Program
{
    private static int Main(string[] args)
    {
        if (args.Length == 0 || args[0] is "-h" or "--help" or "help")
        {
            PrintUsage();
            return 0;
        }

        return args[0].ToLowerInvariant() switch
        {
            "compare" => RunCompare(args.AsSpan(1)),
            _ => Unknown(args[0]),
        };
    }

    private static int Unknown(string cmd)
    {
        Console.Error.WriteLine($"Unknown command: {cmd}");
        PrintUsage();
        return 2;
    }

    private static void PrintUsage()
    {
        Console.WriteLine("""
            cw-compare — CodeWalker XML diff for GTA V resources (dev harness)

            Usage:
              cw-compare compare <original> <converted> [options]

            Options:
              --format <ydr|ydd|yft|ybn|ytd|ypt|yld|ycd|meta>   Override format (default: from extension)
              --epsilon <double>           Float tolerance (default: 1e-5)
              --allowlist <path>           JSON allowlist (path prefix or regex entries)
              --out <path>                 Write JSON report
              --html <path>                Write HTML report

            Exit code: 0 if no unexpected mismatches and both files parse; 1 otherwise.
            """);
    }

    private static int RunCompare(ReadOnlySpan<string> args)
    {
        if (args.Length < 2)
        {
            Console.Error.WriteLine("compare requires <original> <converted>");
            return 2;
        }

        var originalPath = Path.GetFullPath(args[0]);
        var convertedPath = Path.GetFullPath(args[1]);
        ResourceFormat? format = null;
        double epsilon = 1e-5;
        string? allowlistPath = null;
        string? jsonOut = null;
        string? htmlOut = null;

        for (int i = 2; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--format" when i + 1 < args.Length:
                {
                    var f = ParseFormat(args[++i]);
                    if (f == null)
                    {
                        Console.Error.WriteLine("Invalid --format (use ydr, ydd, yft, ybn, ytd, ypt, yld, ycd, meta)");
                        return 2;
                    }
                    format = f;
                    break;
                }
                case "--epsilon" when i + 1 < args.Length:
                    if (!double.TryParse(args[++i], System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out epsilon))
                    {
                        Console.Error.WriteLine("Invalid --epsilon");
                        return 2;
                    }
                    break;
                case "--allowlist" when i + 1 < args.Length:
                    allowlistPath = args[++i];
                    break;
                case "--out" when i + 1 < args.Length:
                    jsonOut = args[++i];
                    break;
                case "--html" when i + 1 < args.Length:
                    htmlOut = args[++i];
                    break;
                default:
                    Console.Error.WriteLine($"Unknown or incomplete option: {args[i]}");
                    return 2;
            }
        }

        format ??= CodeWalkerLoader.FormatFromExtension(originalPath)
                  ?? CodeWalkerLoader.FormatFromExtension(convertedPath);
        if (format == null)
        {
            Console.Error.WriteLine("Could not infer format; use --format.");
            return 2;
        }

        var fmt = format.Value;
        var allowlist = Allowlist.Load(allowlistPath);

        var report = new CompareReport
        {
            OriginalPath = originalPath,
            ConvertedPath = convertedPath,
            Format = fmt.ToString().ToLowerInvariant(),
            Epsilon = epsilon,
        };

        var (okO, xmlO, errO) = CodeWalkerLoader.LoadXml(originalPath, fmt);
        report.OriginalParse = new ParseSide { Success = okO, Error = errO };

        var (okC, xmlC, errC) = CodeWalkerLoader.LoadXml(convertedPath, fmt);
        report.ConvertedParse = new ParseSide { Success = okC, Error = errC };

        if (!okO || !okC || xmlO == null || xmlC == null)
        {
            WriteOutputs(report, jsonOut, htmlOut);
            Console.Error.WriteLine(okO ? "" : "Original failed CodeWalker parse.");
            Console.Error.WriteLine(okC ? "" : "Converted failed CodeWalker parse.");
            return 1;
        }

        XDocument docO;
        XDocument docC;
        try
        {
            docO = XmlCanonicalizer.LoadDocument(xmlO);
            docC = XmlCanonicalizer.LoadDocument(xmlC);
        }
        catch (Exception ex)
        {
            report.OriginalParse = new ParseSide { Success = false, Error = "XML parse (original/converted): " + ex.Message };
            WriteOutputs(report, jsonOut, htmlOut);
            return 1;
        }

        if (docO.Root == null || docC.Root == null)
        {
            report.OriginalParse = new ParseSide { Success = false, Error = "Empty XML root" };
            WriteOutputs(report, jsonOut, htmlOut);
            return 1;
        }

        var fpCache = new XmlFingerprintCache();
        var canonO = XmlCanonicalizer.Canonicalize(docO.Root, fpCache);
        var canonC = XmlCanonicalizer.Canonicalize(docC.Root, fpCache);

        var diff = new XmlSemanticDiff(epsilon, allowlist, fpCache);
        report.Issues = diff.Compare(canonO, canonC).ToList();

        WriteOutputs(report, jsonOut, htmlOut);

        if (report.UnexpectedMismatchCount > 0)
        {
            Console.WriteLine($"Unexpected mismatches: {report.UnexpectedMismatchCount} (allowed: {report.AllowedDivergenceCount})");
            return 1;
        }

        Console.WriteLine($"OK — no unexpected mismatches ({report.AllowedDivergenceCount} allowed divergences).");
        return 0;
    }

    private static ResourceFormat? ParseFormat(string s) =>
        s.ToLowerInvariant() switch
        {
            "ydr" => ResourceFormat.Ydr,
            "ydd" => ResourceFormat.Ydd,
            "yft" => ResourceFormat.Yft,
            "ybn" => ResourceFormat.Ybn,
            "ytd" => ResourceFormat.Ytd,
            "ypt" => ResourceFormat.Ypt,
            "yld" => ResourceFormat.Yld,
            "ycd" => ResourceFormat.Ycd,
            // All three binary metadata containers share one arm; the loader
            // picks between them on the leading magic.
            "meta" or "ymt" or "ymf" or "pso" => ResourceFormat.Meta,
            _ => null,
        };

    private static void WriteOutputs(CompareReport report, string? jsonOut, string? htmlOut)
    {
        var opts = new JsonSerializerOptions
        {
            WriteIndented = true,
            DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        };

        if (!string.IsNullOrEmpty(jsonOut))
        {
            var dir = Path.GetDirectoryName(Path.GetFullPath(jsonOut));
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            File.WriteAllText(jsonOut, JsonSerializer.Serialize(report, opts));
        }

        if (!string.IsNullOrEmpty(htmlOut))
        {
            var dir = Path.GetDirectoryName(Path.GetFullPath(htmlOut));
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            HtmlReportWriter.Write(htmlOut, report);
        }
    }
}
