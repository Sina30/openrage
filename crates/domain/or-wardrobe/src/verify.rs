//! The verifier: what a built resource has to hold for the pack that was planned, read back.
//!
//! A build already says that every file it wrote is the plan's and reloads through a reader. This
//! says what only a pack knows: that each ped's variation metadata states the garments the model
//! places, with the textures and the component info each of them has, that the creature and shop
//! metadata say what the plan says, and that the collection each ped registers is the one claimed.
//!
//! ⚠ It checks the files and not the game. Whether GTA V loads a pack is a thing nobody working on
//! this repository can run.

use std::collections::BTreeMap;
use std::path::Path;

use or_io::decl::File;
use or_io::meta::{
    CAlternateVariations, CCreatureMetaData, CPedVariationInfo, FirstPersonAlternateData,
    MetaDocument,
};
use or_project::{BuiltFiles, Folder};

use crate::layout;
use crate::metadata::hashed;
use crate::model::Wardrobe;
use crate::names::{Ped, Slot};
use crate::plan::{place_name, Planned};
use crate::shop;

pub use or_project::content::check::{Check, Verification};

/// Read `resource`, the folder a build of `planned` made, against `wardrobe`.
pub fn verify(wardrobe: &Wardrobe, planned: &Planned, resource: &Path) -> Verification {
    verify_files(wardrobe, planned, &Folder(resource))
}

/// Read the files of the build of `planned`, wherever the build keeps them, against `wardrobe`.
///
/// ⚠ `wardrobe` is the pack as the build wrote it ([`crate::plan::planned_for`] returns it), and the paths are the ones `planned` states.
pub fn verify_files(
    wardrobe: &Wardrobe,
    planned: &Planned,
    resource: &dyn BuiltFiles,
) -> Verification {
    let mut found = Verification::default();
    for (ped, part) in planned.documents.collections() {
        let collection = wardrobe.collection_of(ped, part);
        if let Some(expected) = planned.documents.variation(ped, part) {
            variation(
                &mut found,
                wardrobe,
                planned,
                resource,
                (ped, part),
                &collection,
                expected,
            );
        }
        // ⚠ A ped with no garment has a shop file when entries were kept for it, and no variation metadata.
        shop(
            &mut found,
            wardrobe,
            planned,
            resource,
            (ped, part),
            &collection,
        );
    }
    creature(&mut found, wardrobe, planned, resource);
    alternates(&mut found, planned, resource);
    behaviour(&mut found, wardrobe, planned, resource);
    cloth(&mut found, wardrobe, planned, resource);
    found
}

/// The alternate variations and the first person alternates the plan made: each file reopens as the typed root, and is the root the plan made.
fn alternates(found: &mut Verification, planned: &Planned, resource: &dyn BuiltFiles) {
    if let Some(expected) = &planned.documents.alternates {
        let path = planned.build.paths.alternate_path();
        let read = resource.read(&path).and_then(|bytes| {
            let root = CAlternateVariations::from_bytes(&bytes).map_err(|e| e.to_string())?;
            Ok((root, bytes))
        });
        let name = "alternate variations".to_string();
        match read {
            Ok((root, bytes)) if expected.to_bytes().ok().as_deref() == Some(&bytes[..]) => {
                let switches: usize = root.peds.iter().map(|p| p.switches.len()).sum();
                found.add(
                    name,
                    Vec::new(),
                    format!("{switches} switch(es) in `{path}`, as planned"),
                );
            }
            Ok(_) => found.add(
                name,
                vec![format!("`{path}` is not the metadata the plan made")],
                String::new(),
            ),
            Err(e) => found.add(
                name,
                vec![format!("`{path}` does not reopen: {e}")],
                String::new(),
            ),
        }
    }
    if let Some(expected) = &planned.documents.first_person {
        let path = planned.build.paths.first_person_path();
        let read = resource.read(&path).and_then(|bytes| {
            let root = FirstPersonAlternateData::from_bytes(&bytes).map_err(|e| e.to_string())?;
            Ok((root, bytes))
        });
        let name = "first person alternates".to_string();
        match read {
            Ok((root, bytes)) if expected.to_bytes().ok().as_deref() == Some(&bytes[..]) => found
                .add(
                    name,
                    Vec::new(),
                    format!(
                        "{} entr(ies) in `{path}`, as planned",
                        root.alternates.len()
                    ),
                ),
            Ok(_) => found.add(
                name,
                vec![format!("`{path}` is not the metadata the plan made")],
                String::new(),
            ),
            Err(e) => found.add(
                name,
                vec![format!("`{path}` does not reopen: {e}")],
                String::new(),
            ),
        }
    }
}

/// What each garment does, read back off the metadata the build wrote: its flags, its mask, its audio, and what a hat and a hair do to the hair.
///
/// ⚠ Read off the written files and not off the plan, so a build whose metadata and pack disagree is caught whichever of them was wrong.
fn behaviour(
    found: &mut Verification,
    wardrobe: &Wardrobe,
    planned: &Planned,
    resource: &dyn BuiltFiles,
) {
    let creature = planned
        .documents
        .creature
        .as_ref()
        .and_then(|_| resource.read(&planned.build.paths.creature_path()).ok())
        .and_then(|bytes| CCreatureMetaData::from_bytes(&bytes).ok());
    let mut problems = Vec::new();
    let mut checked = 0usize;
    for (ped, part) in planned.documents.collections() {
        let collection = wardrobe.collection_of(ped, part);
        let path = planned.build.paths.variation_path(ped, &collection);
        // The variation check says when the metadata is not there or does not reopen.
        let Some(info) = resource
            .read(&path)
            .ok()
            .and_then(|bytes| CPedVariationInfo::from_bytes(&bytes).ok())
        else {
            continue;
        };
        for p in (planned.allocation.placed.iter()).filter(|p| p.ped == ped && p.part == part) {
            let garment = &wardrobe.garments[p.garment];
            if garment.is_reserved() {
                continue;
            }
            checked += 1;
            let label = place_name(wardrobe, p);
            let said = crate::behaviour::stated(&info, p.slot, p.position);
            let wanted = crate::behaviour::Stated::of(garment);
            for difference in said.differences(&wanted) {
                problems.push(format!("{label} {difference}"));
            }
            // The expressions that drive the hair are in the creature metadata, one entry for each garment that states one.
            for (index, bone, value) in crate::behaviour::hair_driven(garment) {
                let driven = creature
                    .as_ref()
                    .is_some_and(|c| crate::behaviour::drives(c, p.slot, p.position, index, bone));
                if !driven {
                    problems.push(format!(
                        "{label} states {value} for the hair and the creature metadata does not drive bone {bone} with it"
                    ));
                }
            }
        }
    }
    if checked > 0 {
        found.add(
            "garment behaviour",
            problems,
            format!("{checked} garment(s) state in the metadata the flags, mask, audio and hair their pack says"),
        );
    }
}

/// The cloth of the drawables that own it: the metadata the build wrote says which do, and each has a file beside its model that a reader opens.
///
/// ⚠ Read off the written metadata and not off the pack, so a build whose `ownsCloth` and whose files disagree is caught whichever of them was wrong.
fn cloth(
    found: &mut Verification,
    wardrobe: &Wardrobe,
    planned: &Planned,
    resource: &dyn BuiltFiles,
) {
    let reader = or_io::gta5::formats::format("yld");
    let (mut problems, mut notes) = (Vec::new(), Vec::new());
    let (mut owning, mut opened, mut winds) = (0usize, 0usize, 0usize);
    for (ped, part) in planned.documents.collections() {
        let collection = wardrobe.collection_of(ped, part);
        let path = planned.build.paths.variation_path(ped, &collection);
        // The variation check says when the metadata is not there or does not reopen.
        let Some(info) = resource
            .read(&path)
            .ok()
            .and_then(|bytes| CPedVariationInfo::from_bytes(&bytes).ok())
        else {
            continue;
        };
        for p in (planned.allocation.placed.iter()).filter(|p| p.ped == ped && p.part == part) {
            let Slot::Component(c) = p.slot else {
                continue;
            };
            let garment = &wardrobe.garments[p.garment];
            let owns = info
                .avail_comp
                .get(usize::from(c.id()))
                .filter(|i| **i != u8::MAX)
                .and_then(|i| info.a_component_data3.get(usize::from(*i)))
                .and_then(|d| d.a_drawbl_data3.get(p.position as usize))
                .is_some_and(|d| d.cloth_data.owns_cloth);
            let ships = garment.cloth.is_some() && !garment.is_reserved();
            if !owns && !ships {
                continue;
            }
            owning += usize::from(owns);
            let label = place_name(wardrobe, p);
            let paths = &planned.build.paths;
            let name = layout::cloth_name(&paths.model_name(
                ped,
                &collection,
                p.slot,
                p.position,
                garment.skin,
            ));
            let file = format!("{}/{name}", paths.directory(ped, &collection, p.slot));
            if !ships {
                // The pack waived the rule that says a cloth is missing, with a reason.
                let waived = planned.findings.iter().find(|f| {
                    f.code == "cloth-missing"
                        && f.garment.as_deref() == Some(garment.id.as_str())
                        && f.waived.is_some()
                });
                match waived {
                    Some(f) => notes.push(format!(
                        "{label} owns cloth and ships none (waived: {})",
                        f.waived.as_deref().unwrap_or_default()
                    )),
                    None => problems.push(format!(
                        "{label} states ownsCloth and the build has no cloth file for it"
                    )),
                }
                continue;
            }
            match resource.read(&file) {
                Err(_) => problems.push(format!("{label}: `{file}` is not in the build")),
                Ok(bytes) => {
                    match reader.map(|f| f.open_declared(&bytes)) {
                        Some(Ok(_)) => opened += 1,
                        Some(Err(e)) => {
                            problems.push(format!("{label}: `{file}` does not reopen: {e}"))
                        }
                        None => problems.push("no reader for `.yld` is declared".to_string()),
                    }
                    if let Some(scale) = garment.wind_scale {
                        winds += 1;
                        problems.extend(wind_problem(&label, &file, scale, &bytes));
                    }
                }
            }
            if !owns {
                notes.push(format!(
                    "{label} ships `{file}` and its metadata does not state ownsCloth, so the game does not use it"
                ));
            }
        }
    }
    if owning == 0 && opened == 0 && notes.is_empty() && problems.is_empty() {
        return;
    }
    let mut held = format!(
        "{owning} drawable(s) own cloth and {opened} cloth file(s) beside their models reopen"
    );
    if winds > 0 {
        held.push_str(&format!(
            "; {winds} of them state in the build the wind scale their pack says"
        ));
    }
    if !notes.is_empty() {
        held.push_str(&format!("; {}", notes.join("; ")));
    }
    found.add("cloth", problems, held);
}

/// Why the cloth file `file` of the build does not state the wind scale `scale` its pack says, none where every cloth of it does.
fn wind_problem(label: &str, file: &str, scale: f32, bytes: &[u8]) -> Option<String> {
    match crate::wind::stated(bytes) {
        Err(e) => Some(format!(
            "{label}: `{file}` does not read back its wind: {e}"
        )),
        Ok(winds) if winds.is_empty() || winds.iter().any(|w| *w != scale) => Some(format!(
            "{label}: `{file}` states the wind scale {winds:?} and the pack says {scale}"
        )),
        Ok(_) => None,
    }
}

fn variation(
    found: &mut Verification,
    wardrobe: &Wardrobe,
    planned: &Planned,
    resource: &dyn BuiltFiles,
    (ped, part): (Ped, u32),
    collection: &str,
    expected: &CPedVariationInfo,
) {
    let name = match part {
        0 => format!("{ped} variation metadata"),
        _ => format!("{ped} variation metadata of {collection}"),
    };
    let path = planned.build.paths.variation_path(ped, collection);
    let bytes = match resource.read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return found.add(
                name,
                vec![format!("`{path}` cannot be read: {e}")],
                String::new(),
            )
        }
    };
    let info = match CPedVariationInfo::from_bytes(&bytes) {
        Ok(info) => info,
        Err(e) => {
            return found.add(
                name,
                vec![format!("`{path}` does not reopen: {e}")],
                String::new(),
            )
        }
    };
    let mut problems = Vec::new();
    // ⚠ Compared as the bytes the plan's records make, because a reopened name has not the text it was built from.
    if expected.to_bytes().ok().as_deref() != Some(&bytes[..]) {
        problems.push("it is not the metadata the plan made".to_string());
    }
    if info.dlc_name.hash != hashed(collection).hash {
        problems.push(format!("its dlcName is not `{collection}`"));
    }
    let mut checked = 0usize;
    for p in (planned.allocation.placed.iter()).filter(|p| p.ped == ped && p.part == part) {
        let garment = &wardrobe.garments[p.garment];
        let label = place_name(wardrobe, p);
        checked += 1;
        match p.slot {
            Slot::Component(c) => {
                let data = info
                    .avail_comp
                    .get(usize::from(c.id()))
                    .filter(|i| **i != u8::MAX)
                    .and_then(|i| info.a_component_data3.get(usize::from(*i)));
                let Some(drawable) = data.and_then(|d| d.a_drawbl_data3.get(p.position as usize))
                else {
                    problems.push(format!("{label} has no drawable"));
                    continue;
                };
                if !garment.is_reserved() && drawable.a_tex_data.len() != garment.variations.len() {
                    problems.push(format!(
                        "{label} states {} textures and has {}",
                        drawable.a_tex_data.len(),
                        garment.variations.len()
                    ));
                }
                let infos = info
                    .comp_infos
                    .iter()
                    .filter(|i| {
                        i.ped_xml_comp_idx == c.id()
                            && u32::from(i.ped_xml_drawbl_idx) == p.position
                    })
                    .count();
                if infos != 1 {
                    problems.push(format!(
                        "{label} has {infos} component infos, and a drawable has one"
                    ));
                }
            }
            Slot::Prop(prop) => {
                let meta =
                    info.prop_info.a_prop_meta_data.iter().find(|m| {
                        m.anchor_id == prop.anchor() && u32::from(m.prop_id) == p.position
                    });
                match meta {
                    None => problems.push(format!("{label} has no prop")),
                    Some(m)
                        if !garment.is_reserved()
                            && m.tex_data.len() != garment.variations.len() =>
                    {
                        problems.push(format!(
                            "{label} states {} textures and has {}",
                            m.tex_data.len(),
                            garment.variations.len()
                        ));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    found.add(
        name,
        problems,
        format!("{checked} garment(s) are drawables of `{path}`, with the textures they have"),
    );
}

fn creature(
    found: &mut Verification,
    _wardrobe: &Wardrobe,
    planned: &Planned,
    resource: &dyn BuiltFiles,
) {
    let parts: Vec<(u32, &CCreatureMetaData)> = (planned.documents.creature.iter())
        .map(|c| (0, c))
        .chain(
            planned
                .documents
                .later_creatures
                .iter()
                .map(|(n, c)| (*n, c)),
        )
        .collect();
    for (part, expected) in parts {
        creature_of(found, planned, resource, part, expected);
    }
}

fn creature_of(
    found: &mut Verification,
    planned: &Planned,
    resource: &dyn BuiltFiles,
    part: u32,
    expected: &CCreatureMetaData,
) {
    let name = match part {
        0 => "creature metadata".to_string(),
        n => format!("creature metadata of collection {n}"),
    };
    let path = planned.build.paths.creature_path_of(part);
    let read = resource.read(&path).and_then(|bytes| {
        let creature = CCreatureMetaData::from_bytes(&bytes).map_err(|e| e.to_string())?;
        Ok((creature, bytes))
    });
    match read {
        Ok((creature, bytes)) if expected.to_bytes().ok().as_deref() == Some(&bytes[..]) => {
            let held = creature.ped_comp_expressions.len() + creature.ped_prop_expressions.len();
            found.add(
                name,
                Vec::new(),
                format!("{held} expression(s) in `{path}`, as planned"),
            );
        }
        Ok(_) => found.add(
            name,
            vec![format!("`{path}` is not the metadata the plan made")],
            String::new(),
        ),
        Err(e) => found.add(
            name,
            vec![format!("`{path}` does not reopen: {e}")],
            String::new(),
        ),
    }
}

fn shop(
    found: &mut Verification,
    wardrobe: &Wardrobe,
    planned: &Planned,
    resource: &dyn BuiltFiles,
    (ped, part): (Ped, u32),
    collection: &str,
) {
    let name = match part {
        0 => format!("{ped} shop metadata"),
        _ => format!("{ped} shop metadata of {collection}"),
    };
    let path = planned.build.paths.shop_path(ped, collection);
    let bytes = match resource.read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return found.add(
                name,
                vec![format!("`{path}` cannot be read: {e}")],
                String::new(),
            )
        }
    };
    let mut problems = Vec::new();
    if let Err(e) = MetaDocument::parse(or_io::meta::default_library(), bytes.clone()) {
        problems.push(format!("`{path}` does not reload: {e}"));
    }
    let text = String::from_utf8_lossy(&bytes);
    // ⚠ Held to the ids the plan wrote, entry by entry: a build whose shop file has the right number of entries and the wrong ids is not the plan.
    let stated = match shop::ids_in(&text) {
        Ok(stated) => stated,
        Err(e) => {
            problems.push(format!("`{path}` does not read as shop metadata: {e}"));
            Vec::new()
        }
    };
    let expected: Vec<shop::Written> = shop::written(wardrobe, &planned.allocation)
        .into_iter()
        .filter(|w| w.ped == ped && w.part == part)
        .collect();
    let (mut components, mut props) = (0usize, 0usize);
    for w in &expected {
        match w.prop {
            true => props += 1,
            false => components += 1,
        }
    }
    let (held_props, held_components) = (
        stated.iter().filter(|(k, _)| k.0).count(),
        stated.iter().filter(|(k, _)| !k.0).count(),
    );
    for (section, held, wanted) in [
        ("pedComponents", held_components, components),
        ("pedProps", held_props, props),
    ] {
        if held != wanted {
            problems.push(format!(
                "{section} holds {held} entries and the plan writes {wanted}"
            ));
        }
    }
    // ⚠ Two entries may point at one texture (an import keeps the second as it was stated), so the nth entry of a place is held to the nth the plan writes for it.
    let mut seen: BTreeMap<shop::Key, usize> = BTreeMap::new();
    for w in &expected {
        let key = shop::key_of(
            w.prop,
            &w.kind,
            w.position,
            u32::try_from(w.texture).unwrap_or(u32::MAX),
        );
        let nth = seen.entry(key.clone()).or_insert(0);
        let held = stated.iter().filter(|(k, _)| *k == key).nth(*nth);
        *nth += 1;
        match held {
            Some((_, id)) if *id == w.id => {}
            Some((_, id)) => problems.push(format!(
                "the entry for {} {} texture {} states the id {id} and the plan {}",
                w.kind, w.position, w.texture, w.id
            )),
            None => problems.push(format!(
                "there is no entry for {} {} texture {}",
                w.kind, w.position, w.texture
            )),
        }
    }
    if !text.contains(&format!("<dlcName>{collection}</dlcName>")) {
        problems.push(format!("its dlcName is not `{collection}`"));
    }
    found.add(
        name,
        problems,
        format!("`{path}` registers `{collection}` with {components} component and {props} prop entries"),
    );
}
