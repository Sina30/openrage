//! The ped variation family opened as typed roots: every staged
//! `CPedVariationInfo` and `CCreatureMetaData` against the value tree it was
//! read from, each declared member against the names the documents' own
//! tables state, an edit and a grown array that must reach the file as that
//! change and nothing else, and what the game would misread, refused.

use crate::corpus::Corpus;
use crate::support;

use or_io::decl::File;
use or_io::hash::literal_string_hash;
use or_io::meta::binary::synth::name_hash;
use or_io::meta::binary::ContainerLayout;
use or_io::meta::instance::HashedString;
use or_io::meta::{
    BinaryFormat, CAnchorProps, CComponentInfo, CCreatureMetaData, CPVClothComponentData,
    CPVComponentData, CPVDrawblData, CPVTextureData, CPedCompExpressionData,
    CPedPropExpressionData, CPedPropInfo, CPedPropMetaData, CPedPropTexData, CPedSelectionSet,
    CPedVariationInfo, CShaderVariableComponent, MetaDocument, MetaInstance, Value,
};

const VARIATION: &str = "CPedVariationInfo";
const CREATURE: &str = "CCreatureMetaData";

struct Staged {
    name: String,
    bytes: Vec<u8>,
    tree: MetaDocument,
    rbf: bool,
}

/// Every staged document whose root is `root`, with its value tree.
fn staged(root: &str, what: &str) -> Option<Vec<Staged>> {
    let library = support::schema();
    let found: Vec<_> = Corpus::load()
        .iter()
        .filter_map(|f| {
            let tree = MetaDocument::parse(library, f.bytes.clone()).ok()?;
            (name_hash(&tree.instance.type_name) == literal_string_hash(root.as_bytes())).then(
                || Staged {
                    name: f.name.clone(),
                    bytes: f.bytes.clone(),
                    tree,
                    rbf: f.format == BinaryFormat::Rbf,
                },
            )
        })
        .collect();
    if found.is_empty() {
        or_test_support::skip_corpus(what, format!("no {root} is staged in the metadata corpus"));
        return None;
    }
    Some(found)
}

/// The creature metadata a typed root reads: RBF holds tags and no types, and is refused by name.
fn typed_creatures(what: &str) -> Option<Vec<Staged>> {
    let found: Vec<_> = staged(CREATURE, what)?
        .into_iter()
        .filter(|s| !s.rbf)
        .collect();
    if found.is_empty() {
        or_test_support::skip_corpus(what, "no creature metadata is staged as PSO or RBM");
        return None;
    }
    Some(found)
}

fn field<'a>(inst: &'a MetaInstance, name: &str) -> Option<&'a Value> {
    let key = literal_string_hash(name.as_bytes());
    inst.fields
        .iter()
        .find(|(n, _)| name_hash(n) == key)
        .map(|(_, v)| v)
}

fn field_mut<'a>(inst: &'a mut MetaInstance, name: &str) -> Option<&'a mut Value> {
    let key = literal_string_hash(name.as_bytes());
    inst.fields
        .iter_mut()
        .find(|(n, _)| name_hash(n) == key)
        .map(|(_, v)| v)
}

fn inner(value: Option<&Value>) -> Option<&MetaInstance> {
    match value? {
        Value::Struct(inst) | Value::Pointer(Some(inst)) => Some(inst),
        _ => None,
    }
}

fn structs<'a>(inst: &'a MetaInstance, name: &str) -> Vec<&'a MetaInstance> {
    match field(inst, name) {
        Some(Value::Array(items)) => items.iter().filter_map(|v| inner(Some(v))).collect(),
        _ => Vec::new(),
    }
}

fn uint(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::UInt(v) => Some(*v),
        Value::Int(v) => u64::try_from(*v).ok(),
        Value::Bool(v) => Some(u64::from(*v)),
        Value::Bitset { bits, .. } => u64::try_from(*bits).ok(),
        _ => None,
    }
}

fn int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Int(v) => Some(*v),
        Value::UInt(v) => i64::try_from(*v).ok(),
        _ => None,
    }
}

fn hash(value: Option<&Value>) -> Option<u32> {
    match value? {
        Value::Hash(h) => Some(h.hash),
        Value::UInt(v) => u32::try_from(*v).ok(),
        Value::String(s) => Some(or_io::string_hash(s.as_bytes())),
        _ => None,
    }
}

fn bytes_of(inst: &MetaInstance, name: &str) -> Vec<u64> {
    match field(inst, name) {
        Some(Value::Array(items)) => items.iter().filter_map(|v| uint(Some(v))).collect(),
        _ => Vec::new(),
    }
}

fn floats_of(inst: &MetaInstance, name: &str) -> Vec<f32> {
    match field(inst, name) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::Float(f) => Some(*f as f32),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// What a typed record and the tree it was read from disagree on.
#[derive(Default)]
struct Differ {
    out: Vec<String>,
}

impl Differ {
    fn eq<T: PartialEq + std::fmt::Debug>(&mut self, what: impl Fn() -> String, tree: T, typed: T) {
        if tree != typed {
            self.out
                .push(format!("{}: typed {typed:?}, tree {tree:?}", what()));
        }
    }

    fn len(&mut self, what: &str, tree: usize, typed: usize) {
        self.eq(|| format!("{what} count"), tree, typed);
    }
}

fn wide<T: Copy + Into<u64>>(items: &[T]) -> Vec<u64> {
    items.iter().map(|v| (*v).into()).collect()
}

fn expression_data(
    d: &mut Differ,
    at: &str,
    tree: &MetaInstance,
    tracks: &[u8],
    ids: &[u16],
    components: &[u8],
) {
    d.eq(
        || format!("{at} tracks"),
        bytes_of(tree, "tracks"),
        wide(tracks),
    );
    d.eq(|| format!("{at} ids"), bytes_of(tree, "ids"), wide(ids));
    d.eq(
        || format!("{at} components"),
        bytes_of(tree, "components"),
        wide(components),
    );
}

fn compare_variation(info: &CPedVariationInfo, root: &MetaInstance) -> Vec<String> {
    let mut d = Differ::default();
    for (name, typed) in [
        ("bHasTexVariations", info.b_has_tex_variations),
        ("bHasDrawblVariations", info.b_has_drawbl_variations),
        ("bHasLowLODs", info.b_has_low_lods),
        ("bIsSuperLOD", info.b_is_super_lod),
    ] {
        d.eq(
            || name.to_string(),
            uint(field(root, name)),
            Some(u64::from(typed)),
        );
    }
    d.eq(
        || "availComp".into(),
        bytes_of(root, "availComp"),
        wide(&info.avail_comp),
    );
    d.eq(
        || "dlcName".into(),
        hash(field(root, "dlcName")),
        Some(info.dlc_name.hash),
    );

    let components = structs(root, "aComponentData3");
    d.len(
        "aComponentData3",
        components.len(),
        info.a_component_data3.len(),
    );
    for (c, (tree, typed)) in components.iter().zip(&info.a_component_data3).enumerate() {
        d.eq(
            || format!("component {c} numAvailTex"),
            uint(field(tree, "numAvailTex")),
            Some(u64::from(typed.num_avail_tex)),
        );
        let drawables = structs(tree, "aDrawblData3");
        d.len(
            &format!("component {c} aDrawblData3"),
            drawables.len(),
            typed.a_drawbl_data3.len(),
        );
        for (n, (tree, typed)) in drawables.iter().zip(&typed.a_drawbl_data3).enumerate() {
            let at = format!("component {c} drawable {n}");
            d.eq(
                || format!("{at} propMask"),
                uint(field(tree, "propMask")),
                Some(u64::from(typed.prop_mask)),
            );
            d.eq(
                || format!("{at} numAlternatives"),
                uint(field(tree, "numAlternatives")),
                Some(u64::from(typed.num_alternatives)),
            );
            let textures = structs(tree, "aTexData");
            d.len(
                &format!("{at} aTexData"),
                textures.len(),
                typed.a_tex_data.len(),
            );
            for (t, (tree, typed)) in textures.iter().zip(&typed.a_tex_data).enumerate() {
                d.eq(
                    || format!("{at} texture {t} texId"),
                    uint(field(tree, "texId")),
                    Some(u64::from(typed.tex_id)),
                );
                d.eq(
                    || format!("{at} texture {t} distribution"),
                    uint(field(tree, "distribution")),
                    Some(u64::from(typed.distribution)),
                );
            }
            d.eq(
                || format!("{at} ownsCloth"),
                inner(field(tree, "clothData")).and_then(|c| uint(field(c, "ownsCloth"))),
                Some(u64::from(typed.cloth_data.owns_cloth)),
            );
        }
    }

    let infos = structs(root, "compInfos");
    d.len("compInfos", infos.len(), info.comp_infos.len());
    for (i, (tree, typed)) in infos.iter().zip(&info.comp_infos).enumerate() {
        let at = format!("compInfo {i}");
        d.eq(
            || format!("{at} audioID"),
            hash(field(tree, "pedXml_audioID")),
            Some(typed.ped_xml_audio_id.hash),
        );
        d.eq(
            || format!("{at} audioID2"),
            hash(field(tree, "pedXml_audioID2")),
            Some(typed.ped_xml_audio_id2.hash),
        );
        d.eq(
            || format!("{at} expressionMods"),
            floats_of(tree, "pedXml_expressionMods"),
            typed.ped_xml_expression_mods.clone(),
        );
        d.eq(
            || format!("{at} flags"),
            uint(field(tree, "flags")),
            Some(u64::from(typed.flags)),
        );
        d.eq(
            || format!("{at} inclusions"),
            uint(field(tree, "inclusions")),
            Some(u64::from(typed.inclusions)),
        );
        d.eq(
            || format!("{at} exclusions"),
            uint(field(tree, "exclusions")),
            Some(u64::from(typed.exclusions)),
        );
        d.eq(
            || format!("{at} vfxComps"),
            uint(field(tree, "pedXml_vfxComps")),
            Some(u64::from(typed.ped_xml_vfx_comps)),
        );
        d.eq(
            || format!("{at} pedXml_flags"),
            uint(field(tree, "pedXml_flags")),
            Some(u64::from(typed.ped_xml_flags)),
        );
        d.eq(
            || format!("{at} compIdx"),
            uint(field(tree, "pedXml_compIdx")),
            Some(u64::from(typed.ped_xml_comp_idx)),
        );
        d.eq(
            || format!("{at} drawblIdx"),
            uint(field(tree, "pedXml_drawblIdx")),
            Some(u64::from(typed.ped_xml_drawbl_idx)),
        );
    }

    let props = inner(field(root, "propInfo"));
    d.eq(
        || "numAvailProps".into(),
        props.and_then(|p| uint(field(p, "numAvailProps"))),
        Some(u64::from(info.prop_info.num_avail_props)),
    );
    let metas = props
        .map(|p| structs(p, "aPropMetaData"))
        .unwrap_or_default();
    d.len(
        "aPropMetaData",
        metas.len(),
        info.prop_info.a_prop_meta_data.len(),
    );
    for (i, (tree, typed)) in metas
        .iter()
        .zip(&info.prop_info.a_prop_meta_data)
        .enumerate()
    {
        let at = format!("prop {i}");
        d.eq(
            || format!("{at} audioId"),
            hash(field(tree, "audioId")),
            Some(typed.audio_id.hash),
        );
        d.eq(
            || format!("{at} expressionMods"),
            floats_of(tree, "expressionMods"),
            typed.expression_mods.clone(),
        );
        d.eq(
            || format!("{at} renderFlags"),
            uint(field(tree, "renderFlags")),
            Some(u64::from(typed.render_flags)),
        );
        d.eq(
            || format!("{at} propFlags"),
            uint(field(tree, "propFlags")),
            Some(u64::from(typed.prop_flags)),
        );
        d.eq(
            || format!("{at} flags"),
            uint(field(tree, "flags")),
            Some(u64::from(typed.flags)),
        );
        d.eq(
            || format!("{at} anchorId"),
            uint(field(tree, "anchorId")),
            Some(u64::from(typed.anchor_id)),
        );
        d.eq(
            || format!("{at} propId"),
            uint(field(tree, "propId")),
            Some(u64::from(typed.prop_id)),
        );
        d.eq(
            || format!("{at} stickyness"),
            uint(field(tree, "stickyness")),
            Some(u64::from(typed.stickyness)),
        );
        let textures = structs(tree, "texData");
        d.len(
            &format!("{at} texData"),
            textures.len(),
            typed.tex_data.len(),
        );
        for (t, (tree, typed)) in textures.iter().zip(&typed.tex_data).enumerate() {
            let at = format!("{at} texture {t}");
            d.eq(
                || format!("{at} inclusions"),
                uint(field(tree, "inclusions")),
                Some(u64::from(typed.inclusions)),
            );
            d.eq(
                || format!("{at} exclusions"),
                uint(field(tree, "exclusions")),
                Some(u64::from(typed.exclusions)),
            );
            d.eq(
                || format!("{at} texId"),
                uint(field(tree, "texId")),
                Some(u64::from(typed.tex_id)),
            );
            d.eq(
                || format!("{at} inclusionId"),
                uint(field(tree, "inclusionId")),
                Some(u64::from(typed.inclusion_id)),
            );
            d.eq(
                || format!("{at} exclusionId"),
                uint(field(tree, "exclusionId")),
                Some(u64::from(typed.exclusion_id)),
            );
            d.eq(
                || format!("{at} distribution"),
                uint(field(tree, "distribution")),
                Some(u64::from(typed.distribution)),
            );
        }
    }
    let anchors = props.map(|p| structs(p, "aAnchors")).unwrap_or_default();
    d.len("aAnchors", anchors.len(), info.prop_info.a_anchors.len());
    for (i, (tree, typed)) in anchors.iter().zip(&info.prop_info.a_anchors).enumerate() {
        d.eq(
            || format!("anchor {i} props"),
            bytes_of(tree, "props"),
            wide(&typed.props),
        );
    }

    let sets = structs(root, "aSelectionSets");
    d.len("aSelectionSets", sets.len(), info.a_selection_sets.len());
    for (i, (tree, typed)) in sets.iter().zip(&info.a_selection_sets).enumerate() {
        let at = format!("selection set {i}");
        d.eq(
            || format!("{at} name"),
            hash(field(tree, "name")),
            Some(typed.name.hash),
        );
        d.eq(
            || format!("{at} compDrawableId"),
            bytes_of(tree, "compDrawableId"),
            wide(&typed.comp_drawable_id),
        );
        d.eq(
            || format!("{at} compTexId"),
            bytes_of(tree, "compTexId"),
            wide(&typed.comp_tex_id),
        );
        d.eq(
            || format!("{at} propAnchorId"),
            bytes_of(tree, "propAnchorId"),
            wide(&typed.prop_anchor_id),
        );
        d.eq(
            || format!("{at} propDrawableId"),
            bytes_of(tree, "propDrawableId"),
            wide(&typed.prop_drawable_id),
        );
        d.eq(
            || format!("{at} propTexId"),
            bytes_of(tree, "propTexId"),
            wide(&typed.prop_tex_id),
        );
    }
    d.out
}

fn compare_creature(creature: &CCreatureMetaData, root: &MetaInstance) -> Vec<String> {
    let mut d = Differ::default();
    let shaders = structs(root, "shaderVariableComponents");
    d.len(
        "shaderVariableComponents",
        shaders.len(),
        creature.shader_variable_components.len(),
    );
    for (i, (tree, typed)) in shaders
        .iter()
        .zip(&creature.shader_variable_components)
        .enumerate()
    {
        let at = format!("shader variable {i}");
        d.eq(
            || format!("{at} pedcompID"),
            uint(field(tree, "pedcompID")),
            Some(u64::from(typed.pedcomp_id)),
        );
        d.eq(
            || format!("{at} maskID"),
            uint(field(tree, "maskID")),
            Some(u64::from(typed.mask_id)),
        );
        d.eq(
            || format!("{at} hash string"),
            hash(field(tree, "shaderVariableHashString")),
            Some(typed.shader_variable_hash_string.hash),
        );
        expression_data(
            &mut d,
            &at,
            tree,
            &typed.tracks,
            &typed.ids,
            &typed.components,
        );
    }
    let props = structs(root, "pedPropExpressions");
    d.len(
        "pedPropExpressions",
        props.len(),
        creature.ped_prop_expressions.len(),
    );
    for (i, (tree, typed)) in props.iter().zip(&creature.ped_prop_expressions).enumerate() {
        let at = format!("prop expression {i}");
        d.eq(
            || format!("{at} pedPropID"),
            uint(field(tree, "pedPropID")),
            Some(u64::from(typed.ped_prop_id)),
        );
        d.eq(
            || format!("{at} pedPropVarIndex"),
            int(field(tree, "pedPropVarIndex")),
            Some(i64::from(typed.ped_prop_var_index)),
        );
        d.eq(
            || format!("{at} pedPropExpressionIndex"),
            uint(field(tree, "pedPropExpressionIndex")),
            Some(u64::from(typed.ped_prop_expression_index)),
        );
        d.eq(
            || format!("{at} types"),
            bytes_of(tree, "types"),
            wide(&typed.types),
        );
        expression_data(
            &mut d,
            &at,
            tree,
            &typed.tracks,
            &typed.ids,
            &typed.components,
        );
    }
    let comps = structs(root, "pedCompExpressions");
    d.len(
        "pedCompExpressions",
        comps.len(),
        creature.ped_comp_expressions.len(),
    );
    for (i, (tree, typed)) in comps.iter().zip(&creature.ped_comp_expressions).enumerate() {
        let at = format!("component expression {i}");
        d.eq(
            || format!("{at} pedCompID"),
            uint(field(tree, "pedCompID")),
            Some(u64::from(typed.ped_comp_id)),
        );
        d.eq(
            || format!("{at} pedCompVarIndex"),
            int(field(tree, "pedCompVarIndex")),
            Some(i64::from(typed.ped_comp_var_index)),
        );
        d.eq(
            || format!("{at} pedCompExpressionIndex"),
            uint(field(tree, "pedCompExpressionIndex")),
            Some(u64::from(typed.ped_comp_expression_index)),
        );
        d.eq(
            || format!("{at} types"),
            bytes_of(tree, "types"),
            wide(&typed.types),
        );
        expression_data(
            &mut d,
            &at,
            tree,
            &typed.tracks,
            &typed.ids,
            &typed.components,
        );
    }
    d.out
}

fn report(what: &str, total: usize, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} of {total} {what}:\n  {}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// A typed root, read then saved untouched and saved again, held to the tree's own save.
macro_rules! fixed_point {
    ($failures:ident, $root:ty, $s:ident, $compare:ident) => {{
        let typed = match <$root>::from_bytes(&$s.bytes) {
            Ok(typed) => typed,
            Err(e) => {
                $failures.push(format!("{}: does not open: {e}", $s.name));
                continue;
            }
        };
        let wrong = $compare(&typed, &$s.tree.instance);
        if !wrong.is_empty() {
            $failures.push(format!(
                "{}: {}",
                $s.name,
                wrong[..wrong.len().min(3)].join("; ")
            ));
            continue;
        }
        let once = match typed.to_bytes() {
            Ok(once) => once,
            Err(e) => {
                $failures.push(format!("{}: an untouched save failed: {e}", $s.name));
                continue;
            }
        };
        if $s.tree.to_bytes().ok().as_ref() != Some(&once) {
            $failures.push(format!(
                "{}: an untouched save is not the value tree's own save",
                $s.name
            ));
            continue;
        }
        match <$root>::from_bytes(&once) {
            Ok(back) => {
                let wrong = $compare(&back, &$s.tree.instance);
                if !wrong.is_empty() {
                    $failures.push(format!(
                        "{}: the saved file reads differently from the original: {}",
                        $s.name,
                        wrong[..wrong.len().min(3)].join("; ")
                    ));
                } else if back.to_bytes().ok().as_ref() != Some(&once) {
                    $failures.push(format!("{}: a second save differs from the first", $s.name));
                }
            }
            Err(e) => $failures.push(format!("{}: the save does not reopen: {e}", $s.name)),
        }
    }};
}

#[test]
fn every_staged_ped_variation_opens_typed_and_saves_to_its_fixed_point() {
    const WHAT: &str = "every_staged_ped_variation_opens_typed_and_saves_to_its_fixed_point";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    for s in &found {
        fixed_point!(failures, CPedVariationInfo, s, compare_variation);
    }
    let drawables: usize = found
        .iter()
        .filter_map(|s| CPedVariationInfo::from_bytes(&s.bytes).ok())
        .map(|i| {
            i.a_component_data3
                .iter()
                .map(|c| c.a_drawbl_data3.len())
                .sum::<usize>()
        })
        .sum();
    assert!(
        drawables > 0,
        "{} documents are staged and none holds a drawable",
        found.len()
    );
    report(VARIATION, found.len(), &failures);
}

#[test]
fn every_staged_creature_metadata_opens_typed_and_saves_to_its_fixed_point() {
    const WHAT: &str = "every_staged_creature_metadata_opens_typed_and_saves_to_its_fixed_point";
    let Some(found) = typed_creatures(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    for s in &found {
        fixed_point!(failures, CCreatureMetaData, s, compare_creature);
    }
    let entries: usize = found
        .iter()
        .filter_map(|s| CCreatureMetaData::from_bytes(&s.bytes).ok())
        .map(|c| {
            c.shader_variable_components.len()
                + c.ped_prop_expressions.len()
                + c.ped_comp_expressions.len()
        })
        .sum();
    assert!(
        entries > 0,
        "{} documents are staged and none holds an entry",
        found.len()
    );
    report(CREATURE, found.len(), &failures);
}

/// Each struct a document's tree holds, by the hash of its name, and the hash of each member it carries.
fn stated_members(
    found: &[Staged],
) -> std::collections::HashMap<u32, std::collections::HashSet<u32>> {
    fn walk(
        inst: &MetaInstance,
        out: &mut std::collections::HashMap<u32, std::collections::HashSet<u32>>,
    ) {
        let members = out.entry(name_hash(&inst.type_name)).or_default();
        members.extend(inst.fields.iter().map(|(n, _)| name_hash(n)));
        for (_, value) in &inst.fields {
            walk_value(value, out);
        }
    }
    fn walk_value(
        value: &Value,
        out: &mut std::collections::HashMap<u32, std::collections::HashSet<u32>>,
    ) {
        match value {
            Value::Struct(inst) | Value::Pointer(Some(inst)) => walk(inst, out),
            Value::Array(items) => items.iter().for_each(|v| walk_value(v, out)),
            _ => {}
        }
    }
    let mut out = std::collections::HashMap::new();
    for s in found {
        walk(&s.tree.instance, &mut out);
    }
    out
}

#[test]
fn every_declared_member_is_one_the_documents_own_tables_state() {
    const WHAT: &str = "every_declared_member_is_one_the_documents_own_tables_state";
    let (Some(mut found), Some(creatures)) = (staged(VARIATION, WHAT), typed_creatures(WHAT))
    else {
        return;
    };
    found.extend(creatures);
    let stated = stated_members(&found);
    let declared: [(&str, &[&str]); 15] = [
        (VARIATION, CPedVariationInfo::MEMBERS),
        ("CPVComponentData", CPVComponentData::MEMBERS),
        ("CPVDrawblData", CPVDrawblData::MEMBERS),
        ("CPVTextureData", CPVTextureData::MEMBERS),
        (
            "CPVDrawblData__CPVClothComponentData",
            CPVClothComponentData::MEMBERS,
        ),
        ("CComponentInfo", CComponentInfo::MEMBERS),
        ("CPedPropInfo", CPedPropInfo::MEMBERS),
        ("CPedPropMetaData", CPedPropMetaData::MEMBERS),
        ("CPedPropTexData", CPedPropTexData::MEMBERS),
        ("CAnchorProps", CAnchorProps::MEMBERS),
        ("CPedSelectionSet", CPedSelectionSet::MEMBERS),
        (CREATURE, CCreatureMetaData::MEMBERS),
        (
            "CShaderVariableComponent",
            CShaderVariableComponent::MEMBERS,
        ),
        ("CPedPropExpressionData", CPedPropExpressionData::MEMBERS),
        ("CPedCompExpressionData", CPedCompExpressionData::MEMBERS),
    ];
    let mut failures = Vec::new();
    for (type_name, members) in declared {
        let Some(have) = stated.get(&literal_string_hash(type_name.as_bytes())) else {
            failures.push(format!("{type_name}: no staged document holds one"));
            continue;
        };
        for member in members {
            if !have.contains(&literal_string_hash(member.as_bytes())) {
                failures.push(format!(
                    "{type_name}::{member} is stated by no staged document"
                ));
            }
        }
    }
    report("declared types", declared.len(), &failures);
}

/// Whether the type table `bytes` carry names a type by `hash`.
fn names_type(bytes: &[u8], hash: u32) -> bool {
    let Ok(doc) = MetaDocument::parse(support::schema(), bytes.to_vec()) else {
        return false;
    };
    match doc.layout.map(|l| l.container) {
        Some(ContainerLayout::Pso(p)) => p.schema.struct_for(hash).is_some(),
        Some(ContainerLayout::Rbm(r)) => r.model.class_for(hash).is_some(),
        _ => false,
    }
}

fn first_component(info: &mut CPedVariationInfo) -> Option<&mut CPVComponentData> {
    info.a_component_data3
        .iter_mut()
        .find(|c| !c.a_drawbl_data3.is_empty())
}

/// The edit both paths make: a flag flipped, a count set, a name replaced and a component info's flag changed.
fn edit_typed(info: &mut CPedVariationInfo) -> bool {
    let Some(drawable) = first_component(info).and_then(|c| c.a_drawbl_data3.first_mut()) else {
        return false;
    };
    drawable.num_alternatives = drawable.num_alternatives.wrapping_add(3);
    let Some(component_info) = info.comp_infos.first_mut() else {
        return false;
    };
    component_info.flags ^= 0x10;
    info.b_has_low_lods = !info.b_has_low_lods;
    info.dlc_name = HashedString::from_hash(or_io::string_hash(b"openrage_ped_variation"));
    true
}

fn edit_in_tree(root: &mut MetaInstance) -> bool {
    let Some(Value::Array(components)) = field_mut(root, "aComponentData3") else {
        return false;
    };
    let Some(Value::Array(drawables)) = components
        .iter_mut()
        .filter_map(|c| match c {
            Value::Struct(i) | Value::Pointer(Some(i)) => field_mut(i, "aDrawblData3"),
            _ => None,
        })
        .find(|d| matches!(d, Value::Array(a) if !a.is_empty()))
    else {
        return false;
    };
    let (Value::Struct(drawable) | Value::Pointer(Some(drawable))) = &mut drawables[0] else {
        return false;
    };
    let Some(slot) = field_mut(drawable, "numAlternatives") else {
        return false;
    };
    let was = uint(Some(slot)).unwrap_or(0) as u8;
    *slot = Value::UInt(u64::from(was.wrapping_add(3)));

    let Some(Value::Array(infos)) = field_mut(root, "compInfos") else {
        return false;
    };
    let Some(Value::Struct(first) | Value::Pointer(Some(first))) = infos.first_mut() else {
        return false;
    };
    let Some(slot) = field_mut(first, "flags") else {
        return false;
    };
    *slot = Value::UInt(uint(Some(slot)).unwrap_or(0) ^ 0x10);

    let Some(slot) = field_mut(root, "bHasLowLODs") else {
        return false;
    };
    *slot = Value::Bool(!matches!(slot, Value::Bool(true)));
    let Some(slot) = field_mut(root, "dlcName") else {
        return false;
    };
    *slot = Value::Hash(HashedString::from_hash(or_io::string_hash(
        b"openrage_ped_variation",
    )));
    true
}

#[test]
fn an_edited_variation_reaches_the_file_as_that_edit_and_nothing_else() {
    const WHAT: &str = "an_edited_variation_reaches_the_file_as_that_edit_and_nothing_else";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut edited_any = 0usize;
    for s in &found {
        let Ok(info) = CPedVariationInfo::from_bytes(&s.bytes) else {
            continue;
        };
        let mut edited = info.clone();
        let mut expected = s.tree.clone();
        if !edit_typed(&mut edited) || !edit_in_tree(&mut expected.instance) {
            continue;
        }
        edited_any += 1;
        match (edited.to_bytes(), expected.to_bytes()) {
            (Ok(saved), Ok(want)) if saved == want => match CPedVariationInfo::from_bytes(&saved) {
                Ok(back) if back == edited => {}
                Ok(_) => failures.push(format!("{}: the edit does not reopen as saved", s.name)),
                Err(e) => failures.push(format!("{}: the edit does not reopen: {e}", s.name)),
            },
            (Ok(_), Ok(_)) => failures.push(format!(
                "{}: the edit changed more than its members, or left the document's layout",
                s.name
            )),
            (Err(e), _) => failures.push(format!("{}: the edit does not save: {e}", s.name)),
            (_, Err(e)) => failures.push(format!("{}: the edited tree does not save: {e}", s.name)),
        }
    }
    assert!(
        edited_any > 0,
        "no staged variation holds a drawable and a component info to edit"
    );
    report("edited variations", edited_any, &failures);
}

/// Every array a clothing tool grows: a texture, a drawable, a component's info, a prop and a selection set, each a copy of one the file holds.
fn grow_typed(info: &mut CPedVariationInfo) -> bool {
    let Some(component) = first_component(info) else {
        return false;
    };
    let Some(mut drawable) = component.a_drawbl_data3.last().cloned() else {
        return false;
    };
    let Some(mut texture) = drawable.a_tex_data.last().cloned() else {
        return false;
    };
    texture.tex_id = 9;
    drawable.a_tex_data.push(texture);
    component.a_drawbl_data3.push(drawable);

    let Some(last) = info.comp_infos.last().cloned() else {
        return false;
    };
    info.comp_infos.push(last);
    if let Some(prop) = info.prop_info.a_prop_meta_data.last().cloned() {
        info.prop_info.a_prop_meta_data.push(prop);
    }
    if let Some(set) = info.a_selection_sets.last().cloned() {
        info.a_selection_sets.push(set);
    }
    // ⚠ The counts follow the arrays: the save refuses a variation that states another.
    info.restate_counts();
    true
}

fn push_copy(root: &mut MetaInstance, array: &str, edit: impl FnOnce(&mut MetaInstance)) -> bool {
    let Some(Value::Array(items)) = field_mut(root, array) else {
        return false;
    };
    let Some(mut copy) = items.last().cloned() else {
        return false;
    };
    if let Value::Struct(inst) | Value::Pointer(Some(inst)) = &mut copy {
        edit(inst);
    }
    items.push(copy);
    true
}

fn grow_in_tree(root: &mut MetaInstance) -> bool {
    let Some(Value::Array(components)) = field_mut(root, "aComponentData3") else {
        return false;
    };
    let Some(component) = components.iter_mut().find_map(|c| match c {
        Value::Struct(i) | Value::Pointer(Some(i)) => {
            matches!(field(i, "aDrawblData3"), Some(Value::Array(a)) if !a.is_empty()).then_some(i)
        }
        _ => None,
    }) else {
        return false;
    };
    let grew = push_copy(component, "aDrawblData3", |drawable| {
        push_copy(drawable, "aTexData", |texture| {
            if let Some(slot) = field_mut(texture, "texId") {
                *slot = Value::UInt(9);
            }
        });
    });
    let held: usize = structs(component, "aDrawblData3")
        .iter()
        .map(|d| structs(d, "aTexData").len())
        .sum();
    if let Some(slot) = field_mut(component, "numAvailTex") {
        *slot = Value::UInt((held % 256) as u64);
    }
    if !grew || !push_copy(root, "compInfos", |_| {}) {
        return false;
    }
    if let Some(Value::Struct(props) | Value::Pointer(Some(props))) = field_mut(root, "propInfo") {
        if push_copy(props, "aPropMetaData", |_| {}) {
            let held = structs(props, "aPropMetaData").len();
            if let Some(slot) = field_mut(props, "numAvailProps") {
                *slot = Value::UInt((held % 256) as u64);
            }
        }
    }
    push_copy(root, "aSelectionSets", |_| {});
    true
}

#[test]
fn a_grown_array_reaches_the_file_as_that_growth_and_nothing_else() {
    const WHAT: &str = "a_grown_array_reaches_the_file_as_that_growth_and_nothing_else";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut grown_any = 0usize;
    for s in &found {
        let Ok(info) = CPedVariationInfo::from_bytes(&s.bytes) else {
            continue;
        };
        let mut grown = info.clone();
        let mut expected = s.tree.clone();
        if !grow_typed(&mut grown) || !grow_in_tree(&mut expected.instance) {
            continue;
        }
        grown_any += 1;
        let saved = match grown.to_bytes() {
            Ok(saved) => saved,
            Err(e) => {
                failures.push(format!("{}: the grown array does not save: {e}", s.name));
                continue;
            }
        };
        if !names_type(
            &saved,
            literal_string_hash(b"CPVDrawblData__CPVClothComponentData"),
        ) {
            failures.push(format!(
                "{}: the grown file's type table lacks the game's hash for CPVDrawblData::CPVClothComponentData",
                s.name
            ));
            continue;
        }
        let back = match CPedVariationInfo::from_bytes(&saved) {
            Ok(back) => back,
            Err(e) => {
                failures.push(format!("{}: the grown file does not reopen: {e}", s.name));
                continue;
            }
        };
        let wrong = compare_variation(&back, &expected.instance);
        if !wrong.is_empty() {
            failures.push(format!(
                "{}: the grown file is not the grown tree: {}",
                s.name,
                wrong[..wrong.len().min(3)].join("; ")
            ));
            continue;
        }
        if back != grown {
            failures.push(format!(
                "{}: the grown file reopens as something else",
                s.name
            ));
            continue;
        }
        match back.to_bytes() {
            Ok(again) if again == saved => {}
            Ok(_) => failures.push(format!(
                "{}: a second save of the grown file differs",
                s.name
            )),
            Err(e) => failures.push(format!(
                "{}: the grown file does not save again: {e}",
                s.name
            )),
        }
    }
    assert!(
        grown_any > 0,
        "no staged variation holds a drawable to grow"
    );
    report("grown variations", grown_any, &failures);
}

#[test]
fn a_grown_creature_array_reaches_the_file() {
    const WHAT: &str = "a_grown_creature_array_reaches_the_file";
    let Some(found) = typed_creatures(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut grown_any = 0usize;
    for s in &found {
        let Ok(creature) = CCreatureMetaData::from_bytes(&s.bytes) else {
            continue;
        };
        let mut grown = creature.clone();
        let mut expected = s.tree.clone();
        let (Some(shader), Some(comp)) = (
            grown.shader_variable_components.last().cloned(),
            grown.ped_comp_expressions.last().cloned(),
        ) else {
            continue;
        };
        grown.shader_variable_components.push(shader);
        grown.ped_comp_expressions.push(comp);
        grown_any += 1;
        let _ = (
            push_copy(&mut expected.instance, "shaderVariableComponents", |_| {}),
            push_copy(&mut expected.instance, "pedCompExpressions", |_| {}),
        );
        let saved = match grown.to_bytes() {
            Ok(saved) => saved,
            Err(e) => {
                failures.push(format!("{}: the grown array does not save: {e}", s.name));
                continue;
            }
        };
        match CCreatureMetaData::from_bytes(&saved) {
            Ok(back) => {
                let wrong = compare_creature(&back, &expected.instance);
                if !wrong.is_empty() {
                    failures.push(format!(
                        "{}: {}",
                        s.name,
                        wrong[..wrong.len().min(3)].join("; ")
                    ));
                } else if back != grown {
                    failures.push(format!(
                        "{}: the grown file reopens as something else",
                        s.name
                    ));
                } else if back.to_bytes().ok().as_ref() != Some(&saved) {
                    failures.push(format!(
                        "{}: a second save of the grown file differs",
                        s.name
                    ));
                }
            }
            Err(e) => failures.push(format!("{}: the grown file does not reopen: {e}", s.name)),
        }
    }
    assert!(
        grown_any > 0,
        "no staged creature metadata holds entries to grow"
    );
    report("grown creature documents", grown_any, &failures);
}

fn a_variation(what: &str) -> Option<CPedVariationInfo> {
    staged(VARIATION, what)?
        .iter()
        .filter_map(|s| CPedVariationInfo::from_bytes(&s.bytes).ok())
        .find(|i| {
            i.a_component_data3
                .iter()
                .any(|c| !c.a_drawbl_data3.is_empty())
                && !i.comp_infos.is_empty()
        })
}

#[test]
fn a_component_slot_naming_data_the_file_lacks_is_refused_at_save() {
    const WHAT: &str = "a_component_slot_naming_data_the_file_lacks_is_refused_at_save";
    let Some(mut info) = a_variation(WHAT) else {
        return;
    };
    info.to_bytes().expect("the file as staged saves");
    info.avail_comp[0] = info.a_component_data3.len() as u8;
    let err = info.to_bytes().expect_err("a slot past the component data");
    assert!(err.to_string().contains("availComp"), "{err}");
}

#[test]
fn a_component_info_naming_a_drawable_the_file_lacks_is_refused_at_save() {
    const WHAT: &str = "a_component_info_naming_a_drawable_the_file_lacks_is_refused_at_save";
    let Some(mut info) = a_variation(WHAT) else {
        return;
    };
    info.comp_infos[0].ped_xml_drawbl_idx = 255;
    let err = info.to_bytes().expect_err("a drawable no component holds");
    assert!(err.to_string().contains("compInfos"), "{err}");
}

#[test]
fn expression_arrays_of_different_lengths_are_refused_at_save() {
    const WHAT: &str = "expression_arrays_of_different_lengths_are_refused_at_save";
    let Some(found) = typed_creatures(WHAT) else {
        return;
    };
    let Some(mut creature) = found
        .iter()
        .filter_map(|s| CCreatureMetaData::from_bytes(&s.bytes).ok())
        .find(|c| !c.ped_comp_expressions.is_empty())
    else {
        or_test_support::skip_corpus(
            WHAT,
            "no staged creature metadata holds a component expression",
        );
        return;
    };
    creature.to_bytes().expect("the file as staged saves");
    creature.ped_comp_expressions[0].tracks.push(0);
    let err = creature.to_bytes().expect_err("a track with no id");
    assert!(err.to_string().contains("pedCompExpressions"), "{err}");
}

#[test]
fn a_document_of_another_root_is_refused_at_open() {
    const WHAT: &str = "a_ped_document_of_another_root_is_refused_at_open";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let err = CCreatureMetaData::from_bytes(&found[0].bytes)
        .expect_err("a variation is not creature metadata");
    assert!(err.to_string().contains(CREATURE), "{err}");
}

#[test]
fn a_variation_built_here_saves_reopens_and_reaches_a_fixed_point() {
    const WHAT: &str = "a_variation_built_here_saves_reopens_and_reaches_a_fixed_point";
    let Some(library) = support::schema_or_skip(WHAT) else {
        return;
    };
    if library.resolve_any(VARIATION).is_none() {
        or_test_support::skip_schema(WHAT);
        return;
    }
    let mut info = CPedVariationInfo::default();
    info.avail_comp = vec![255; 12];
    info.avail_comp[3] = 0;
    info.b_has_drawbl_variations = true;
    info.dlc_name = HashedString::new("openrage_built");
    let mut component = CPVComponentData::default();
    component.num_avail_tex = 1;
    let mut drawable = CPVDrawblData::default();
    drawable.prop_mask = 1;
    let mut texture = CPVTextureData::default();
    texture.distribution = 255;
    drawable.a_tex_data.push(texture);
    component.a_drawbl_data3.push(drawable);
    info.a_component_data3.push(component);
    let mut component_info = CComponentInfo::default();
    component_info.ped_xml_comp_idx = 3;
    info.comp_infos.push(component_info);

    let once = info.to_bytes().expect("a variation built here saves");
    let back = CPedVariationInfo::from_bytes(&once).expect("it reopens");
    assert_eq!(back.avail_comp, info.avail_comp);
    assert_eq!(back.a_component_data3.len(), 1);
    assert_eq!(
        back.a_component_data3[0].a_drawbl_data3[0].a_tex_data[0].distribution,
        255
    );
    assert_eq!(back.comp_infos[0].ped_xml_comp_idx, 3);
    assert_eq!(back.dlc_name.hash, info.dlc_name.hash);
    assert_eq!(back.to_bytes().expect("it saves again"), once);
}

fn selection_set_lengths(set: &CPedSelectionSet) -> [usize; 5] {
    [
        set.comp_drawable_id.len(),
        set.comp_tex_id.len(),
        set.prop_anchor_id.len(),
        set.prop_drawable_id.len(),
        set.prop_tex_id.len(),
    ]
}

#[test]
fn a_selection_set_built_here_is_shaped_like_every_staged_one() {
    const WHAT: &str = "a_selection_set_built_here_is_shaped_like_every_staged_one";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let built = selection_set_lengths(&CPedSelectionSet::default());
    let (mut sets, mut failures) = (0usize, Vec::new());
    for s in &found {
        let Ok(info) = CPedVariationInfo::from_bytes(&s.bytes) else {
            continue;
        };
        for (i, set) in info.a_selection_sets.iter().enumerate() {
            sets += 1;
            if selection_set_lengths(set) != built {
                failures.push(format!(
                    "{}: set {i} holds {:?} and a built one {built:?}",
                    s.name,
                    selection_set_lengths(set)
                ));
            }
        }
    }
    if sets == 0 {
        or_test_support::skip_corpus(WHAT, "no staged variation holds a selection set");
        return;
    }
    report("selection sets", sets, &failures);
}

#[test]
fn a_selection_set_built_here_saves_into_every_staged_variation() {
    const WHAT: &str = "a_selection_set_built_here_saves_into_every_staged_variation";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    // ⚠ A record built here is conformed to the installed schema, so this needs the snapshot.
    if support::schema_or_skip(WHAT).is_none() {
        return;
    }
    let mut set = CPedSelectionSet::default();
    set.name = HashedString::new("openrage_built");
    let (mut built, mut failures) = (0usize, Vec::new());
    for s in &found {
        let Ok(mut info) = CPedVariationInfo::from_bytes(&s.bytes) else {
            continue;
        };
        built += 1;
        let held = info.a_selection_sets.len();
        info.a_selection_sets.push(set.clone());
        let saved = match info.to_bytes() {
            Ok(saved) => saved,
            Err(e) => {
                failures.push(format!("{}: the built set does not save: {e}", s.name));
                continue;
            }
        };
        match CPedVariationInfo::from_bytes(&saved) {
            Ok(back) => match back.a_selection_sets.get(held) {
                Some(last)
                    if back.a_selection_sets.len() == held + 1
                        && last.name.hash == set.name.hash
                        && selection_set_lengths(last) == selection_set_lengths(&set)
                        && last.comp_drawable_id == set.comp_drawable_id
                        && last.prop_anchor_id == set.prop_anchor_id =>
                {
                    if back.to_bytes().ok().as_ref() != Some(&saved) {
                        failures.push(format!("{}: a second save differs from the first", s.name));
                    }
                }
                _ => failures.push(format!(
                    "{}: the built set reopens as something else",
                    s.name
                )),
            },
            Err(e) => failures.push(format!("{}: the file does not reopen: {e}", s.name)),
        }
    }
    assert!(built > 0, "no staged variation opens typed");
    report(
        "variations a built selection set was added to",
        built,
        &failures,
    );
}

#[test]
fn every_staged_variation_states_how_many_textures_and_props_it_holds() {
    const WHAT: &str = "every_staged_variation_states_how_many_textures_and_props_it_holds";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    for s in &found {
        let Ok(info) = CPedVariationInfo::from_bytes(&s.bytes) else {
            continue;
        };
        for (c, component) in info.a_component_data3.iter().enumerate() {
            let textures: usize = component
                .a_drawbl_data3
                .iter()
                .map(|d| d.a_tex_data.len())
                .sum();
            if component.num_avail_tex != textures as u8 {
                failures.push(format!(
                    "{}: component {c} states {} textures and holds {textures}",
                    s.name, component.num_avail_tex
                ));
            }
        }
        let props = info.prop_info.a_prop_meta_data.len();
        if info.prop_info.num_avail_props != props as u8 {
            failures.push(format!(
                "{}: states {} props and holds {props}",
                s.name, info.prop_info.num_avail_props
            ));
        }
    }
    report("variations", found.len(), &failures);
}

#[test]
fn a_selection_sets_texture_is_255_where_its_drawable_is_none_in_most_staged_slots() {
    const WHAT: &str =
        "a_selection_sets_texture_is_255_where_its_drawable_is_none_in_most_staged_slots";
    let Some(found) = staged(VARIATION, WHAT) else {
        return;
    };
    let (mut undressed, mut texture_none, mut dressed_without_texture) = (0usize, 0usize, 0usize);
    for s in &found {
        let Ok(info) = CPedVariationInfo::from_bytes(&s.bytes) else {
            continue;
        };
        for set in &info.a_selection_sets {
            for (drawable, texture) in set.comp_drawable_id.iter().zip(&set.comp_tex_id) {
                match (*drawable == 255, *texture == 255) {
                    (true, true) => {
                        undressed += 1;
                        texture_none += 1;
                    }
                    (true, false) => undressed += 1,
                    (false, true) => dressed_without_texture += 1,
                    (false, false) => {}
                }
            }
        }
    }
    if undressed == 0 {
        or_test_support::skip_corpus(WHAT, "no staged variation holds a selection set");
        return;
    }
    eprintln!("{undressed} undressed slots, {texture_none} with texture 255");
    assert_eq!(
        dressed_without_texture, 0,
        "a dressed slot always states its texture"
    );
    assert!(
        texture_none * 2 > undressed,
        "255 is the texture of an undressed slot in {texture_none} of {undressed}"
    );
    assert_eq!(
        CPedSelectionSet::default().comp_tex_id,
        vec![255; 12],
        "a set built here keeps the majority's value"
    );
}
