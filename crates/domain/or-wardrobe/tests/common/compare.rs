//! Two variation metadata documents compared by what they mean, for a plan held to a shipped document.

#![allow(dead_code)]

use std::collections::BTreeMap;

use or_io::meta::{CCreatureMetaData, CPedVariationInfo};

/// What differs between `shipped` and `made`, in the members a pack models; empty when they mean the same.
///
/// Left out: the flags that state whether a ped has variations, which no content decides, the
/// selection sets, which a pack has none of, the `dlcName`, which the collection decides, and the
/// order of `compInfos`, which the game looks up by slot and drawable and the staged packs do not
/// agree on.
pub fn same_meaning(shipped: &CPedVariationInfo, made: &CPedVariationInfo) -> Vec<String> {
    let mut wrong = Vec::new();
    if shipped.avail_comp != made.avail_comp {
        wrong.push(format!(
            "availComp {:?} against {:?}",
            shipped.avail_comp, made.avail_comp
        ));
    }
    if shipped.a_component_data3.len() != made.a_component_data3.len() {
        wrong.push(format!(
            "{} components against {}",
            shipped.a_component_data3.len(),
            made.a_component_data3.len()
        ));
        return wrong;
    }
    for (c, (a, b)) in shipped
        .a_component_data3
        .iter()
        .zip(&made.a_component_data3)
        .enumerate()
    {
        if a.num_avail_tex != b.num_avail_tex {
            wrong.push(format!(
                "component data {c}: numAvailTex {} against {}",
                a.num_avail_tex, b.num_avail_tex
            ));
        }
        if a.a_drawbl_data3.len() != b.a_drawbl_data3.len() {
            wrong.push(format!(
                "component data {c}: {} drawables against {}",
                a.a_drawbl_data3.len(),
                b.a_drawbl_data3.len()
            ));
            continue;
        }
        for (d, (x, y)) in a.a_drawbl_data3.iter().zip(&b.a_drawbl_data3).enumerate() {
            let textures = |d: &or_io::meta::CPVDrawblData| -> Vec<(u8, u8)> {
                d.a_tex_data
                    .iter()
                    .map(|t| (t.tex_id, t.distribution))
                    .collect()
            };
            if (x.prop_mask, x.num_alternatives, x.cloth_data.owns_cloth)
                != (y.prop_mask, y.num_alternatives, y.cloth_data.owns_cloth)
                || textures(x) != textures(y)
            {
                wrong.push(format!(
                    "component data {c}, drawable {d}: {x:?} against {y:?}"
                ));
                break;
            }
        }
    }
    let key = |i: &or_io::meta::CComponentInfo| (i.ped_xml_comp_idx, i.ped_xml_drawbl_idx);
    let made_infos: BTreeMap<_, _> = made.comp_infos.iter().map(|i| (key(i), i)).collect();
    for info in &shipped.comp_infos {
        match made_infos.get(&key(info)) {
            None => wrong.push(format!(
                "no component info for slot {} drawable {}",
                key(info).0,
                key(info).1
            )),
            Some(m) => {
                let words = |i: &or_io::meta::CComponentInfo| {
                    (
                        i.ped_xml_audio_id.hash,
                        i.ped_xml_audio_id2.hash,
                        i.ped_xml_expression_mods
                            .iter()
                            .map(|f| f.to_bits())
                            .collect::<Vec<_>>(),
                        i.flags,
                        i.inclusions,
                        i.exclusions,
                        i.ped_xml_vfx_comps,
                        i.ped_xml_flags,
                    )
                };
                if words(info) != words(m) {
                    wrong.push(format!(
                        "component info of slot {} drawable {} differs",
                        key(info).0,
                        key(info).1
                    ));
                }
            }
        }
    }
    let (a, b) = (&shipped.prop_info, &made.prop_info);
    if a.num_avail_props != b.num_avail_props {
        wrong.push(format!(
            "numAvailProps {} against {}",
            a.num_avail_props, b.num_avail_props
        ));
    }
    // The props and the anchors are looked up by the ids they state, and the staged packs do not agree on their order.
    let props = |p: &or_io::meta::CPedPropInfo| -> Vec<String> {
        let mut sorted: Vec<_> = p.a_prop_meta_data.iter().collect();
        sorted.sort_by_key(|m| (m.anchor_id, m.prop_id));
        sorted
            .into_iter()
            .map(|m| {
                format!(
                    "{}/{} {:?} {:?} {:?} {} {} {} {} {:?}",
                    m.anchor_id,
                    m.prop_id,
                    m.audio_id.hash,
                    m.expression_mods
                        .iter()
                        .map(|f| f.to_bits())
                        .collect::<Vec<_>>(),
                    m.render_flags,
                    m.prop_flags,
                    m.flags,
                    m.stickyness,
                    m.tex_data.len(),
                    m.tex_data
                        .iter()
                        .map(|t| (
                            t.tex_id,
                            t.distribution,
                            t.inclusions,
                            t.exclusions,
                            t.inclusion_id,
                            t.exclusion_id
                        ))
                        .collect::<Vec<_>>()
                )
            })
            .collect()
    };
    if props(a) != props(b) {
        wrong.push("the props differ".to_string());
    }
    let anchors = |p: &or_io::meta::CPedPropInfo| -> Vec<(u8, Vec<u8>)> {
        let mut held: Vec<_> = p
            .a_anchors
            .iter()
            .map(|x| (x.anchor.0, x.props.clone()))
            .collect();
        held.sort();
        held
    };
    if anchors(a) != anchors(b) {
        wrong.push(format!("anchors {:?} against {:?}", anchors(a), anchors(b)));
    }
    wrong
}

/// The expressions of a creature document, as a sorted list of what they state.
pub fn expressions(creature: &CCreatureMetaData) -> Vec<String> {
    let mut found: Vec<String> = creature
        .ped_comp_expressions
        .iter()
        .map(|e| {
            format!(
                "comp {} {} {} {:?} {:?} {:?} {:?}",
                e.ped_comp_id,
                e.ped_comp_var_index,
                e.ped_comp_expression_index,
                e.tracks,
                e.ids,
                e.types,
                e.components
            )
        })
        .chain(creature.ped_prop_expressions.iter().map(|e| {
            format!(
                "prop {} {} {} {:?} {:?} {:?} {:?}",
                e.ped_prop_id,
                e.ped_prop_var_index,
                e.ped_prop_expression_index,
                e.tracks,
                e.ids,
                e.types,
                e.components
            )
        }))
        .collect();
    found.sort();
    found
}
