using System.Net;
using System.Text;
using CwCompare.Reports;

namespace CwCompare;

public static class HtmlReportWriter
{
    public static void Write(string path, CompareReport report)
    {
        var sb = new StringBuilder();
        sb.AppendLine("<!DOCTYPE html><html><head><meta charset=\"utf-8\"/><title>cw-compare</title>");
        sb.AppendLine("<style>body{font-family:system-ui;margin:1rem;} table{border-collapse:collapse;} td,th{border:1px solid #ccc;padding:4px 8px;vertical-align:top;} .unexpected{color:#a00;} .allowed{color:#666;}</style>");
        sb.AppendLine("</head><body>");
        sb.AppendLine("<h1>CodeWalker XML compare</h1>");
        sb.AppendLine($"<p><b>Original:</b> {WebUtility.HtmlEncode(report.OriginalPath)}<br/>");
        sb.AppendLine($"<b>Converted:</b> {WebUtility.HtmlEncode(report.ConvertedPath)}<br/>");
        sb.AppendLine($"<b>Format:</b> {WebUtility.HtmlEncode(report.Format)} &nbsp; <b>Epsilon:</b> {report.Epsilon}</p>");

        sb.AppendLine("<h2>Parse</h2><ul>");
        sb.AppendLine($"<li>Original: {(report.OriginalParse?.Success == true ? "OK" : "FAIL")} {WebUtility.HtmlEncode(report.OriginalParse?.Error ?? "")}</li>");
        sb.AppendLine($"<li>Converted: {(report.ConvertedParse?.Success == true ? "OK" : "FAIL")} {WebUtility.HtmlEncode(report.ConvertedParse?.Error ?? "")}</li>");
        sb.AppendLine("</ul>");

        sb.AppendLine($"<h2>Issues ({report.Issues.Count})</h2>");
        sb.AppendLine("<p>Unexpected: " + report.UnexpectedMismatchCount + ", Allowed: " + report.AllowedDivergenceCount + "</p>");
        sb.AppendLine("<table><thead><tr><th>Path</th><th>Kind</th><th>Classification</th><th>Original</th><th>Converted</th><th>Note</th></tr></thead><tbody>");

        foreach (var i in report.Issues)
        {
            var cls = i.Classification == "unexpected_mismatch" ? "unexpected" : "allowed";
            sb.AppendLine("<tr class=\"" + cls + "\">");
            sb.AppendLine($"<td><code>{WebUtility.HtmlEncode(i.Path)}</code></td>");
            sb.AppendLine($"<td>{WebUtility.HtmlEncode(i.Kind)}</td>");
            sb.AppendLine($"<td>{WebUtility.HtmlEncode(i.Classification)}</td>");
            sb.AppendLine($"<td><pre>{WebUtility.HtmlEncode(i.Original ?? "")}</pre></td>");
            sb.AppendLine($"<td><pre>{WebUtility.HtmlEncode(i.Converted ?? "")}</pre></td>");
            sb.AppendLine($"<td>{WebUtility.HtmlEncode(i.Note ?? "")}</td>");
            sb.AppendLine("</tr>");
        }

        sb.AppendLine("</tbody></table></body></html>");
        File.WriteAllText(path, sb.ToString(), Encoding.UTF8);
    }
}
