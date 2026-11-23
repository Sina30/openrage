//! Growing an array inside an RBM document, which is the one thing a
//! synthesiser exists for.
//!
//! ⚠ A document that did *not* grow has to keep re-encoding to the bytes it
//! arrived as. Re-encoding against a synthesised layout is byte-identical to
//! the original for none of the 2,338 staged RBM documents, so a fallback that
//! fired unconditionally would rewrite every file it touched and still pass a
//! "does it re-read" check.

use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::rbm::schema::PsoType;
use or_io::meta::binary::synth::rbm_layout_for_instance;
use or_io::meta::binary::type_tables;
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::{BinaryFormat, Diagnostics, MetaInstance, SchemaLibrary, Value};

const REPORTED: usize = 10;

/// The name of a top-level member that is a **heap** array with something in
/// it, which is the only kind that can grow.
///
/// ⚠ An inline `char[N]`-style run cannot: its length is in the type table,
/// so a synthesiser rebuilds it at the declared count and the writer refuses
/// the extra element a second time. Picking one would make this suite report
/// a refusal it caused itself.
fn a_growable_array(
    inst: &MetaInstance,
    layout: &or_io::meta::binary::layout::RbmLayout,
) -> Option<String> {
    let idx = support::hash_index();
    let root = layout.model.block(layout.model.root_block_id as i32)?;
    let class = layout.model.class_for(root.data_type)?;
    for entry in &class.entries {
        if !matches!(
            entry.data_type,
            PsoType::AtArray32
                | PsoType::AtArray64
                | PsoType::AtArray32With32BitIdx
                | PsoType::AtArray64With32BitIdx
        ) {
            continue;
        }
        let name = idx.name_or_literal(entry.name_hash);
        if let Some(Value::Array(items)) = inst.get(&name) {
            if !items.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn grow(inst: &MetaInstance, field: &str) -> MetaInstance {
    let mut grown = inst.clone();
    if let Some(Value::Array(items)) = grown.get(field).cloned() {
        let mut items = items;
        let last = items.last().cloned().expect("non-empty");
        items.push(last);
        grown.set(field.to_string(), Value::Array(items));
    }
    grown
}

fn encode_with_donor(layout: &MetaLayout, inst: &MetaInstance) -> or_io::meta::Result<Vec<u8>> {
    write_meta_binary(BinaryFormat::Rbm, Some(layout), inst)
}

fn payload_of(bytes: &[u8]) -> Option<Vec<u8>> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    Some(Rsc7::open(bytes).ok()?.data)
}

#[test]
fn an_array_that_grew_encodes_and_one_that_did_not_is_untouched() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "an_array_that_grew_encodes_and_one_that_did_not_is_untouched",
            "no RBM fixtures present",
        );
        return;
    }

    let mut exercised = 0usize;
    let mut no_growable_array = 0usize;
    let mut not_refused: Vec<String> = Vec::new();
    let mut synthesis_failed: Vec<String> = Vec::new();
    let mut lost_the_element: Vec<String> = Vec::new();
    let mut disturbed: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((original, layout)) =
            parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
        else {
            continue;
        };
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        let Some(field) = a_growable_array(&original, rbm) else {
            no_growable_array += 1;
            continue;
        };
        let before = match original.get(&field) {
            Some(Value::Array(items)) => items.len(),
            _ => continue,
        };
        let grown = grow(&original, &field);

        // The document's own layout has to refuse, and to refuse *this* way:
        // that refusal is the whole trigger.
        match encode_with_donor(&layout, &grown) {
            Err(e) if e.outgrew_its_layout() => {}
            Err(e) => {
                not_refused.push(format!("{}: refused as something else: {e}", fixture.name));
                continue;
            }
            Ok(_) => {
                not_refused.push(format!(
                    "{}: the original layout accepted {} elements where it allocated {before}",
                    fixture.name,
                    before + 1
                ));
                continue;
            }
        }

        let library =
            type_tables::extend(&SchemaLibrary::default(), &type_tables::of_rbm(&rbm.model))
                .into_owned();
        let synthesised = match rbm_layout_for_instance(&library, &grown, Some(&rbm.model)) {
            Ok(l) => MetaLayout {
                format: BinaryFormat::Rbm,
                container: ContainerLayout::Rbm(Box::new(l)),
            },
            Err(e) => {
                synthesis_failed.push(format!("{}: {e}", fixture.name));
                continue;
            }
        };
        let bytes = match encode_with_donor(&synthesised, &grown) {
            Ok(b) => b,
            Err(e) => {
                synthesis_failed.push(format!("{}: encode: {e}", fixture.name));
                continue;
            }
        };

        let mut diags = Diagnostics::lenient();
        match parse_meta_binary_full(support::schema(), &bytes, &mut diags) {
            Ok((back, _)) => match back.get(&field) {
                Some(Value::Array(items)) if items.len() == before + 1 => {}
                Some(Value::Array(items)) => lost_the_element.push(format!(
                    "{}: {field} came back with {} elements, not {}",
                    fixture.name,
                    items.len(),
                    before + 1
                )),
                other => lost_the_element
                    .push(format!("{}: {field} came back as {other:?}", fixture.name)),
            },
            Err(e) => lost_the_element.push(format!("{}: re-read: {e}", fixture.name)),
        }

        // ⚠ The RSC7 header and the *decompressed* payload of the ungrown
        // document, never the file: RBM's payload goes back under a fresh
        // DEFLATE and Rockstar's compressor is not reproducible, so comparing
        // whole files would fail for a reason that has nothing to do with
        // whether the fallback fired.
        match encode_with_donor(&layout, &original) {
            Ok(same) => {
                if same[..16] != fixture.bytes[..16] {
                    disturbed.push(format!("{}: the RSC7 header changed", fixture.name));
                } else if payload_of(&same) != payload_of(&fixture.bytes) {
                    disturbed.push(format!("{}: the payload changed", fixture.name));
                }
            }
            Err(e) => disturbed.push(format!("{}: would not re-encode at all: {e}", fixture.name)),
        }

        exercised += 1;
    }

    println!(
        "growth exercised on {exercised} of {} RBM fixtures; {no_growable_array} hold no \
         non-empty heap array to grow",
        fixtures.len()
    );
    for (what, list) in [
        ("did not refuse a grown array", &not_refused),
        ("could not be synthesised", &synthesis_failed),
        ("lost the added element", &lost_the_element),
        ("no longer re-encode unchanged", &disturbed),
    ] {
        assert!(
            list.is_empty(),
            "{} of {exercised} documents {what}:\n  {}",
            list.len(),
            list.iter()
                .take(REPORTED)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    assert!(
        exercised > 0,
        "{} RBM fixtures are staged and none reached the growth path, so this run asserted \
         nothing",
        fixtures.len()
    );
}

/// How many documents would change if the fallback fired when nothing had
/// grown, which is why the trigger is the refusal rather than a mode flag.
#[test]
fn synthesising_a_document_that_did_not_grow_would_change_it() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "synthesising_a_document_that_did_not_grow_would_change_it",
            "no RBM fixtures present",
        );
        return;
    }
    let mut compared = 0usize;
    let mut identical = 0usize;

    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((original, layout)) =
            parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
        else {
            continue;
        };
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        let library =
            type_tables::extend(&SchemaLibrary::default(), &type_tables::of_rbm(&rbm.model))
                .into_owned();
        let Ok(synthesised) = rbm_layout_for_instance(&library, &original, Some(&rbm.model)) else {
            continue;
        };
        compared += 1;
        if synthesised.decompressed == rbm.decompressed {
            identical += 1;
        }
    }

    println!("{identical} of {compared} RBM documents synthesise to the bytes they arrived as");
    assert!(
        compared > 0,
        "nothing was compared, so this run says nothing about the cost of synthesising"
    );
    assert_eq!(
        identical, 0,
        "{identical} of {compared} documents synthesise back to themselves, so the argument \
         that synthesis has to stay a fallback no longer holds and this suite should be \
         re-read rather than deleted"
    );
}
