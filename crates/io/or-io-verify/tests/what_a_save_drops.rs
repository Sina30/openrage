//! Every declared format, over the corpus: what a canonical save loses. Two
//! classes: a pointer the file holds non-null that the save writes null,
//! because nothing describes what it points at, and a 64-bit number field
//! whose value is an address in the file, which a save copies and which then
//! names bytes the rebuild moved.
//!
//! ⚠ **The list is exact in both directions.** A site that starts losing data
//! fails this, and so does a site that stops, so fixing one means removing its
//! row: the list, [`STILL_LOST`] with [`RULED`], is what the roadmap's
//! inventory is read from, and the padding census reads it too.

use std::collections::{BTreeMap, BTreeSet};

use or_io::desc::{FieldRepr, Image, IntWidth, Pack, Scalar, Value};
use or_io::gta5::formats::FORMATS;
use or_io::write::Dropped;
use or_io_verify::census::{self, RULED, STILL_LOST};
use or_io_verify::gates::{files, pack, Opened};

/// Per `Type.field`, the 64-bit numbers `v` holds whose value is an address in `image`.
fn numbers_holding_addresses(
    pack: &Pack,
    image: Image<'_>,
    v: &Value,
    out: &mut BTreeMap<String, usize>,
) {
    match v {
        Value::Struct(r) => {
            numbers_in(pack, image, r.type_name(), r.bytes(), 0, out);
            for (_, child) in r.children() {
                numbers_holding_addresses(pack, image, child, out);
            }
        }
        Value::Records(run) => {
            for r in run.iter() {
                numbers_in(pack, image, run.type_name(), r.bytes(), 0, out);
            }
        }
        Value::Array { elements, .. } => {
            for e in elements {
                numbers_holding_addresses(pack, image, e, out);
            }
        }
        Value::Ptr {
            target: Some(t), ..
        } => numbers_holding_addresses(pack, image, t, out),
        Value::Blob { parts, .. } => {
            for (_, p) in parts {
                numbers_holding_addresses(pack, image, p, out);
            }
        }
        _ => {}
    }
}

fn numbers_in(
    pack: &Pack,
    image: Image<'_>,
    type_name: &str,
    bytes: &[u8],
    base: usize,
    out: &mut BTreeMap<String, usize>,
) {
    let Some(ty) = pack.type_by_name(type_name) else {
        return;
    };
    for f in &ty.fields {
        let at = base + f.offset;
        let words = match &f.repr {
            FieldRepr::Scalar(Scalar::Int(IntWidth::U64)) => 1,
            FieldRepr::ScalarArray {
                element: Scalar::Int(IntWidth::U64),
                count,
            } => *count,
            FieldRepr::Inline { type_name } => {
                numbers_in(pack, image, type_name, bytes, at, out);
                continue;
            }
            FieldRepr::InlineArray { type_name, count } => {
                let size = pack.type_by_name(type_name).map_or(0, |t| t.size);
                for i in 0..*count {
                    numbers_in(pack, image, type_name, bytes, at + i * size, out);
                }
                continue;
            }
            _ => continue,
        };
        for i in 0..words {
            let Some(word) = bytes
                .get(at + 8 * i..at + 8 * i + 8)
                .map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")))
            else {
                continue;
            };
            if word != 0 && image.resolve(word).is_ok() {
                *out.entry(format!("{}.{}", ty.name, f.name)).or_default() += 1;
            }
        }
    }
}

/// Per site in `wanted`, how many instances a walk from `root` reaches hold a non-null word there.
fn non_null(
    pack: &Pack,
    image: Image<'_>,
    root: &str,
    wanted: &BTreeSet<(String, String)>,
) -> BTreeMap<(String, String), usize> {
    if wanted.is_empty() {
        return BTreeMap::new();
    }
    let (_, sites) = census::walk(pack, image, root, 0);
    let mut out = BTreeMap::new();
    for (ty, field) in wanted {
        let Some(f) = pack.type_by_name(ty).and_then(|t| t.field_by_name(field)) else {
            continue;
        };
        let n = sites
            .iter()
            .filter(|(t, at)| t == ty && image.pointer_at(at + f.offset).is_ok_and(|w| w != 0))
            .count();
        out.insert((ty.clone(), field.clone()), n);
    }
    out
}

#[test]
fn what_a_save_drops_is_the_list() {
    let mut lost: BTreeMap<(String, String, String, &str), (usize, usize)> = BTreeMap::new();
    let mut read = 0usize;
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for fmt in FORMATS {
        // ⚠ `targets()` repeats a target with two roots, and `files` takes a target, not a root.
        for t in fmt.targets().collect::<BTreeSet<_>>() {
            let Some(paths) = files(fmt.ext, Some(t), "what a save drops") else {
                continue;
            };
            let pack = pack(fmt.ext, t);
            for path in &paths {
                let opened = Opened::read(path);
                let image = opened.image();
                let tree = fmt
                    .read(t, image)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let Ok((saved, report)) = fmt.save_reporting(t, &tree) else {
                    continue;
                };
                read += 1;
                seen.insert((fmt.ext.to_string(), t.slug().to_string()));
                // ⚠ The builder reports a pointer it writes null even where a
                // codec writes it again afterwards, as the parameter block's
                // does its slot tables: a site the saved file holds non-null
                // at least as often as the shipped one is carried, not lost.
                let reported: BTreeSet<(String, String)> = report
                    .dropped_non_null()
                    .map(|d| (d.type_name.clone(), d.field.clone()))
                    .collect();
                let root = fmt.root_name(t).expect("declared");
                let shipped = non_null(&pack, image, &root, &reported);
                let saved = Opened::of(&saved);
                let after = non_null(&pack, saved.image(), &root, &reported);
                let carried = |d: &Dropped| {
                    let site = (d.type_name.clone(), d.field.clone());
                    let n = shipped.get(&site).copied().unwrap_or(0);
                    n > 0 && after.get(&site).copied().unwrap_or(0) >= n
                };
                let mut dropped_here = BTreeSet::new();
                for d in report.dropped_non_null().filter(|d| !carried(d)) {
                    let class = if image.resolve(d.address).is_ok() {
                        "null on save"
                    } else {
                        "null on save, and not an address in the file"
                    };
                    let key = (
                        fmt.ext.to_string(),
                        t.slug().to_string(),
                        format!("{}.{}", d.type_name, d.field),
                        class,
                    );
                    let e = lost.entry(key.clone()).or_default();
                    e.0 += 1;
                    if dropped_here.insert(key) {
                        e.1 += 1;
                    }
                }
                let mut numbers = BTreeMap::new();
                numbers_holding_addresses(&pack, image, &tree, &mut numbers);
                for (site, n) in numbers {
                    let e = lost
                        .entry((
                            fmt.ext.to_string(),
                            t.slug().to_string(),
                            site,
                            "an address carried as a number",
                        ))
                        .or_default();
                    e.0 += n;
                    e.1 += 1;
                }
            }
        }
    }
    println!("{read} files saved:");
    for ((ext, t, site, class), (sites, files)) in &lost {
        println!("  .{ext} {t}: {site} — {class}: {sites} in {files} file(s)");
    }
    for (site, why) in RULED {
        println!("  by decision, {site}: {why}");
    }
    // A word that is not an address in the file names nothing a save could keep.
    let measured: BTreeSet<(&str, &str, &str, &str)> = lost
        .keys()
        .filter(|(_, _, s, c)| {
            !RULED.iter().any(|(r, _)| r == s)
                && *c != "null on save, and not an address in the file"
        })
        .map(|(e, t, s, c)| (e.as_str(), t.as_str(), s.as_str(), *c))
        .collect();
    // A format with nothing staged here measured nothing, so its rows are not judged.
    let expected: BTreeSet<(&str, &str, &str, &str)> = STILL_LOST
        .iter()
        .filter(|(e, t, _, _)| seen.contains(&(e.to_string(), t.to_string())))
        .copied()
        .collect();
    let new: Vec<_> = measured.difference(&expected).collect();
    let fixed: Vec<_> = expected.difference(&measured).collect();
    assert!(
        new.is_empty(),
        "a save loses data at sites nobody listed: {new:#?}"
    );
    assert!(
        fixed.is_empty(),
        "listed sites no save loses any more: {fixed:#?}"
    );
}
