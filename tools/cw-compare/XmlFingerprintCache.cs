using System.Collections.Generic;
using System.Globalization;
using System.Text;
using System.Xml.Linq;

namespace CwCompare;

/// <summary>
/// Memoizes subtree fingerprints for <see cref="XElement"/> identity (canonical trees only).
/// </summary>
public sealed class XmlFingerprintCache
{
    private readonly Dictionary<XElement, string> _fingerprints = new(ReferenceEqualityComparer.Instance);

    public string Fingerprint(XElement e)
    {
        if (_fingerprints.TryGetValue(e, out var hit))
            return hit;

        var sb = new StringBuilder();
        sb.Append(e.Name.LocalName);
        foreach (var a in e.Attributes().OrderBy(x => x.Name.LocalName, StringComparer.Ordinal))
        {
            sb.Append('|');
            sb.Append(a.Name.LocalName);
            sb.Append('=');
            sb.Append(NormalizeScalarForSort(a.Value));
        }

        var sortedChildren = e.Elements()
            .OrderBy(Fingerprint, StringComparer.Ordinal)
            .ToList();
        foreach (var c in sortedChildren)
        {
            sb.Append('{');
            sb.Append(_fingerprints[c]);
            sb.Append('}');
        }

        var t = e.Value.Trim();
        if (t.Length > 0 && !e.Elements().Any())
        {
            sb.Append(':');
            sb.Append(NormalizeScalarForSort(t));
        }

        var s = sb.ToString();
        _fingerprints[e] = s;
        return s;
    }

    private static string NormalizeScalarForSort(string value)
    {
        if (double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var d))
            return d.ToString("G15", CultureInfo.InvariantCulture);
        return value;
    }
}
