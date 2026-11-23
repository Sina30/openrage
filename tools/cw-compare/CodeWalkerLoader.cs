using CodeWalker.GameFiles;

namespace CwCompare;

public enum ResourceFormat
{
    Ydr,
    Ydd,
    Yft,
    Ybn,
    Ytd,
    Ypt,
    Yld,
    Ycd,
    /// <summary>
    /// Binary metadata: PSO (PSIN), RBM (RSC7+PRD0) or RBF0. Which one a file
    /// is has to come from its leading magic — the .ymt extension covers all
    /// three, so extension-based routing is not enough.
    /// </summary>
    Meta,
}

public static class CodeWalkerLoader
{
    public static ResourceFormat? FormatFromExtension(string path)
    {
        return Path.GetExtension(path).ToLowerInvariant() switch
        {
            ".ydr" => ResourceFormat.Ydr,
            ".ydd" => ResourceFormat.Ydd,
            ".yft" => ResourceFormat.Yft,
            ".ybn" => ResourceFormat.Ybn,
            ".ytd" => ResourceFormat.Ytd,
            ".ypt" => ResourceFormat.Ypt,
            ".yld" => ResourceFormat.Yld,
            ".ycd" => ResourceFormat.Ycd,
            ".ymt" or ".ymf" or ".pso" => ResourceFormat.Meta,
            _ => null,
        };
    }

    /// <summary>
    /// Resource versions that use the Gen9 (GTA V Enhanced) struct layouts.
    /// Mirrors <c>ResourceType::gen9_versions</c> on the Rust side; see
    /// <c>docs/Gen9_Resource_Versions.md</c> for the census these came from.
    /// </summary>
    private static readonly Dictionary<ResourceFormat, int[]> Gen9Versions = new()
    {
        [ResourceFormat.Ytd] = new[] { 5 },
        [ResourceFormat.Ydr] = new[] { 159, 154 },
        [ResourceFormat.Ydd] = new[] { 159 },
        [ResourceFormat.Yft] = new[] { 171 },
        [ResourceFormat.Ypt] = new[] { 71 },
    };

    /// <summary>
    /// Whether a file uses the Gen9 layouts, from its own RSC7 header version.
    /// Not from the edition of the install it came from: an Enhanced install
    /// ships plenty of resources still stored at their Gen8 version, and
    /// CodeWalker itself falls back per file for exactly that reason.
    /// </summary>
    public static bool IsGen9File(byte[] data, ResourceFormat format)
    {
        // RSC7 header: magic, version, then the two page-flag words.
        if (data.Length < 8) return false;
        if (BitConverter.ToUInt32(data, 0) != 0x37435352) return false;
        var version = (int)BitConverter.ToUInt32(data, 4);
        return Gen9Versions.TryGetValue(format, out var versions)
            && Array.IndexOf(versions, version) >= 0;
    }

    /// <summary>
    /// Loads a file with CodeWalker and returns XML export. The Gen8/Gen9
    /// layout mode is selected from the file's own resource version, so a
    /// mixed corpus can be compared in one run.
    /// </summary>
    public static (bool success, string? xml, string? error) LoadXml(string filePath, ResourceFormat format)
    {
        byte[] data;
        try
        {
            data = File.ReadAllBytes(filePath);
        }
        catch (Exception ex)
        {
            return (false, null, ex.ToString());
        }

        RpfManager.IsGen9 = IsGen9File(data, format);

        var name = Path.GetFileName(filePath);

        try
        {
            switch (format)
            {
                case ResourceFormat.Ydr:
                {
                    var f = new YdrFile();
                    f.Name = name;
                    f.Load(data);
                    if (f.Drawable == null)
                        return (false, null, "CodeWalker parsed YDR but Drawable is null.");
                    return (true, YdrXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Ydd:
                {
                    var f = new YddFile();
                    f.Name = name;
                    f.Load(data);
                    if (f.DrawableDict == null)
                        return (false, null, "CodeWalker parsed YDD but DrawableDict is null.");
                    return (true, YddXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Yft:
                {
                    var f = new YftFile();
                    f.Name = name;
                    f.Load(data);
                    if (f.Fragment == null)
                        return (false, null, "CodeWalker parsed YFT but Fragment is null.");
                    return (true, YftXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Ybn:
                {
                    var f = new YbnFile();
                    f.Name = name;
                    f.Load(data);
                    if (f.Bounds == null)
                        return (false, null, "CodeWalker parsed YBN but Bounds is null.");
                    return (true, YbnXml.GetXml(f), null);
                }
                case ResourceFormat.Ytd:
                {
                    var f = new YtdFile();
                    f.Name = name;
                    f.Load(data);
                    if (f.TextureDict == null)
                        return (false, null, "CodeWalker parsed YTD but TextureDict is null.");
                    return (true, YtdXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Ypt:
                {
                    var f = new YptFile();
                    f.Name = name;
                    f.Load(data);
                    if (!string.IsNullOrEmpty(f.ErrorMessage))
                        return (false, null, f.ErrorMessage);
                    if (f.PtfxList == null)
                        return (false, null, "CodeWalker parsed YPT but PtfxList is null.");
                    return (true, YptXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Yld:
                {
                    // YldFile has no single-argument Load overload, so go via
                    // RpfFile.LoadResourceFile, which synthesises the resource
                    // entry from the RSC7 header the same way YdrFile.Load does.
                    //
                    // Caveat worth knowing when reading a YLD diff: CodeWalker
                    // models only three of the four per-LOD slots in
                    // clothController.m_Cloth, phMorphController.m_MapData and
                    // each clothBridgeSimGfx atRangeArray, so its XML cannot
                    // show a difference in the fourth slot of any of them. A
                    // clean YLD compare is necessary but not sufficient.
                    var f = new YldFile();
                    f.Name = name;
                    RpfFile.LoadResourceFile(f, data, 8);
                    if (!string.IsNullOrEmpty(f.LoadException))
                        return (false, null, f.LoadException);
                    if (f.ClothDictionary == null)
                        return (false, null, "CodeWalker parsed YLD but ClothDictionary is null.");
                    return (true, YldXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Ycd:
                {
                    // Caveats worth knowing when reading a YCD diff, both from
                    // CodeWalker's own source comments:
                    //
                    // 1. It exports a LinearFloat channel *labelled* as a
                    //    QuantizeFloat ("TODO - FIX!  temporary: just export
                    //    this as a quantize float"), so channel-type diffs on
                    //    linear channels are CodeWalker's bug, not ours.
                    // 2. It discards crAnimation::m_Name, folding it into a
                    //    block of unknown padding, so animation names never
                    //    appear in its XML.
                    //
                    // Both are pre-registered in cw-compare-allowlist.json.
                    var f = new YcdFile();
                    f.Name = name;
                    f.Load(data);
                    if (f.ClipDictionary == null)
                        return (false, null, "CodeWalker parsed YCD but ClipDictionary is null.");
                    return (true, YcdXml.GetXml(f, ""), null);
                }
                case ResourceFormat.Meta:
                {
                    // All three containers share the .ymt extension, so route
                    // on the leading magic — the same discriminator our own
                    // reader uses.
                    //
                    // Only the RSC7 flavour goes through YmtFile: its
                    // `Load(byte[])` is `RpfFile.LoadResourceFile`, which
                    // assumes a paged resource and cannot read a bare PSIN or
                    // RBF0 stream. Those get their own loaders.
                    if (data.Length < 4)
                        return (false, null, "File is too short to identify.");
                    var magic = System.Text.Encoding.ASCII.GetString(data, 0, 4);
                    switch (magic)
                    {
                        case "PSIN":
                        {
                            var pso = new PsoFile();
                            pso.Load(data);
                            if (pso.DataSection == null || pso.DataMapSection == null)
                                return (false, null, "CodeWalker parsed PSO but its sections are null.");
                            return (true, PsoXml.GetXml(pso), null);
                        }
                        case "RBF0":
                        {
                            var rbf = new RbfFile();
                            rbf.Load(data);
                            if (rbf.current == null)
                                return (false, null, "CodeWalker parsed RBF but its root is null.");
                            return (true, RbfXml.GetXml(rbf), null);
                        }
                        case "RSC7":
                        {
                            var f = new YmtFile();
                            f.Name = name;
                            f.Load(data);
                            if (f.Meta == null)
                                return (false, null, "CodeWalker parsed RSC7 metadata but Meta is null.");
                            return (true, MetaXml.GetXml(f.Meta), null);
                        }
                        default:
                            return (false, null, $"Unrecognised metadata magic '{magic}'.");
                    }
                }
                default:
                    return (false, null, "Unsupported format.");
            }
        }
        catch (Exception ex)
        {
            return (false, null, ex.ToString());
        }
    }
}
