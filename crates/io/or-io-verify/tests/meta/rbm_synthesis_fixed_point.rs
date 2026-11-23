//! The **XML leg** for RBM: export to XML, import it back, and encode
//! against a layout synthesised from the value tree alone. The second pass
//! has to reproduce the first byte for byte.
//!
//! ⚠ **No donor reaches the output.** The layout the document was read with
//! is used for one thing — recovering the structure definitions, from the
//! file's own type table — and is then dropped. Every byte of `C1` and `C2`
//! comes from [`or_io::meta::binary::synth`], so a defect in synthesis reaches
//! this suite and a corpus-wide `binary → binary` round-trip cannot.
//!
//! ⚠ The property is a **fixed point**, not byte-identity with the original:
//! synthesis chooses its own page shape and block allocation, and RBM's
//! payload goes back under a fresh DEFLATE that Rockstar's compressor is not
//! reproducible against, so only the *decompressed* payload is compared.

use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::synth::rbm_layout_for_instance;
use or_io::meta::binary::type_tables;
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::instance::parse_meta;
use or_io::meta::{write_meta, BinaryFormat, Diagnostics, MetaInstance, SchemaLibrary};
use rayon::prelude::*;

/// How many documents a failing run names before it stops listing them.
const REPORTED: usize = 12;

/// How much of the corpus may drop out before the run stops being evidence.
///
/// ⚠ **This bound is the only thing that catches one of the two
/// perturbations.** Seeding synthesised padding with `0xAB` instead of zero
/// leaves all seven documents that still reach the property a fixed point and
/// takes the exercised count from 2,338 to 7, so a suite asserting only the
/// mismatch list reports that green. Ordering the type table by the lookup
/// map's iteration puts 2,273 of 2,338 in the mismatch list with the
/// exercised count unmoved.
const MAX_REFUSED_PERCENT: usize = 5;

struct Leg {
    bytes: Vec<u8>,
}

/// XML out, XML in, and a layout invented for what came back.
fn synthesise(library: &SchemaLibrary, instance: &MetaInstance) -> or_io::meta::Result<Leg> {
    let xml = write_meta(instance)?;
    let reimported = parse_meta(library, xml.as_bytes())?;
    let layout = MetaLayout {
        format: BinaryFormat::Rbm,
        container: ContainerLayout::Rbm(Box::new(rbm_layout_for_instance(
            library,
            &reimported,
            None,
        )?)),
    };
    let bytes = write_meta_binary(BinaryFormat::Rbm, Some(&layout), &reimported)?;
    Ok(Leg { bytes })
}

fn payload_of(bytes: &[u8]) -> Option<Vec<u8>> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    Some(Rsc7::open(bytes).ok()?.data)
}

/// What one fixture did, read off a worker thread.
enum Verdict {
    Skipped,
    Refused(String),
    Differed(String),
    /// Synthesised twice; the difference between the two, if there is one.
    Exercised(Option<String>),
}

#[test]
fn a_synthesised_rbm_is_a_fixed_point_of_export_and_import() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "a_synthesised_rbm_is_a_fixed_point_of_export_and_import",
            "no RBM fixtures present",
        );
        return;
    }

    // Each fixture synthesises on its own, so the cores share the corpus; the verdicts fold in file order.
    let verdicts: Vec<Verdict> = fixtures
        .par_iter()
        .map(|fixture| {
            let mut diags = Diagnostics::lenient();
            let Ok((original, layout)) =
                parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
            else {
                return Verdict::Skipped;
            };
            let ContainerLayout::Rbm(rbm) = &layout.container else {
                return Verdict::Skipped;
            };
            let library =
                type_tables::extend(&SchemaLibrary::default(), &type_tables::of_rbm(&rbm.model))
                    .into_owned();

            let first = match synthesise(&library, &original) {
                Ok(leg) => leg,
                Err(e) => return Verdict::Refused(format!("{}: {e}", fixture.name)),
            };
            let mut diags = Diagnostics::lenient();
            let Ok((readback, _)) =
                parse_meta_binary_full(support::schema(), &first.bytes, &mut diags)
            else {
                return Verdict::Differed(format!("{}: C1 does not parse", fixture.name));
            };
            let second = match synthesise(&library, &readback) {
                Ok(leg) => leg,
                Err(e) => return Verdict::Differed(format!("{}: C2 refused: {e}", fixture.name)),
            };

            // ⚠ The *decompressed* payload, because RBM's is raw DEFLATE and a
            // compressor is not part of what synthesis decides.
            let (Some(a), Some(b)) = (payload_of(&first.bytes), payload_of(&second.bytes)) else {
                return Verdict::Exercised(Some(format!("{}: could not decompress", fixture.name)));
            };
            if first.bytes[..16] != second.bytes[..16] {
                return Verdict::Exercised(Some(format!(
                    "{}: the invented RSC7 header differs",
                    fixture.name
                )));
            }
            Verdict::Exercised((a != b).then(|| {
                let at = a
                    .iter()
                    .zip(&b)
                    .position(|(x, y)| x != y)
                    .unwrap_or(a.len().min(b.len()));
                format!(
                    "{}: C1 payload is {} bytes, C2 is {}, first difference at {at:#X}",
                    fixture.name,
                    a.len(),
                    b.len()
                )
            }))
        })
        .collect();
    let mut exercised = 0usize;
    let mut refused: Vec<String> = Vec::new();
    let mut differed: Vec<String> = Vec::new();
    for verdict in verdicts {
        match verdict {
            Verdict::Skipped => {}
            Verdict::Refused(why) => refused.push(why),
            Verdict::Differed(why) => differed.push(why),
            Verdict::Exercised(why) => {
                exercised += 1;
                differed.extend(why);
            }
        }
    }

    println!(
        "fixed point exercised on {exercised} of {} RBM fixtures; {} were refused before the \
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
        "no RBM fixture reached the fixed point at all — {} of {} were refused before the \
         property could be tested, so this run asserted nothing",
        refused.len(),
        fixtures.len()
    );
    assert!(
        refused.len() * 100 <= fixtures.len() * MAX_REFUSED_PERCENT,
        "{} of {} RBM fixtures were refused before the property could be tested, over the \
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
fn rbm_synthesis_reaches_the_shapes_real_documents_have() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "rbm_synthesis_reaches_the_shapes_real_documents_have",
            "no RBM fixtures present",
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
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        parsed += 1;
        let library =
            type_tables::extend(&SchemaLibrary::default(), &type_tables::of_rbm(&rbm.model))
                .into_owned();
        match rbm_layout_for_instance(&library, &instance, None) {
            Ok(_) => laid_out += 1,
            Err(e) => reasons.push(format!("{}: {e}", fixture.name)),
        }
    }

    println!("synthesis laid out {laid_out} of {parsed} RBM documents");
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

/// Two synthesises of the same value tree have to agree byte for byte, and
/// the run has to say how many documents it compared.
#[test]
fn synthesis_is_reproducible() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus("synthesis_is_reproducible", "no RBM fixtures present");
        return;
    }
    let mut exercised = 0usize;
    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) =
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
        let Ok(a) = rbm_layout_for_instance(&library, &instance, None) else {
            continue;
        };
        let b = rbm_layout_for_instance(&library, &instance, None).expect("the second agrees");
        assert_eq!(
            a.decompressed, b.decompressed,
            "{}: two synthesised pages differ",
            fixture.name
        );
        exercised += 1;
    }
    println!("two synthesised pages agreed on {exercised} documents");
    assert!(
        exercised > 0,
        "{} RBM fixtures are staged and none was synthesised twice, so this run compared \
         nothing",
        fixtures.len()
    );
}

/// A class's `signature` is a value only the game's own parser holds, so
/// synthesis takes it from a donor that already names the type. Everything
/// else — offsets, ordering, page shape, padding — has to be identical with
/// and without one, or the fixed point would depend on which file a caller
/// happened to have open.
#[test]
fn a_donor_changes_nothing_but_the_signature_words() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "a_donor_changes_nothing_but_the_signature_words",
            "no RBM fixtures present",
        );
        return;
    }
    let mut exercised = 0usize;
    let mut carried = 0usize;

    for fixture in fixtures.iter().take(200) {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) =
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
        let Ok(bare) = rbm_layout_for_instance(&library, &instance, None) else {
            continue;
        };
        let with = rbm_layout_for_instance(&library, &instance, Some(&rbm.model))
            .expect("a donor cannot make synthesis fail");
        exercised += 1;

        assert_eq!(
            bare.decompressed.len(),
            with.decompressed.len(),
            "{}: a donor changed the page size",
            fixture.name
        );
        assert_eq!(
            bare.header.class_refs_ptr, with.header.class_refs_ptr,
            "{}: a donor moved the class table",
            fixture.name
        );

        // Every difference has to land in a class or enum header's
        // `signature` word, which is the four bytes at +0x04.
        let signature_words: std::collections::BTreeSet<usize> = (0..bare.header.class_count)
            .map(|i| {
                (bare.header.class_refs_ptr & 0x0FFF_FFFF) as usize + usize::from(i) * 0x20 + 4
            })
            .chain((0..bare.header.enum_count).map(|i| {
                (bare.header.enum_refs_ptr & 0x0FFF_FFFF) as usize + usize::from(i) * 0x18 + 4
            }))
            .collect();

        for (at, (x, y)) in bare.decompressed.iter().zip(&with.decompressed).enumerate() {
            if x == y {
                continue;
            }
            carried += 1;
            let word = at & !3;
            assert!(
                signature_words.contains(&word),
                "{}: a donor changed byte {at:#X}, which is not a signature word",
                fixture.name
            );
        }
    }

    println!(
        "{exercised} documents synthesised with and without a donor; {carried} bytes differed, \
         all of them signature words"
    );
    assert!(
        exercised > 0,
        "no document was synthesised both ways, so this run compared nothing"
    );
    assert!(
        carried > 0,
        "a donor changed nothing at all across {exercised} documents, so this run would pass \
         for a donor that was never consulted"
    );
}
