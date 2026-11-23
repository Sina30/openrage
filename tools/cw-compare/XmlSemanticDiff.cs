using System.Globalization;
using System.Text;
using System.Xml.Linq;
using CwCompare.Reports;

namespace CwCompare;

public sealed class XmlSemanticDiff
{
    private readonly double _epsilon;
    private readonly Allowlist _allowlist;
    private readonly XmlFingerprintCache _fingerprints;

    public XmlSemanticDiff(double epsilon, Allowlist allowlist, XmlFingerprintCache fingerprints)
    {
        _epsilon = epsilon;
        _allowlist = allowlist;
        _fingerprints = fingerprints;
    }

    public IReadOnlyList<DiffIssue> Compare(XElement original, XElement converted)
    {
        var issues = new List<DiffIssue>();
        CompareElement("/", original, converted, issues);
        return issues;
    }

    private void CompareElement(string path, XElement a, XElement b, List<DiffIssue> issues)
    {
        if (a.Name != b.Name)
        {
            AddIssue(issues, path, "name_mismatch", a.Name.LocalName, b.Name.LocalName);
            return;
        }

        var pathHere = JoinElementPath(path, a);

        var attrsA = a.Attributes().OrderBy(x => x.Name.LocalName, StringComparer.Ordinal).ToList();
        var attrsB = b.Attributes().OrderBy(x => x.Name.LocalName, StringComparer.Ordinal).ToList();

        var names = attrsA.Select(x => x.Name.LocalName)
            .Union(attrsB.Select(x => x.Name.LocalName), StringComparer.Ordinal)
            .OrderBy(x => x, StringComparer.Ordinal)
            .ToList();

        foreach (var name in names)
        {
            var va = attrsA.FirstOrDefault(x => x.Name.LocalName == name);
            var vb = attrsB.FirstOrDefault(x => x.Name.LocalName == name);
            var ap = $"{pathHere}@{name}";

            if (va == null && vb != null)
                AddIssue(issues, ap, "extra_attribute", null, vb.Value);
            else if (va != null && vb == null)
                AddIssue(issues, ap, "missing_attribute", va.Value, null);
            else if (va != null && vb != null && !ValuesEqual(va.Value, vb.Value))
                AddIssue(issues, ap, "value_mismatch", va.Value, vb.Value);
        }

        var groupsA = a.Elements().GroupBy(e => e.Name.LocalName, StringComparer.Ordinal)
            .ToDictionary(g => g.Key, g => g.OrderBy(_fingerprints.Fingerprint, StringComparer.Ordinal).ToList(), StringComparer.Ordinal);
        var groupsB = b.Elements().GroupBy(e => e.Name.LocalName, StringComparer.Ordinal)
            .ToDictionary(g => g.Key, g => g.OrderBy(_fingerprints.Fingerprint, StringComparer.Ordinal).ToList(), StringComparer.Ordinal);

        var allTags = groupsA.Keys.Union(groupsB.Keys, StringComparer.Ordinal).OrderBy(x => x, StringComparer.Ordinal).ToList();

        foreach (var tag in allTags)
        {
            groupsA.TryGetValue(tag, out var listA);
            groupsB.TryGetValue(tag, out var listB);
            listA ??= new List<XElement>();
            listB ??= new List<XElement>();

            int n = Math.Max(listA.Count, listB.Count);
            for (int i = 0; i < n; i++)
            {
                var childPath = $"{pathHere}/{tag}[{i}]";
                if (i >= listA.Count)
                {
                    AddIssue(issues, childPath, "extra_element", null, listB[i].ToString());
                    continue;
                }
                if (i >= listB.Count)
                {
                    AddIssue(issues, childPath, "missing_element", listA[i].ToString(), null);
                    continue;
                }
                CompareElement(childPath, listA[i], listB[i], issues);
            }
        }

        var textA = string.Join("", a.Nodes().OfType<XText>().Select(t => t.Value.Trim()).Where(s => s.Length > 0));
        var textB = string.Join("", b.Nodes().OfType<XText>().Select(t => t.Value.Trim()).Where(s => s.Length > 0));
        if (a.Elements().Any() == false && b.Elements().Any() == false)
        {
            if (textA.Length > 0 || textB.Length > 0)
            {
                if (!ValuesEqual(textA, textB))
                    AddIssue(issues, $"{pathHere}/#text", "value_mismatch", textA, textB);
            }
        }
    }

    private bool ValuesEqual(string? a, string? b)
    {
        if (ReferenceEquals(a, b)) return true;
        if (a == null || b == null) return false;
        if (a == b) return true;

        if (double.TryParse(a, NumberStyles.Float, CultureInfo.InvariantCulture, out var da) &&
            double.TryParse(b, NumberStyles.Float, CultureInfo.InvariantCulture, out var db))
            return Math.Abs(da - db) <= _epsilon;

        return string.Equals(a.Trim(), b.Trim(), StringComparison.Ordinal);
    }

    private void AddIssue(List<DiffIssue> issues, string path, string kind, string? original, string? converted)
    {
        var allow = _allowlist.Match(path);
        var classification = allow ?? "unexpected_mismatch";
        issues.Add(new DiffIssue
        {
            Path = path,
            Kind = kind,
            Original = Truncate(original),
            Converted = Truncate(converted),
            Classification = classification,
            Note = _allowlist.MatchNote(path),
        });
    }

    private static string? Truncate(string? s, int max = 2000)
    {
        if (s == null) return null;
        if (s.Length <= max) return s;
        return s[..max] + "…";
    }

    private static string JoinElementPath(string path, XElement a)
    {
        var n = a.Name.LocalName;
        if (string.IsNullOrEmpty(path))
            return "/" + n;
        return path + "/" + n;
    }
}
