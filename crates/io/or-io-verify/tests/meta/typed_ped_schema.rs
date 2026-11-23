//! The ped variation family's types, declared in code, held to the type tables the staged
//! documents state, and a record built from nothing held to the same tables.
//!
//! ⚠ Every staged document states one table for each class of the family, so a class a
//! synthesiser lays out differently is a table nothing shipped.

use std::collections::BTreeMap;

use crate::corpus::Corpus;
use crate::support;

use or_io::decl::File;
use or_io::hash::literal_string_hash;
use or_io::meta::binary::rbm::schema::{RbmClass, RbmEnum, RbmModel};
use or_io::meta::binary::synth::{name_hash, rbm_layout_for_instance};
use or_io::meta::binary::ContainerLayout;
use or_io::meta::instance::HashedString;
use or_io::meta::typed::ped_types;
use or_io::meta::{
    Anchor, CAnchorProps, CComponentInfo, CPVComponentData, CPVDrawblData, CPVTextureData,
    CPedPropMetaData, CPedPropTexData, CPedSelectionSet, CPedVariationInfo, MetaDocument,
    MetaInstance,
};

const ROOTS: [&str; 2] = ["CPedVariationInfo", "CCreatureMetaData"];

/// The table the declared types lay out for `root`, with the signatures the family carries.
fn declared_model(root: &str) -> RbmModel {
    rbm_layout_for_instance(
        ped_types::declared(),
        &MetaInstance::new(root),
        Some(&ped_types::donor(None)),
    )
    .unwrap_or_else(|e| panic!("the declared types lay out {root}: {e}"))
    .model
}

/// The RBM table of every staged document rooted at one of the family's two roots.
fn staged_tables(what: &str) -> Option<Vec<(String, String, RbmModel)>> {
    let library = support::schema();
    let found: Vec<_> = Corpus::load()
        .iter()
        .filter_map(|f| {
            let doc = MetaDocument::parse(library, f.bytes.clone()).ok()?;
            let root = ROOTS
                .iter()
                .find(|r| name_hash(r) == literal_string_hash(doc.instance.type_name.as_bytes()))?;
            let ContainerLayout::Rbm(rbm) = doc.layout?.container else {
                return None;
            };
            Some((f.name.clone(), (*root).to_string(), rbm.model))
        })
        .collect();
    if found.is_empty() {
        or_test_support::skip_corpus(
            what,
            "no ped variation or creature document is staged as RBM",
        );
        return None;
    }
    Some(found)
}

/// What differs between two classes, or nothing.
fn class_diff(shipped: &RbmClass, declared: &RbmClass) -> Option<String> {
    let mut wrong = Vec::new();
    macro_rules! same {
        ($($field:ident),*) => {$(
            if shipped.$field != declared.$field {
                wrong.push(format!(
                    "{} is {:?} in the documents and {:?} declared",
                    stringify!($field), shipped.$field, declared.$field
                ));
            }
        )*};
    }
    same!(signature, flags, align_power, data_size, major_version);
    if shipped.entries != declared.entries {
        let at = shipped
            .entries
            .iter()
            .zip(&declared.entries)
            .position(|(a, b)| a != b);
        wrong.push(format!(
            "the member descriptors differ ({} and {} of them, the first difference at {at:?}: {:?} against {:?})",
            shipped.entries.len(),
            declared.entries.len(),
            at.and_then(|i| shipped.entries.get(i)),
            at.and_then(|i| declared.entries.get(i)),
        ));
    }
    (!wrong.is_empty()).then(|| wrong.join("; "))
}

fn enum_diff(shipped: &RbmEnum, declared: &RbmEnum) -> Option<String> {
    let mut wrong = Vec::new();
    if shipped.signature != declared.signature || shipped.flags != declared.flags {
        wrong.push(format!(
            "signature {:#010X} and flags {} in the documents, {:#010X} and {} declared",
            shipped.signature, shipped.flags, declared.signature, declared.flags
        ));
    }
    if shipped.values != declared.values {
        wrong.push(format!(
            "the enumerators differ: {:?} against {:?}",
            shipped.values, declared.values
        ));
    }
    (!wrong.is_empty()).then(|| wrong.join("; "))
}

/// Compare every class and enum `shipped` states with the one `declared` lays out for it.
fn compare(
    file: &str,
    shipped: &RbmModel,
    declared: &RbmModel,
    compared: &mut BTreeMap<String, usize>,
    failures: &mut BTreeMap<String, String>,
) {
    let names: BTreeMap<u32, &String> = ped_types::declared()
        .structs
        .keys()
        .chain(ped_types::declared().enums.keys())
        .map(|n| (name_hash(n), n))
        .collect();
    for hash in &shipped.class_order {
        let class = &shipped.classes[hash];
        let name = names
            .get(hash)
            .map_or_else(|| format!("{hash:#010X}"), |n| (*n).clone());
        *compared.entry(name.clone()).or_default() += 1;
        match declared.class_for(*hash) {
            None => {
                failures.entry(name).or_insert_with(|| {
                    format!("{file}: the declared types do not reach this class")
                });
            }
            Some(own) => {
                if let Some(why) = class_diff(class, own) {
                    failures
                        .entry(name)
                        .or_insert_with(|| format!("{file}: {why}"));
                }
            }
        }
    }
    for hash in &shipped.enum_order {
        let en = &shipped.enums[hash];
        let name = names
            .get(hash)
            .map_or_else(|| format!("{hash:#010X}"), |n| (*n).clone());
        *compared.entry(name.clone()).or_default() += 1;
        match declared.enum_for(*hash) {
            None => {
                failures.entry(name).or_insert_with(|| {
                    format!("{file}: the declared types do not reach this enum")
                });
            }
            Some(own) => {
                if let Some(why) = enum_diff(en, own) {
                    failures
                        .entry(name)
                        .or_insert_with(|| format!("{file}: {why}"));
                }
            }
        }
    }
}

fn expect_every_declared_type_compared(compared: &BTreeMap<String, usize>) {
    let library = ped_types::declared();
    let missing: Vec<_> = library
        .structs
        .keys()
        .chain(library.enums.keys())
        .filter(|n| !compared.contains_key(*n))
        .collect();
    assert!(
        missing.is_empty(),
        "no staged document states {missing:?}, so nothing holds the declaration to a table"
    );
}

fn report(failures: &BTreeMap<String, String>) {
    assert!(
        failures.is_empty(),
        "{} declared type(s) are not what the tables state:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(name, why)| format!("  {name}: {why}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn every_declared_ped_class_is_the_table_the_documents_state() {
    const WHAT: &str = "every_declared_ped_class_is_the_table_the_documents_state";
    let Some(found) = staged_tables(WHAT) else {
        return;
    };
    let declared: BTreeMap<&str, RbmModel> =
        ROOTS.iter().map(|r| (*r, declared_model(r))).collect();
    let (mut compared, mut failures) = (BTreeMap::new(), BTreeMap::new());
    for (file, root, shipped) in &found {
        compare(
            file,
            shipped,
            &declared[root.as_str()],
            &mut compared,
            &mut failures,
        );
    }
    report(&failures);
    expect_every_declared_type_compared(&compared);
}

/// A variation with every kind of record the family has, each of them built here.
fn built_variation() -> CPedVariationInfo {
    let mut info = CPedVariationInfo::default();
    info.b_has_drawbl_variations = true;
    info.b_has_tex_variations = true;
    info.avail_comp[11] = 0;
    info.dlc_name = HashedString::new("openrage_built");
    let mut drawable = CPVDrawblData::default();
    drawable.prop_mask = 1;
    let mut texture = CPVTextureData::default();
    texture.distribution = 255;
    drawable.a_tex_data.push(texture);
    let mut component = CPVComponentData::default();
    component.a_drawbl_data3.push(drawable);
    info.a_component_data3.push(component);
    let mut component_info = CComponentInfo::default();
    component_info.ped_xml_comp_idx = 11;
    info.comp_infos.push(component_info);
    info.a_selection_sets.push(CPedSelectionSet::default());
    let mut prop = CPedPropMetaData::default();
    prop.tex_data.push(CPedPropTexData::default());
    info.prop_info.a_prop_meta_data.push(prop);
    let mut anchor = CAnchorProps::default();
    anchor.props = vec![1];
    anchor.anchor = Anchor(0);
    info.prop_info.a_anchors.push(anchor);
    info.restate_counts();
    info
}

#[test]
fn a_variation_built_here_carries_the_table_every_shipped_one_does() {
    const WHAT: &str = "a_variation_built_here_carries_the_table_every_shipped_one_does";
    let Some(found) = staged_tables(WHAT) else {
        return;
    };
    let saved = built_variation()
        .to_bytes()
        .expect("a variation built here saves");
    let doc = MetaDocument::parse(support::schema(), saved).expect("it parses");
    let ContainerLayout::Rbm(rbm) = doc.layout.expect("it has a layout").container else {
        panic!("a variation built here is an RBM document");
    };
    let (mut compared, mut failures) = (BTreeMap::new(), BTreeMap::new());
    for (file, root, shipped) in &found {
        if root == "CPedVariationInfo" {
            compare(file, shipped, &rbm.model, &mut compared, &mut failures);
        }
    }
    report(&failures);
    for name in [
        "CPVDrawblData__CPVClothComponentData",
        "CPVTextureData",
        "CAnchorProps",
    ] {
        let name = name.replace("__", "::");
        assert!(compared.contains_key(&name), "{name} was never compared");
    }
}
