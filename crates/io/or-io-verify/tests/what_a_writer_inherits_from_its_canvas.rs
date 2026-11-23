//! Does any declared field of a canonical save come back from the reservation
//! rather than a write?
//!
//! A writer that misses a field leaves the reservation's zero, a plausible
//! null, count, flag or `0.0`, and the file parses, reloads and passes every
//! census; a round trip compares it with a source field that was often also
//! zero. The poison canvas (`or_io::write::coverage_with`) builds each tree
//! under two fills and attributes each byte they disagree about to the field
//! that owns it. `owned_saves_as_tree` holds an owned save to its tree's
//! bytes, so the tree's build is the owned value's.

use std::collections::{BTreeMap, BTreeSet};

use or_io::desc::Pack;
use or_io::gta5::formats::FORMATS;
use or_io::write::{coverage_with, BlobWriters};
use or_io::Generation;
use or_io_verify::gates::{self, name, Opened};

/// The codec writers a format's canonical save runs.
fn writers(ext: &str) -> BlobWriters {
    match ext {
        "ydr" | "ydd" | "ypt" => or_io::gta5::drawable_codecs::writers(),
        "yft" => or_io::gta5::fragment::writers(),
        "ybn" | "ybd" | "yld" => or_io::gta5::bounds::writers(),
        _ => BlobWriters::new(),
    }
}

/// A format with codecs is on [`writers`]'s list, and one without is not.
#[test]
fn every_format_with_codecs_names_its_writers() {
    for fmt in FORMATS {
        assert_eq!(
            writers(fmt.ext).is_empty(),
            fmt.covered().is_empty(),
            ".{}: `writers` disagrees with the format's own codecs",
            fmt.ext
        );
    }
}

#[test]
fn no_declared_field_comes_back_from_the_reservation() {
    const WHAT: &str = "no_declared_field_comes_back_from_the_reservation";
    let mut failures = Vec::new();
    for fmt in FORMATS {
        let ext = fmt.ext;
        let writers = writers(ext);
        for target in fmt.targets().collect::<BTreeSet<_>>() {
            let Some(paths) = gates::files(ext, Some(target), WHAT) else {
                continue;
            };
            let pack = gates::pack(ext, target);
            let (mut untouched, mut total) = (0usize, 0usize);
            let mut inherited: BTreeMap<String, usize> = BTreeMap::new();
            for path in &paths {
                let opened = Opened::read(path);
                let tree = match fmt.read(target, opened.image()) {
                    Ok(tree) => tree,
                    Err(e) => {
                        failures.push(format!(".{ext} {}: the read: {e}", name(path)));
                        continue;
                    }
                };
                drop(opened);
                let canvas = match coverage_with(&pack, &tree, &writers) {
                    Ok(canvas) => canvas,
                    Err(e) => {
                        failures.push(format!(".{ext} {}: the build: {e}", name(path)));
                        continue;
                    }
                };
                untouched += canvas.untouched_bytes();
                total += canvas.total_bytes();
                for found in canvas.inherited(&pack) {
                    *inherited.entry(found.to_string()).or_default() += 1;
                }
            }
            println!(
                ".{ext} {}: {} file(s), {untouched} of {total} heap byte(s) never written, \
                 {} declared field(s) inherited",
                target.slug(),
                paths.len(),
                inherited.len()
            );
            for (what, files) in inherited {
                failures.push(format!(
                    ".{ext} {}: {what}, in {files} file(s)",
                    target.slug()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "a declared field whose bytes nobody wrote takes whatever the reservation \
         held, which is zero and therefore plausible; the writer should write it, or \
         the declaration should name it padding:\n  {}",
        failures.join("\n  ")
    );
}

/// The canvas catches a planted inheritance on a real file: moving
/// `PhBound::radius_around_centroid` four bytes makes the build write it
/// there, so nothing writes its true offset. Attributed against the moved
/// description the build used, nothing is inherited: the control that the
/// instrument reports unwritten bytes and not the perturbation.
#[test]
fn a_planted_inheritance_is_caught_and_named() {
    const WHAT: &str = "a_planted_inheritance_is_caught_and_named";
    let (ext, target) = ("ybn", Generation::Gen8);
    let Some(paths) = gates::files(ext, Some(target), WHAT) else {
        return;
    };
    let fmt = gates::format(ext);
    let pack = gates::pack(ext, target);
    let tree = fmt
        .read(target, Opened::read(&paths[0]).image())
        .expect("a staged bound reads");

    let (ty, field) = ("PhBound", "radius_around_centroid");
    let mut moved: Pack = (*pack).clone();
    let declared = moved
        .types
        .iter_mut()
        .find(|t| t.name == ty)
        .expect("the description declares PhBound")
        .fields
        .iter_mut()
        .find(|f| f.name == field)
        .expect("PhBound has a radius_around_centroid");
    declared.offset += 4;

    let canvas = coverage_with(&moved, &tree, &writers(ext)).expect("the moved description builds");
    let against_the_truth = canvas.inherited(&pack);
    let named = against_the_truth
        .iter()
        .find(|i| i.type_name == ty && i.field == field)
        .unwrap_or_else(|| {
            panic!(
                "nothing wrote {ty}::{field} and the canvas did not say so: {against_the_truth:?}"
            )
        });
    assert!(named.instances > 0 && named.bytes > 0, "{named}");
    assert_eq!(
        canvas.inherited(&moved),
        Vec::new(),
        "against the description the build used, nothing is inherited"
    );
}
