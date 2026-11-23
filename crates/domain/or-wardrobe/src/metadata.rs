//! The metadata a pack writes: each ped's variation `.ymt`, the creature metadata the peds share and
//! each ped's shop metadata, derived from the model and the places the allocator gave.
//!
//! Each rule below was measured on the staged documents (`typed_ped_schema` and the checks of this
//! crate hold them): a ped's components take `aComponentData3` in the order of their slot numbers,
//! a drawable has a component info and a component info only, the props are sorted by anchor and
//! position, and each anchor lists how many textures each of its props holds.

use std::collections::BTreeMap;

use or_io::meta::instance::HashedString;
use or_io::meta::{
    Anchor, CAnchorProps, CComponentInfo, CCreatureMetaData, CPVComponentData, CPVDrawblData,
    CPVTextureData, CPedCompExpressionData, CPedPropExpressionData, CPedPropMetaData,
    CPedPropTexData, CPedVariationInfo,
};

use crate::alloc::{Allocation, Placed};
use crate::layout;
use crate::mask;
use crate::model::{Expression, Garment, Meta, Variation, Wardrobe};
use crate::names::{Component, Ped, Prop, Skin, Slot};
use crate::shop;

/// A hashed string from the text a model keeps it as: a name, or `hash_XXXXXXXX` for one whose name nobody has.
pub fn hashed(text: &str) -> HashedString {
    match text
        .strip_prefix("hash_")
        .filter(|h| h.len() == 8)
        .and_then(|h| u32::from_str_radix(h, 16).ok())
    {
        Some(hash) => HashedString::from_hash(hash),
        None => HashedString::new(text),
    }
}

/// The text a model keeps a hashed string as, the inverse of [`hashed`].
pub fn text_of(value: &HashedString) -> String {
    if !value.raw.is_empty() && or_io::string_hash(value.raw.as_bytes()) == value.hash {
        value.raw.clone()
    } else if value.hash == 0 {
        String::new()
    } else {
        format!("hash_{:08X}", value.hash)
    }
}

fn mods(held: Option<[f32; 5]>) -> Vec<f32> {
    held.unwrap_or([0.0; 5]).to_vec()
}

fn texture(v: &Variation, skin: Skin) -> CPVTextureData {
    let mut t = CPVTextureData::default();
    t.tex_id = v.tex_id.unwrap_or(skin.tex_id());
    t.distribution = v.distribution.unwrap_or(255);
    t
}

fn drawable(g: Option<&Garment>, alternates: bool) -> CPVDrawblData {
    let none = Meta::default();
    let (skin, meta, variations) = match g {
        Some(g) => (g.skin, &g.meta, g.variations.as_slice()),
        None => (Skin::Universal, &none, &[][..]),
    };
    let mut d = CPVDrawblData::default();
    d.prop_mask = meta.prop_mask.unwrap_or_else(|| mask::default_for(skin));
    // The alternates a drawable has are what `numAlternatives` counts.
    let held = g.map_or(0, |g| u8::try_from(g.alternates.len()).unwrap_or(u8::MAX));
    d.num_alternatives = match alternates {
        true => meta.num_alternatives.unwrap_or(held),
        false => 0,
    };
    // A garment has cloth when one is linked; one an import found stated and no file for still states it.
    d.cloth_data.owns_cloth =
        g.is_some_and(|g| g.missing_cloth || (g.cloth.is_some() && g.model.is_some()));
    d.a_tex_data = variations.iter().map(|v| texture(v, skin)).collect();
    // A place held, or a hole between two garments, still wears a texture.
    if d.a_tex_data.is_empty() && g.is_none_or(|g| g.is_reserved()) {
        d.a_tex_data.push(texture(&Variation::default(), skin));
    }
    d
}

fn component_info(g: Option<&Garment>, slot: u8, drawable: u32) -> CComponentInfo {
    let none = Meta::default();
    let meta = g.map_or(&none, |g| &g.meta);
    let mut i = CComponentInfo::default();
    i.ped_xml_audio_id = hashed(meta.ped_xml_audio_id.as_deref().unwrap_or("NONE"));
    i.ped_xml_audio_id2 = hashed(meta.ped_xml_audio_id2.as_deref().unwrap_or("NONE"));
    i.ped_xml_expression_mods = mods(meta.ped_xml_expression_mods);
    i.flags = meta.flags.unwrap_or(0);
    i.inclusions = meta.inclusions.unwrap_or(0);
    i.exclusions = meta.exclusions.unwrap_or(0);
    i.ped_xml_vfx_comps = meta.ped_xml_vfx_comps.unwrap_or(0);
    i.ped_xml_flags = meta.ped_xml_flags.unwrap_or(0);
    i.ped_xml_comp_idx = slot;
    i.ped_xml_drawbl_idx = u8::try_from(drawable).unwrap_or(u8::MAX);
    i
}

fn prop_meta(g: Option<&Garment>, prop: Prop, position: u32) -> CPedPropMetaData {
    let none = Meta::default();
    let meta = g.map_or(&none, |g| &g.meta);
    let mut p = CPedPropMetaData::default();
    p.audio_id = hashed(meta.audio_id.as_deref().unwrap_or("NONE"));
    p.expression_mods = mods(meta.expression_mods);
    let variations = g.map_or(&[][..], |g| g.variations.as_slice());
    p.tex_data = variations
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut t = CPedPropTexData::default();
            // A prop's textures are numbered in the order it holds them.
            t.tex_id = v.tex_id.unwrap_or(u8::try_from(i).unwrap_or(u8::MAX));
            t.distribution = v.distribution.unwrap_or(255);
            t.inclusions = v.inclusions.unwrap_or(0);
            t.exclusions = v.exclusions.unwrap_or(0);
            t.inclusion_id = v.inclusion_id.unwrap_or(0);
            t.exclusion_id = v.exclusion_id.unwrap_or(0);
            t
        })
        .collect();
    if p.tex_data.is_empty() {
        let mut t = CPedPropTexData::default();
        t.distribution = 255;
        p.tex_data.push(t);
    }
    p.render_flags = meta.render_flags.unwrap_or(0);
    p.prop_flags = meta.prop_flags.unwrap_or(0);
    p.flags = meta
        .flags
        .map_or(0, |f| u16::try_from(f).unwrap_or(u16::MAX));
    p.anchor_id = prop.anchor();
    p.prop_id = u8::try_from(position).unwrap_or(u8::MAX);
    p.stickyness = meta.stickyness.unwrap_or(0);
    p
}

fn at<'a>(w: &'a Wardrobe, placed: &[&Placed], position: u32) -> Option<&'a Garment> {
    placed
        .iter()
        .find(|p| p.position == position)
        .map(|p| &w.garments[p.garment])
}

/// The variation metadata of `ped`: a drawable for every position up to the last garment of a slot, and a place held at each hole.
pub fn variation_info(wardrobe: &Wardrobe, allocation: &Allocation, ped: Ped) -> CPedVariationInfo {
    variation_info_with(wardrobe, allocation, ped, true)
}

/// [`variation_info`], stating no alternate models when `alternates` is off.
pub fn variation_info_with(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    ped: Ped,
    alternates: bool,
) -> CPedVariationInfo {
    variation_info_of(wardrobe, allocation, (ped, 0), alternates)
}

/// The variation metadata of one collection of `ped`: `part` 0 is the pack's own and n is the n-th a slot past its cap is cut into.
pub fn variation_info_of(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    (ped, part): (Ped, u32),
    alternates: bool,
) -> CPedVariationInfo {
    let mut info = CPedVariationInfo::default();
    info.b_has_tex_variations = true;
    info.b_has_drawbl_variations = true;
    info.dlc_name = hashed(&wardrobe.collection_of(ped, part));
    let mut next = 0u8;
    for component in Component::ALL {
        let placed: Vec<&Placed> = allocation
            .in_part(ped, part, Slot::Component(component))
            .collect();
        let Some(last) = placed.last() else {
            continue;
        };
        info.avail_comp[usize::from(component.id())] = next;
        next += 1;
        let mut data = CPVComponentData::default();
        for position in 0..=last.position {
            let garment = at(wardrobe, &placed, position);
            data.a_drawbl_data3.push(drawable(garment, alternates));
            info.comp_infos
                .push(component_info(garment, component.id(), position));
        }
        info.a_component_data3.push(data);
    }
    for prop in Prop::ALL {
        let placed: Vec<&Placed> = allocation.in_part(ped, part, Slot::Prop(prop)).collect();
        let Some(last) = placed.last() else {
            continue;
        };
        let mut anchor = CAnchorProps::default();
        anchor.anchor = Anchor(prop.anchor());
        for position in 0..=last.position {
            let meta = prop_meta(at(wardrobe, &placed, position), prop, position);
            anchor
                .props
                .push(u8::try_from(meta.tex_data.len()).unwrap_or(u8::MAX));
            info.prop_info.a_prop_meta_data.push(meta);
        }
        info.prop_info.a_anchors.push(anchor);
    }
    info.restate_counts();
    info
}

/// The creature metadata of the pack, none where no garment states an expression.
///
/// ⚠ One file serves both peds, so an entry is a slot and a position and not a garment: two
/// garments at one place state the same expression or the rules refuse the pack (`creature-clash`).
pub fn creature(wardrobe: &Wardrobe, allocation: &Allocation) -> Option<CCreatureMetaData> {
    creature_of(wardrobe, allocation, 0)
}

/// The creature metadata of collection `part` of the pack: each collection has a file of its own, since an entry is a drawable of a collection.
pub fn creature_of(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    part: u32,
) -> Option<CCreatureMetaData> {
    type Key = (u8, u32, u32);
    let mut comps: BTreeMap<Key, &Expression> = BTreeMap::new();
    let mut props: BTreeMap<Key, &Expression> = BTreeMap::new();
    for p in allocation.placed.iter().filter(|p| p.part == part) {
        for e in &wardrobe.garments[p.garment].expressions {
            match p.slot {
                Slot::Component(c) => comps.insert((c.id(), p.position, e.index), e),
                Slot::Prop(r) => props.insert((r.anchor(), p.position, e.index), e),
            };
        }
    }
    if comps.is_empty() && props.is_empty() {
        return None;
    }
    let mut creature = CCreatureMetaData::default();
    for ((slot, position, index), e) in comps {
        let mut r = CPedCompExpressionData::default();
        r.ped_comp_id = u32::from(slot);
        r.ped_comp_var_index = i32::try_from(position).unwrap_or(i32::MAX);
        r.ped_comp_expression_index = index;
        (r.tracks, r.ids, r.types, r.components) = (
            e.tracks.clone(),
            e.ids.clone(),
            e.types.clone(),
            e.components.clone(),
        );
        creature.ped_comp_expressions.push(r);
    }
    for ((slot, position, index), e) in props {
        let mut r = CPedPropExpressionData::default();
        r.ped_prop_id = u32::from(slot);
        r.ped_prop_var_index = i32::try_from(position).unwrap_or(i32::MAX);
        r.ped_prop_expression_index = index;
        (r.tracks, r.ids, r.types, r.components) = (
            e.tracks.clone(),
            e.ids.clone(),
            e.types.clone(),
            e.components.clone(),
        );
        creature.ped_prop_expressions.push(r);
    }
    Some(creature)
}

/// A name the game hashes for one shop entry: Durty Cloth Tool's when the pack asks, else the collection, the slot, the position and the texture.
pub(crate) fn shop_name(wardrobe: &Wardrobe, placed: &Placed, texture: usize) -> String {
    if wardrobe.shop.durty_hashes {
        // ⚠ Durty's names carry no collection, so the place in the slot keeps the names of a cut slot apart.
        return durty_name(placed.ped, placed.slot, placed.place, texture);
    }
    let collection = wardrobe.collection_of(placed.ped, placed.part);
    format!(
        "{collection}_{}_{:03}_{texture}",
        placed.slot.name(),
        placed.position
    )
    .to_ascii_uppercase()
}

/// The word Durty Cloth Tool's shop entries use for a component slot, which is the shop's name for the garment and not the slot's.
///
/// ⚠ Taken from the owner's Vestium source, whose notes say the mapping was checked by paired counts
/// against Durty-built packs. It is not re-measured here: no Durty-built resource is staged.
pub fn durty_category(component: Component) -> &'static str {
    match component {
        Component::Head => "HEAD",
        Component::Berd => "MASK",
        Component::Hair => "HAIR",
        Component::Uppr => "BODY",
        Component::Lowr => "LEGS",
        Component::Hand => "BAG",
        Component::Feet => "SHOES",
        Component::Teef => "ACCESSORIES",
        Component::Accs => "UNDERSHIRT",
        Component::Task => "ARMOR",
        Component::Decl => "DECAL",
        Component::Jbib => "TOP",
    }
}

/// The name Durty Cloth Tool gives a shop entry: `CLO_DCT_TOP_0_1_M` for the second texture of the first top of the male ped, `CLO_DCT_PROPLWRIST_3_0_F` for the first of the fourth left-wrist prop of the female.
pub fn durty_name(ped: Ped, slot: Slot, position: u32, texture: usize) -> String {
    let gender = match ped {
        Ped::Male => 'M',
        Ped::Female => 'F',
    };
    match slot {
        Slot::Component(c) => format!(
            "CLO_DCT_{}_{position}_{texture}_{gender}",
            durty_category(c)
        ),
        Slot::Prop(p) => format!(
            "CLO_DCT_PROP{}_{position}_{texture}_{gender}",
            p.name().trim_start_matches("p_").to_ascii_uppercase()
        ),
    }
}

/// The shop metadata of `ped`: the header that registers its collection and an entry for each texture of each garment that ships a model.
pub fn shop_meta(wardrobe: &Wardrobe, allocation: &Allocation, ped: Ped, creature: bool) -> String {
    shop_meta_of(wardrobe, allocation, (ped, 0), creature)
}

/// The shop metadata of one collection of `ped`; the entries kept for want of a garment are the pack's own collection's.
pub fn shop_meta_of(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    (ped, part): (Ped, u32),
    creature: bool,
) -> String {
    let collection = wardrobe.collection_of(ped, part);
    let mut text = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ShopPedApparel>\n");
    text.push_str(&format!("  <pedName>{}</pedName>\n", ped.model()));
    text.push_str(&format!("  <dlcName>{collection}</dlcName>\n"));
    text.push_str(&format!(
        "  <fullDlcName>{}</fullDlcName>\n",
        layout::stem(ped, &collection, false)
    ));
    // ⚠ A female ped's shop file states the `_F` form and a male's the bare one
    // (`the_plan_states_the_shop_metadata_the_reference_registers_its_collections_with`).
    let character = match ped {
        Ped::Male => "SCR_CHAR_MULTIPLAYER",
        Ped::Female => "SCR_CHAR_MULTIPLAYER_F",
    };
    text.push_str(&format!("  <eCharacter>{character}</eCharacter>\n"));
    if creature {
        text.push_str(&format!(
            "  <creatureMetaData>{}</creatureMetaData>\n",
            layout::creature_name(&layout::creature_pack(&wardrobe.pack, part))
        ));
    }
    // ⚠ The id of an entry is the one an import read or somebody chose, else the name the build gives; it is never worked out from the position of a garment that has one.
    let entries = |text: &mut String, props: bool| {
        for p in allocation
            .placed
            .iter()
            .filter(|p| p.ped == ped && p.part == part && p.slot.is_prop() == props)
        {
            let garment = &wardrobe.garments[p.garment];
            for (t, v) in garment.variations.iter().enumerate() {
                if shop::is_written(garment, t) {
                    let id = shop::id_of(wardrobe, garment, p, t);
                    shop::write_entry(
                        text,
                        props,
                        &id,
                        &shop::kind_of(p.slot),
                        p.position,
                        t,
                        v.shop.as_ref(),
                    );
                }
            }
        }
        // What no garment answers for is written as it was stated.
        for s in wardrobe
            .shop
            .unplaced
            .iter()
            .filter(|s| part == 0 && s.ped == ped && s.prop == props)
        {
            let texture = s.texture as usize;
            shop::write_entry(
                text,
                props,
                &s.entry.id,
                &s.kind,
                s.drawable,
                texture,
                Some(&s.entry),
            );
        }
    };
    text.push_str("  <pedComponents>\n");
    entries(&mut text, false);
    text.push_str("  </pedComponents>\n  <pedProps>\n");
    entries(&mut text, true);
    text.push_str("  </pedProps>\n</ShopPedApparel>\n");
    text
}
