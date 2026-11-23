using System.Xml.Linq;

namespace CwCompare;

/// <summary>
/// Normalizes CodeWalker XML for stable sibling ordering and attribute order.
/// </summary>
public static class XmlCanonicalizer
{
    public static XDocument LoadDocument(string xml)
    {
        var doc = XDocument.Parse(xml, LoadOptions.PreserveWhitespace);
        StripIgnorableWhitespace(doc.Root);
        return doc;
    }

    private static void StripIgnorableWhitespace(XElement? root)
    {
        if (root == null) return;
        root.Nodes().OfType<XText>().ToList().ForEach(t =>
        {
            if (string.IsNullOrWhiteSpace(t.Value))
                t.Remove();
        });
        foreach (var c in root.Elements())
            StripIgnorableWhitespace(c);
    }

    public static XElement Canonicalize(XElement source, XmlFingerprintCache cache)
    {
        var el = new XElement(source.Name);
        foreach (var a in source.Attributes().OrderBy(x => x.Name.LocalName, StringComparer.Ordinal))
            el.SetAttributeValue(a.Name, a.Value);

        var children = source.Elements().Select(ch => Canonicalize(ch, cache)).ToList();
        var grouped = children
            .GroupBy(c => c.Name.LocalName, StringComparer.Ordinal)
            .OrderBy(g => g.Key, StringComparer.Ordinal);

        foreach (var g in grouped)
        {
            var ordered = g.OrderBy(cache.Fingerprint, StringComparer.Ordinal).ToList();
            foreach (var c in ordered)
                el.Add(c);
        }

        var text = string.Join("", source.Nodes().OfType<XText>().Select(t => t.Value.Trim()).Where(s => s.Length > 0));
        if (text.Length > 0 && !el.Elements().Any())
            el.Value = text;

        return el;
    }
}
