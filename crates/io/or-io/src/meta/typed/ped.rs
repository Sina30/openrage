//! The ped variation family as typed roots: `.ymt` as [`CPedVariationInfo`], a
//! ped's or a clothing DLC's components, drawables, textures, props and
//! selection sets, and as [`CCreatureMetaData`], the expressions and shader
//! variables that go with them, each member found by the name hash the
//! document's own type table stores.
//!
//! ⚠ Like the world family, a record keeps the value tree it was read from;
//! `CAnchorProps::anchor` is an enum and reads and writes as the number its enumerator states ([`Anchor`]).

use super::*;

const CLOTH: &str = "CPVDrawblData::CPVClothComponentData";

/// What a slot holds when it holds nothing: `availComp` for a component with no data, and a selection set for a slot it does not dress.
const NONE: u8 = 255;
/// The component slots of a ped, and the prop slots a selection set is laid out at (`SHIPPED_SYMBOLIC_SIZES`).
const COMPONENT_SLOTS: usize = 12;
const PROP_SLOTS: usize = 6;

meta_record! {
    /// `CPVTextureData`: one texture variation of a drawable.
    CPVTextureData = "CPVTextureData" {
        tex_id: u8 = "texId",
        distribution: u8 = "distribution",
    }
}

meta_record! {
    /// `CPVDrawblData::CPVClothComponentData`: whether a drawable carries its own cloth.
    CPVClothComponentData = "CPVDrawblData::CPVClothComponentData" {
        owns_cloth: bool = "ownsCloth",
    }
}

meta_record! {
    /// `CPVDrawblData`: one drawable of a component and the textures it can wear.
    CPVDrawblData = "CPVDrawblData" {
        prop_mask: u8 = "propMask",
        num_alternatives: u8 = "numAlternatives",
        a_tex_data: Vec<CPVTextureData> = "aTexData",
        cloth_data: CPVClothComponentData = "clothData",
    }
}

meta_record! {
    /// `CPVComponentData`: the drawables of one component slot; `numAvailTex` counts their textures, in a byte that wraps at 256.
    CPVComponentData = "CPVComponentData" {
        num_avail_tex: u8 = "numAvailTex",
        a_drawbl_data3: Vec<CPVDrawblData> = "aDrawblData3",
    }
}

meta_record! {
    /// `CPedSelectionSet`: a named outfit, the drawable and texture each slot wears, 255 for none.
    CPedSelectionSet = "CPedSelectionSet" {
        name: HashedString = "name",
        comp_drawable_id: Vec<u8> = "compDrawableId" => vec![NONE; COMPONENT_SLOTS],
        comp_tex_id: Vec<u8> = "compTexId" => vec![NONE; COMPONENT_SLOTS],
        prop_anchor_id: Vec<u8> = "propAnchorId" => vec![NONE; PROP_SLOTS],
        prop_drawable_id: Vec<u8> = "propDrawableId" => vec![NONE; PROP_SLOTS],
        prop_tex_id: Vec<u8> = "propTexId" => vec![NONE; PROP_SLOTS],
    }
}

meta_record! {
    /// `CComponentInfo`: the audio, flags and hide masks of one drawable, named by its component slot and index.
    CComponentInfo = "CComponentInfo" {
        ped_xml_audio_id: HashedString = "pedXml_audioID" => HashedString::new("NONE"),
        ped_xml_audio_id2: HashedString = "pedXml_audioID2" => HashedString::new("NONE"),
        ped_xml_expression_mods: Vec<f32> = "pedXml_expressionMods" => vec![0.0; 5],
        flags: u32 = "flags",
        inclusions: u32 = "inclusions",
        exclusions: u32 = "exclusions",
        ped_xml_vfx_comps: u16 = "pedXml_vfxComps",
        ped_xml_flags: u16 = "pedXml_flags",
        ped_xml_comp_idx: u8 = "pedXml_compIdx",
        ped_xml_drawbl_idx: u8 = "pedXml_drawblIdx",
    }
}

meta_record! {
    /// `CPedPropTexData`: one texture variation of a prop.
    CPedPropTexData = "CPedPropTexData" {
        inclusions: u32 = "inclusions",
        exclusions: u32 = "exclusions",
        tex_id: u8 = "texId",
        inclusion_id: u8 = "inclusionId",
        exclusion_id: u8 = "exclusionId",
        distribution: u8 = "distribution",
    }
}

meta_record! {
    /// `CPedPropMetaData`: one prop's audio, flags, anchor and textures.
    CPedPropMetaData = "CPedPropMetaData" {
        audio_id: HashedString = "audioId" => HashedString::new("NONE"),
        expression_mods: Vec<f32> = "expressionMods" => vec![0.0; 5],
        tex_data: Vec<CPedPropTexData> = "texData",
        render_flags: u8 = "renderFlags",
        prop_flags: u32 = "propFlags",
        flags: u16 = "flags",
        anchor_id: u8 = "anchorId",
        prop_id: u8 = "propId",
        stickyness: u8 = "stickyness",
    }
}

/// One of the engine's `eAnchorPoints`, as the number its enumerator states (`ANCHOR_HEAD` is 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Anchor(pub u8);

const ANCHORS: &str = "eAnchorPoints";

impl Anchor {
    /// The enumerator's name, for the numbers the family's enum states.
    pub fn name(self) -> Option<&'static str> {
        ped_types::declared()
            .enum_(ANCHORS)?
            .values
            .iter()
            .find(|v| v.value == Some(i64::from(self.0)))
            .map(|v| v.name.as_str())
    }

    /// The number a variant spells: its name, the `hash_XXXXXXXX` a reader spells a name it cannot resolve, or the `0xXXXXXXXX` it spells a value its table does not list.
    fn from_variant(variant: &str) -> Option<Self> {
        let values = &ped_types::declared().enum_(ANCHORS)?.values;
        let named = |hash: u32| values.iter().find(|v| name_hash(&v.name) == hash);
        let found = if let Some(value) = crate::meta::binary::enum_text::value_literal(variant) {
            u8::try_from(value).ok()
        } else if let Some(hash) = crate::meta::binary::hash_index::spelled_hash(variant) {
            named(hash).and_then(|v| u8::try_from(v.value?).ok())
        } else {
            named(name_hash(variant)).and_then(|v| u8::try_from(v.value?).ok())
        };
        found.map(Self)
    }
}

impl Member for Anchor {
    fn read(value: &Value) -> Result<Self, String> {
        match value {
            Value::Enum { variant, .. } => Self::from_variant(variant)
                .ok_or_else(|| format!("`{variant}` is not an {ANCHORS} the family declares")),
            other => expected("an enum", other),
        }
    }

    fn write(&self, slot: &mut Value) -> Result<(), String> {
        if Self::read(slot).ok() == Some(*self) {
            return Ok(());
        }
        let name = self
            .name()
            .ok_or_else(|| format!("{} is not an {ANCHORS} the family declares", self.0))?;
        *slot = Value::Enum {
            type_name: ANCHORS.to_string(),
            variant: name.to_string(),
        };
        Ok(())
    }
}

meta_record! {
    /// `CAnchorProps`: the props that attach at one anchor point, each as the number of textures it holds.
    CAnchorProps = "CAnchorProps" {
        props: Vec<u8> = "props",
        anchor: Anchor = "anchor",
    }
}

meta_record! {
    /// `CPedPropInfo`: a ped's props and the anchors they attach at.
    CPedPropInfo = "CPedPropInfo" {
        num_avail_props: u8 = "numAvailProps",
        a_prop_meta_data: Vec<CPedPropMetaData> = "aPropMetaData",
        a_anchors: Vec<CAnchorProps> = "aAnchors",
    }
}

meta_record! {
    /// `CPedVariationInfo`: a ped's `.ymt`; `availComp` maps each of the twelve component slots to its `aComponentData3` entry, or 255 for none.
    CPedVariationInfo = "CPedVariationInfo" {
        b_has_tex_variations: bool = "bHasTexVariations",
        b_has_drawbl_variations: bool = "bHasDrawblVariations",
        b_has_low_lods: bool = "bHasLowLODs",
        b_is_super_lod: bool = "bIsSuperLOD",
        avail_comp: Vec<u8> = "availComp" => vec![NONE; COMPONENT_SLOTS],
        a_component_data3: Vec<CPVComponentData> = "aComponentData3",
        a_selection_sets: Vec<CPedSelectionSet> = "aSelectionSets",
        comp_infos: Vec<CComponentInfo> = "compInfos",
        prop_info: CPedPropInfo = "propInfo",
        dlc_name: HashedString = "dlcName",
    }
}

meta_record! {
    /// `CShaderVariableComponent`: the tracks that drive one shader variable of a component.
    CShaderVariableComponent = "CShaderVariableComponent" {
        pedcomp_id: u32 = "pedcompID",
        mask_id: u32 = "maskID",
        shader_variable_hash_string: HashedString = "shaderVariableHashString",
        tracks: Vec<u8> = "tracks",
        ids: Vec<u16> = "ids",
        components: Vec<u8> = "components",
    }
}

meta_record! {
    /// `CPedPropExpressionData`: the tracks that drive an expression of one prop variation.
    CPedPropExpressionData = "CPedPropExpressionData" {
        ped_prop_id: u32 = "pedPropID",
        ped_prop_var_index: i32 = "pedPropVarIndex",
        ped_prop_expression_index: u32 = "pedPropExpressionIndex",
        tracks: Vec<u8> = "tracks",
        ids: Vec<u16> = "ids",
        types: Vec<u8> = "types",
        components: Vec<u8> = "components",
    }
}

meta_record! {
    /// `CPedCompExpressionData`: the tracks that drive an expression of one component variation.
    CPedCompExpressionData = "CPedCompExpressionData" {
        ped_comp_id: u32 = "pedCompID",
        ped_comp_var_index: i32 = "pedCompVarIndex",
        ped_comp_expression_index: u32 = "pedCompExpressionIndex",
        tracks: Vec<u8> = "tracks",
        ids: Vec<u16> = "ids",
        types: Vec<u8> = "types",
        components: Vec<u8> = "components",
    }
}

meta_record! {
    /// `CCreatureMetaData`: a ped's expression and shader-variable tracks, a `.ymt` of its own.
    CCreatureMetaData = "CCreatureMetaData" {
        shader_variable_components: Vec<CShaderVariableComponent> = "shaderVariableComponents",
        ped_prop_expressions: Vec<CPedPropExpressionData> = "pedPropExpressions",
        ped_comp_expressions: Vec<CPedCompExpressionData> = "pedCompExpressions",
    }
}

meta_origin!(CPedVariationInfo);
meta_origin!(CCreatureMetaData);

impl CPedVariationInfo {
    /// Built here or moved from another type, so no arrival layout has its shape.
    fn reshaped(&self) -> bool {
        built_here(&self.carry, "CPedVariationInfo")
            || self.a_component_data3.iter().any(|c| {
                built_here(&c.carry, "CPVComponentData")
                    || c.a_drawbl_data3.iter().any(|d| {
                        built_here(&d.carry, "CPVDrawblData")
                            || built_here(&d.cloth_data.carry, CLOTH)
                            || d.a_tex_data
                                .iter()
                                .any(|t| built_here(&t.carry, "CPVTextureData"))
                    })
            })
            || self
                .a_selection_sets
                .iter()
                .any(|s| built_here(&s.carry, "CPedSelectionSet"))
            || self
                .comp_infos
                .iter()
                .any(|i| built_here(&i.carry, "CComponentInfo"))
            || built_here(&self.prop_info.carry, "CPedPropInfo")
            || self.prop_info.a_prop_meta_data.iter().any(|p| {
                built_here(&p.carry, "CPedPropMetaData")
                    || p.tex_data
                        .iter()
                        .any(|t| built_here(&t.carry, "CPedPropTexData"))
            })
            || self
                .prop_info
                .a_anchors
                .iter()
                .any(|a| built_here(&a.carry, "CAnchorProps"))
    }

    /// The texture count each component's `numAvailTex` states, and the prop count `numAvailProps` states: a byte that wraps at 256, and one every staged variation holds to its own arrays.
    fn stated_counts(&self) -> (Vec<u8>, u8) {
        let textures = self.a_component_data3.iter().map(|c| {
            let held: usize = c.a_drawbl_data3.iter().map(|d| d.a_tex_data.len()).sum();
            (held % 256) as u8
        });
        (
            textures.collect(),
            (self.prop_info.a_prop_meta_data.len() % 256) as u8,
        )
    }

    /// States the counts the arrays hold, for a record whose arrays were edited.
    pub fn restate_counts(&mut self) {
        let (textures, props) = self.stated_counts();
        for (component, held) in self.a_component_data3.iter_mut().zip(textures) {
            component.num_avail_tex = held;
        }
        self.prop_info.num_avail_props = props;
    }

    /// ⚠ An index the engine trusts: every staged variation names only data
    /// and drawables it holds, and a slot past them is read out of bounds.
    fn broken(&self) -> Option<Broken> {
        let (textures, props) = self.stated_counts();
        if let Some((component, (stated, held))) = self
            .a_component_data3
            .iter()
            .zip(&textures)
            .map(|(c, held)| (c.num_avail_tex, *held))
            .enumerate()
            .find(|(_, (stated, held))| stated != held)
        {
            return Some(Broken::TextureCountDiffers {
                component,
                stated,
                held,
            });
        }
        if self.prop_info.num_avail_props != props {
            return Some(Broken::PropCountDiffers {
                stated: self.prop_info.num_avail_props,
                held: props,
            });
        }
        let len = self.a_component_data3.len();
        if let Some((slot, &index)) = self
            .avail_comp
            .iter()
            .enumerate()
            .find(|(_, &i)| i != 255 && usize::from(i) >= len)
        {
            return Some(Broken::ComponentSlotOutOfRange { slot, index, len });
        }
        self.comp_infos.iter().enumerate().find_map(|(info, i)| {
            let (slot, drawable) = (i.ped_xml_comp_idx, i.ped_xml_drawbl_idx);
            let held = self
                .avail_comp
                .get(usize::from(slot))
                .filter(|&&c| c != 255)
                .and_then(|&c| self.a_component_data3.get(usize::from(c)))
                .is_some_and(|c| usize::from(drawable) < c.a_drawbl_data3.len());
            (!held).then_some(Broken::ComponentInfoNamesNothing {
                info,
                slot,
                drawable,
            })
        })
    }
}

impl CCreatureMetaData {
    fn reshaped(&self) -> bool {
        built_here(&self.carry, "CCreatureMetaData")
            || self
                .shader_variable_components
                .iter()
                .any(|s| built_here(&s.carry, "CShaderVariableComponent"))
            || self
                .ped_prop_expressions
                .iter()
                .any(|e| built_here(&e.carry, "CPedPropExpressionData"))
            || self
                .ped_comp_expressions
                .iter()
                .any(|e| built_here(&e.carry, "CPedCompExpressionData"))
    }

    /// ⚠ Parallel arrays the engine walks by one index: every staged entry
    /// holds them at one length.
    fn broken(&self) -> Option<Broken> {
        fn same(lengths: &[usize]) -> bool {
            lengths.windows(2).all(|w| w[0] == w[1])
        }
        let shaders = self
            .shader_variable_components
            .iter()
            .position(|s| !same(&[s.tracks.len(), s.ids.len(), s.components.len()]))
            .map(|entry| ("shaderVariableComponents", entry));
        let props = self
            .ped_prop_expressions
            .iter()
            .position(|e| {
                !same(&[
                    e.tracks.len(),
                    e.ids.len(),
                    e.types.len(),
                    e.components.len(),
                ])
            })
            .map(|entry| ("pedPropExpressions", entry));
        let comps = self
            .ped_comp_expressions
            .iter()
            .position(|e| {
                !same(&[
                    e.tracks.len(),
                    e.ids.len(),
                    e.types.len(),
                    e.components.len(),
                ])
            })
            .map(|entry| ("pedCompExpressions", entry));
        shaders
            .or(props)
            .or(comps)
            .map(|(list, entry)| Broken::ExpressionArraysDiffer { list, entry })
    }
}

/// ⚠ RBF is the tags of creature metadata, which `ped_rbf` reads; a variation is never one.
fn refuse_rbf(bytes: &[u8]) -> Result<(), Error> {
    match crate::meta::detect_binary_format(bytes) {
        Some(BinaryFormat::Rbf) => Err(Error::Read {
            ext: "ymt",
            why: "an RBF document holds the tags of creature metadata, and a ped variation is never one".into(),
        }),
        _ => Ok(()),
    }
}

impl File for CPedVariationInfo {
    const EXTENSION: &'static str = "ymt";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        refuse_rbf(bytes)?;
        let (mut root, arrival) =
            open_root(Self::EXTENSION, "CPedVariationInfo", bytes, Self::read)?;
        root.carry.arrival = Some(Box::new(arrival));
        Ok(root)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        save_root(
            Self::EXTENSION,
            self.broken(),
            self.write(),
            &self.carry,
            self.reshaped(),
        )
    }
}

impl File for CCreatureMetaData {
    const EXTENSION: &'static str = "ymt";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if !matches!(
            crate::meta::detect_binary_format(bytes),
            Some(BinaryFormat::Rbf)
        ) {
            let (mut root, arrival) =
                open_root(Self::EXTENSION, "CCreatureMetaData", bytes, Self::read)?;
            root.carry.arrival = Some(Box::new(arrival));
            return Ok(root);
        }
        // RBF holds tags and no types: its tags are read into the tree the typed members read, and kept to write them back into.
        let read = |why: String| Error::Read {
            ext: Self::EXTENSION,
            why,
        };
        let doc = MetaDocument::parse(crate::meta::default_library(), bytes.to_vec())
            .map_err(|e| read(e.to_string()))?;
        if !is_named(&doc.instance.type_name, "CCreatureMetaData") {
            return Err(Error::WrongType {
                expected: "CCreatureMetaData".to_string(),
                found: doc.instance.type_name,
            });
        }
        let typed = ped_rbf::to_typed(&doc.instance, "CCreatureMetaData").map_err(read)?;
        let mut root = Self::read(&typed).map_err(read)?;
        root.carry.tags = Some(Box::new(doc.instance));
        root.carry.arrival = Some(Box::new(Arrival {
            layout: doc.layout,
            format: doc.source_format,
        }));
        Ok(root)
    }

    /// ⚠ A creature record built here is written as RBF, which is what 64 of the 73 staged documents are.
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let rbf = self
            .carry
            .arrival
            .as_deref()
            .is_none_or(|a| a.format == SourceFormat::Rbf);
        let root = self.write().and_then(|typed| match rbf {
            true => ped_rbf::to_tags(&typed, "CCreatureMetaData", self.carry.tags.as_deref()),
            false => Ok(typed),
        });
        let fallback = rbf.then(ped_rbf::default_arrival);
        save_root_as(
            Self::EXTENSION,
            self.broken(),
            root,
            &self.carry,
            self.reshaped(),
            fallback.as_ref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(type_name: &str, members: Vec<(&str, Value)>) -> MetaInstance {
        let mut inst = MetaInstance::new(type_name);
        for (name, value) in members {
            inst.set(name, value);
        }
        inst
    }

    fn strukt(type_name: &str, members: Vec<(&str, Value)>) -> Value {
        Value::Struct(Box::new(inst(type_name, members)))
    }

    /// A variation with one component slot holding one drawable of one texture.
    fn variation() -> MetaInstance {
        let texture = strukt(
            "CPVTextureData",
            vec![
                ("texId", Value::UInt(4)),
                ("distribution", Value::UInt(255)),
            ],
        );
        let drawable = strukt(
            "CPVDrawblData",
            vec![
                ("propMask", Value::UInt(3)),
                ("numAlternatives", Value::UInt(1)),
                ("aTexData", Value::Array(vec![texture])),
                (
                    "clothData",
                    strukt(CLOTH, vec![("ownsCloth", Value::Bool(true))]),
                ),
            ],
        );
        let component = strukt(
            "CPVComponentData",
            vec![
                ("numAvailTex", Value::UInt(1)),
                ("aDrawblData3", Value::Array(vec![drawable])),
            ],
        );
        let mut avail = vec![Value::UInt(255); 12];
        avail[3] = Value::UInt(0);
        inst(
            "CPedVariationInfo",
            vec![
                ("bHasDrawblVariations", Value::Bool(true)),
                ("availComp", Value::Array(avail)),
                ("aComponentData3", Value::Array(vec![component])),
            ],
        )
    }

    #[test]
    fn a_tree_reads_each_member_by_its_engine_name_and_a_pushed_element_reaches_it() {
        let mut info = CPedVariationInfo::read(&variation()).unwrap();
        assert!(info.b_has_drawbl_variations && !info.b_has_tex_variations);
        assert_eq!(info.avail_comp[3], 0);
        let drawable = &info.a_component_data3[0].a_drawbl_data3[0];
        assert_eq!((drawable.prop_mask, drawable.num_alternatives), (3, 1));
        assert_eq!(drawable.a_tex_data[0].distribution, 255);
        assert!(drawable.cloth_data.owns_cloth);

        let copy = drawable.clone();
        info.a_component_data3[0].a_drawbl_data3.push(copy);
        let written = info.write().unwrap();
        let Some(Value::Array(components)) = written.get("aComponentData3") else {
            panic!("the components were not written");
        };
        let Value::Struct(component) = &components[0] else {
            panic!("a component is a struct");
        };
        let Some(Value::Array(drawables)) = component.get("aDrawblData3") else {
            panic!("the drawables were not written");
        };
        assert_eq!(drawables.len(), 2);
        assert_eq!(drawables[0], drawables[1]);
        assert_eq!(CPedVariationInfo::read(&written).unwrap(), info);
    }

    #[test]
    fn a_member_the_tree_lacks_reads_as_the_engines_init() {
        let info = CPedVariationInfo::read(&MetaInstance::new("CPedVariationInfo")).unwrap();
        assert_eq!(info.avail_comp, vec![255; 12]);
        assert_eq!(info, CPedVariationInfo::default());
        let component_info = CComponentInfo::default();
        assert_eq!(component_info.ped_xml_audio_id, "NONE");
        assert_eq!(component_info.ped_xml_expression_mods, vec![0.0; 5]);
    }

    #[test]
    fn a_selection_set_built_here_dresses_no_slot() {
        use crate::meta::binary::synth::SHIPPED_SYMBOLIC_SIZES;
        let prop_slots = |member: &str| {
            let (.., count) = SHIPPED_SYMBOLIC_SIZES
                .iter()
                .find(|(owner, name, _)| *owner == "CPedSelectionSet" && *name == member)
                .expect("the type tables size the prop arrays");
            *count as usize
        };
        let set = CPedSelectionSet::default();
        let slots = [
            (&set.comp_drawable_id, COMPONENT_SLOTS),
            (&set.comp_tex_id, COMPONENT_SLOTS),
            (&set.prop_anchor_id, prop_slots("propAnchorId")),
            (&set.prop_drawable_id, prop_slots("propDrawableId")),
            (&set.prop_tex_id, prop_slots("propTexId")),
        ];
        for (held, expected) in slots {
            assert_eq!(held, &vec![255; expected]);
        }
    }

    #[test]
    fn without_a_snapshot_an_array_grows_by_a_copy_and_a_record_built_here_saves() {
        assert!(
            !crate::meta::has_default_library(),
            "this test needs no snapshot"
        );
        let mut info = CPedVariationInfo::read(&variation()).unwrap();
        let drawables = &mut info.a_component_data3[0].a_drawbl_data3;
        let copy = drawables[0].clone();
        drawables.push(copy);
        info.restate_counts();
        info.write()
            .expect("a copy of a held element needs no schema");

        // ⚠ Built from nothing: the family's types are declared in `ped_types`, so no snapshot is wanted.
        let mut built = CPedVariationInfo::default();
        built.avail_comp[3] = 0;
        built.b_has_drawbl_variations = true;
        built.dlc_name = HashedString::new("openrage_built");
        let mut drawable = CPVDrawblData {
            prop_mask: 1,
            ..Default::default()
        };
        drawable.a_tex_data.push(CPVTextureData {
            distribution: 255,
            ..Default::default()
        });
        let mut component = CPVComponentData::default();
        component.a_drawbl_data3.push(drawable);
        built.a_component_data3.push(component);
        built.comp_infos.push(CComponentInfo {
            ped_xml_comp_idx: 3,
            ..Default::default()
        });
        built
            .prop_info
            .a_prop_meta_data
            .push(CPedPropMetaData::default());
        built.prop_info.a_anchors.push(CAnchorProps {
            props: vec![0],
            anchor: Anchor(6),
            ..Default::default()
        });
        built.restate_counts();

        let once = built.to_bytes().expect("a variation built here saves");
        let back = CPedVariationInfo::from_bytes(&once).expect("it reopens");
        assert_eq!(back.avail_comp, built.avail_comp);
        assert_eq!(back.a_component_data3.len(), 1);
        assert_eq!(back.a_component_data3[0].num_avail_tex, 1);
        assert_eq!(back.prop_info.num_avail_props, 1);
        assert_eq!(back.prop_info.a_anchors[0].anchor, Anchor(6));
        assert_eq!(back.dlc_name.hash, built.dlc_name.hash);
        assert_eq!(back.to_bytes().expect("it saves again"), once);
    }

    #[test]
    fn a_count_the_arrays_do_not_bear_out_is_refused_before_any_byte_is_written() {
        let mut info = CPedVariationInfo::read(&variation()).unwrap();
        assert!(info.broken().is_none(), "one texture, stated as one");
        info.a_component_data3[0].a_drawbl_data3[0]
            .a_tex_data
            .push(CPVTextureData::default());
        let err = info.to_bytes().expect_err("two textures, stated as one");
        assert!(err.to_string().contains("numAvailTex"), "{err}");

        info.restate_counts();
        assert_eq!(info.a_component_data3[0].num_avail_tex, 2);
        assert!(info.broken().is_none());

        info.prop_info
            .a_prop_meta_data
            .push(CPedPropMetaData::default());
        let err = info.to_bytes().expect_err("one prop, stated as none");
        assert!(err.to_string().contains("numAvailProps"), "{err}");
        info.restate_counts();
        assert_eq!(info.prop_info.num_avail_props, 1);
    }

    #[test]
    fn the_texture_count_is_a_byte_that_wraps_at_256() {
        let mut info = CPedVariationInfo::read(&variation()).unwrap();
        info.a_component_data3[0].a_drawbl_data3[0].a_tex_data =
            vec![CPVTextureData::default(); 257];
        assert!(
            info.broken().is_none(),
            "257 textures, stated as one: 257 mod 256"
        );
        info.a_component_data3[0].a_drawbl_data3[0].a_tex_data =
            vec![CPVTextureData::default(); 256];
        let err = info.to_bytes().expect_err("256 textures, stated as one");
        assert!(err.to_string().contains("numAvailTex"), "{err}");
        info.restate_counts();
        assert_eq!(info.a_component_data3[0].num_avail_tex, 0, "256 mod 256");
        assert!(info.broken().is_none());
    }

    #[test]
    fn an_anchor_reads_by_its_name_its_hash_spelling_or_its_value_and_writes_its_name() {
        let named = |variant: &str| Value::Enum {
            type_name: "eAnchorPoints".into(),
            variant: variant.into(),
        };
        let hash = format!("hash_{:08X}", name_hash("ANCHOR_RIGHT_WRIST"));
        for (spelling, expected) in [("ANCHOR_HEAD", 0), (hash.as_str(), 7), ("0x00000008", 8)] {
            assert_eq!(
                <Anchor as Member>::read(&named(spelling)),
                Ok(Anchor(expected)),
                "{spelling}"
            );
        }
        assert!(<Anchor as Member>::read(&named("ANCHOR_NOWHERE")).is_err());

        let mut slot = named("hash_00000000");
        Anchor(6).write(&mut slot).unwrap();
        assert_eq!(slot, named("ANCHOR_LEFT_WRIST"));
        let before = slot.clone();
        Anchor(6).write(&mut slot).unwrap();
        assert_eq!(
            slot, before,
            "an unchanged anchor leaves its slot as it was"
        );
        assert!(Anchor(99).write(&mut slot).is_err());
    }

    #[test]
    fn a_slot_naming_data_the_file_lacks_is_refused_before_any_byte_is_written() {
        let mut info = CPedVariationInfo::read(&variation()).unwrap();
        assert!(info.broken().is_none());
        info.avail_comp[5] = 1;
        let err = info.to_bytes().expect_err("data 1 of 1");
        assert!(err.to_string().contains("availComp"), "{err}");
    }

    #[test]
    fn a_component_info_naming_no_drawable_is_refused_before_any_byte_is_written() {
        let mut info = CPedVariationInfo::read(&variation()).unwrap();
        info.comp_infos.push(CComponentInfo {
            ped_xml_comp_idx: 3,
            ..Default::default()
        });
        assert!(info.broken().is_none(), "slot 3, drawable 0 is held");
        info.comp_infos[0].ped_xml_drawbl_idx = 1;
        let err = info.to_bytes().expect_err("drawable 1 of 1");
        assert!(err.to_string().contains("compInfos"), "{err}");
        info.comp_infos[0].ped_xml_drawbl_idx = 0;
        info.comp_infos[0].ped_xml_comp_idx = 4;
        assert!(info.broken().is_some(), "slot 4 names no data");
    }

    #[test]
    fn parallel_expression_arrays_of_one_length_are_the_only_ones_saved() {
        let mut creature = CCreatureMetaData::default();
        creature.ped_comp_expressions.push(CPedCompExpressionData {
            tracks: vec![33],
            ids: vec![28462],
            types: vec![2],
            components: vec![1],
            ..Default::default()
        });
        assert!(creature.broken().is_none());
        creature.ped_comp_expressions[0].ids.push(1);
        let err = creature.to_bytes().expect_err("an id with no track");
        assert!(err.to_string().contains("pedCompExpressions"), "{err}");
    }

    #[test]
    fn an_rbf_document_is_no_variation_and_a_truncated_one_is_no_creature() {
        let bytes = b"RBF0".to_vec();
        let err = CPedVariationInfo::from_bytes(&bytes).expect_err("RBF");
        assert!(err.to_string().contains("RBF"), "{err}");
        assert!(
            CCreatureMetaData::from_bytes(&bytes).is_err(),
            "no root tag"
        );
    }
}
