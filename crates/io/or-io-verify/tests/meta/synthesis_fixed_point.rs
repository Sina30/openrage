//! The **XML leg**: export to XML, import it back, and encode against a
//! layout synthesised from the value tree alone. The second pass has to
//! reproduce the first byte for byte.
//!
//! ⚠ **No donor reaches the output.** The layout the document was read with
//! is used only to recover the structure definitions from the file's own type
//! table, so every byte of `C1` and `C2` comes from [`or_io::meta::binary::synth`]
//! and a defect in synthesis reaches this suite where a corpus-wide
//! `binary → binary` round-trip cannot. Over 3,527 PSO documents with the
//! 3,128-struct schema snapshot bound, ordering synthesis's struct fields by
//! `HashMap` iteration puts 3,488 of 3,526 documents in the failure list.
//!
//! ⚠ The property is a **fixed point**, not byte-identity with the original:
//! synthesis chooses its own block allocation, and an embedded `char[N]` comes
//! back as a heap string because `FieldType` has no spelling for a fixed buffer.
//!
//! Nothing here gates on `has_default_library`: a PSO file carries a complete
//! type table of its own, so the leg runs on a clean machine and a snapshot
//! changes only the names.

use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::synth::pso_layout_for_instance;
use or_io::meta::binary::type_tables;
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::instance::parse_meta;
use or_io::meta::{write_meta, BinaryFormat, Diagnostics, MetaInstance, SchemaLibrary};

/// How many documents a failing run names before it stops listing them.
const REPORTED: usize = 12;

/// How much of the corpus may drop out before the run stops being evidence.
///
/// ⚠ Seeding the synthesised padding with `0xAB` instead of zero leaves every
/// *exercised* document a fixed point and takes the exercised count from 3,526
/// to 67, so a suite asserting only the mismatches reports that green having
/// checked under 2%. This bound catches that; it is not a tolerance for an
/// awkward corpus.
const MAX_REFUSED_PERCENT: usize = 2;

struct Leg {
    bytes: Vec<u8>,
    instance: MetaInstance,
}

/// XML out, XML in, and a layout invented for what came back.
fn synthesise(library: &SchemaLibrary, instance: &MetaInstance) -> or_io::meta::Result<Leg> {
    let xml = write_meta(instance)?;
    let reimported = parse_meta(library, xml.as_bytes())?;
    let layout = MetaLayout {
        format: BinaryFormat::Pso,
        container: ContainerLayout::Pso(pso_layout_for_instance(library, &reimported)?),
    };
    let bytes = write_meta_binary(BinaryFormat::Pso, Some(&layout), &reimported)?;
    Ok(Leg {
        bytes,
        instance: reimported,
    })
}

#[test]
fn a_synthesised_pso_is_a_fixed_point_of_export_and_import() {
    let fixtures = Corpus::of_format(BinaryFormat::Pso);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "a_synthesised_pso_is_a_fixed_point_of_export_and_import",
            "no PSO fixtures present",
        );
        return;
    }

    let mut exercised = 0usize;
    let mut refused: Vec<String> = Vec::new();
    let mut differed = Vec::new();

    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((original, layout)) =
            parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
        else {
            continue;
        };
        let ContainerLayout::Pso(pso) = &layout.container else {
            continue;
        };
        let library =
            type_tables::extend(&SchemaLibrary::default(), &type_tables::of_pso(&pso.schema))
                .into_owned();

        let first = match synthesise(&library, &original) {
            Ok(leg) => leg,
            Err(e) => {
                refused.push(format!("{}: {e}", fixture.name));
                continue;
            }
        };
        let mut diags = Diagnostics::lenient();
        let Ok(readback) = parse_meta_binary_full(support::schema(), &first.bytes, &mut diags)
        else {
            differed.push(format!("{}: C1 does not parse", fixture.name));
            continue;
        };
        let second = match synthesise(&library, &readback.0) {
            Ok(leg) => leg,
            Err(e) => {
                differed.push(format!("{}: C2 refused: {e}", fixture.name));
                continue;
            }
        };

        exercised += 1;
        if first.bytes != second.bytes {
            let at = first
                .bytes
                .iter()
                .zip(&second.bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(first.bytes.len().min(second.bytes.len()));
            differed.push(format!(
                "{}: C1 is {} bytes, C2 is {}, first difference at {at:#X}",
                fixture.name,
                first.bytes.len(),
                second.bytes.len()
            ));
        }
        let _ = &first.instance;
        let _ = &second.instance;
    }

    println!(
        "fixed point exercised on {exercised} of {} PSO fixtures; {} were refused before the \
         property could be tested",
        fixtures.len(),
        refused.len()
    );
    if !refused.is_empty() {
        println!(
            "not exercised:\n  {}",
            refused
                .iter()
                .take(REPORTED)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    assert!(
        exercised > 0,
        "no PSO fixture reached the fixed point at all — {} of {} were refused before the \
         property could be tested, so this run asserted nothing",
        refused.len(),
        fixtures.len()
    );
    assert!(
        refused.len() * 100 <= fixtures.len() * MAX_REFUSED_PERCENT,
        "{} of {} PSO fixtures were refused before the property could be tested, over the \
         {MAX_REFUSED_PERCENT}% this suite treats as evidence:\n  {}",
        refused.len(),
        fixtures.len(),
        refused
            .iter()
            .take(REPORTED)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        differed.is_empty(),
        "{} of {exercised} exercised documents are not a fixed point:\n  {}",
        differed.len(),
        differed
            .iter()
            .take(REPORTED)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// What synthesis can lay out, over the documents the corpus actually holds.
///
/// The figure is printed rather than pinned: it is a property of the staged
/// corpus, and a test that asserts a corpus-sized number fails for whoever
/// stages a different one.
#[test]
fn synthesis_reaches_the_shapes_real_documents_have() {
    let fixtures = Corpus::of_format(BinaryFormat::Pso);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "synthesis_reaches_the_shapes_real_documents_have",
            "no PSO fixtures present",
        );
        return;
    }

    let mut laid_out = 0usize;
    let mut parsed = 0usize;
    let mut reasons: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) =
            parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
        else {
            continue;
        };
        let ContainerLayout::Pso(pso) = &layout.container else {
            continue;
        };
        parsed += 1;
        let library =
            type_tables::extend(&SchemaLibrary::default(), &type_tables::of_pso(&pso.schema))
                .into_owned();
        match pso_layout_for_instance(&library, &instance) {
            Ok(_) => laid_out += 1,
            Err(e) => reasons.push(format!("{}: {e}", fixture.name)),
        }
    }

    println!("synthesis laid out {laid_out} of {parsed} PSO documents");
    if !reasons.is_empty() {
        println!(
            "refused:\n  {}",
            reasons
                .iter()
                .take(REPORTED)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    assert!(
        laid_out > 0,
        "synthesis laid out nothing at all across {parsed} documents"
    );
}
