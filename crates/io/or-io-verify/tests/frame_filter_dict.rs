//! `.yfd` through its declarations, over the corpus: typed reads against the
//! tree, the recorded testimony re-keyed by offset, the canonical fixed point,
//! the alignment census and the checksum the `signature` field is named for.
//!
//! ⚠ An Enhanced install ships `.yfd` at the Legacy version and layout, so
//! every file is read with the one description, wherever it was staged.

use or_io::decl::{File, Raw, View};
use or_io::desc::{Image, Pack, Value};
use or_io::gta5::frame_filter_dict::{
    self, CrFrameFilterDictionary, CrFrameFilterMultiWeight, TrackIdIndex,
};
use or_io::Generation;
use or_io_verify::gates::{self, file_tree, files, moved_by, offset_of_run, Filed, Gates, Opened};

const EXT: &str = "yfd";
const T: Generation = Generation::Gen8;

fn pack() -> std::sync::Arc<Pack> {
    gates::pack(EXT, T)
}

fn yfd_files(what: &str) -> Option<Vec<std::path::PathBuf>> {
    files(EXT, None, what)
}

fn bytes_of<R: or_io::decl::Record>(view: &View<'_, R>) -> Vec<u8> {
    or_io::decl::Plain::as_bytes(View::get(view)).to_vec()
}

/// What the typed API reads of a file, in the same shape as [`Filed`].
fn typed(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: or_io::decl::ViewError| e.to_string();
    let mut out = Filed::default();
    let key = |v: usize, t: &str| (v, t.to_string());
    let dict = View::<Raw<CrFrameFilterDictionary>>::at_offset(image, 0).map_err(e)?;
    out.structs
        .insert(key(0, "PgDictionary<CrFrameFilter>"), bytes_of(&dict));
    let hashes = dict.codes().map_err(e)?;
    if let Some(at) = offset_of_run(image, &hashes) {
        out.runs
            .insert(at, hashes.iter().flat_map(|h| h.to_le_bytes()).collect());
    }
    let entries = dict.entries().map_err(e)?;
    for i in 0..entries.len() {
        let Some(filter) = entries.follow(i).map_err(e)? else {
            continue;
        };
        let at = View::offset(&filter);
        out.structs
            .insert(key(at, "CrFrameFilter"), bytes_of(&filter));
        let Some(weighted) = View::variant::<Raw<CrFrameFilterMultiWeight>>(&filter).map_err(e)?
        else {
            return Err(format!(
                "filter {i} is kind {}, which is not declared",
                { filter.r#type }.raw()
            ));
        };
        out.structs
            .insert(key(at, "CrFrameFilterMultiWeight"), bytes_of(&weighted));
        for entry in weighted.track_id_indices().map_err(e)?.views() {
            out.structs
                .insert(key(View::offset(&entry), "TrackIdIndex"), bytes_of(&entry));
        }
        let weights = weighted.weights().map_err(e)?;
        if let Some(at) = offset_of_run(image, &weights) {
            out.runs
                .insert(at, weights.iter().flat_map(|w| w.to_le_bytes()).collect());
        }
    }
    Ok(out)
}

const GATES: Gates = Gates {
    embedded: &["CrFrameFilter"],
    census_types: 4,
    ..Gates::new(EXT, T, ("TrackIdIndex", "track"))
};

or_io_verify::declared_gates! {
    gates: GATES,
    typed: typed,
    reads_typed: every_yfd_reads_typed_as_the_tree_reads,
    owned: CrFrameFilterDictionary => every_yfd_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_yfd_saves_to_a_fixed_point,
    census: every_yfd_instance_is_aligned_for_its_view,
    testimony: the_yfd_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_field_one_byte_late_is_named_by_the_testimony,
}

#[test]
fn a_tree_that_follows_the_wrong_run_differs_from_the_typed_read() {
    let Some(paths) = yfd_files("a misdeclared tree against the typed read") else {
        return;
    };
    // The entry table's header moved onto the weight table's.
    let late = moved_by(
        &pack(),
        "CrFrameFilterMultiWeight",
        "track_id_indices",
        0x10,
    );
    let opened = Opened::read(&paths[0]);
    let image = opened.image();
    let root = or_io::decl::intern(&gates::format(EXT).root_name(T).unwrap());
    let tree =
        or_io::desc::value::read(&late, or_io::desc::TypeName(root), image, 0).expect("reads");
    let mut from_tree = Filed::default();
    file_tree(image, &tree, Some(0), &mut from_tree);
    let from_view = typed(image).expect("reads");
    let differ = from_view
        .structs
        .iter()
        .filter(|(k, b)| k.1 == "TrackIdIndex" && from_tree.structs.get(*k) != Some(*b))
        .count();
    println!("{differ} typed entries the misdeclared tree does not hold as read");
    assert!(differ > 0);
}

/// Every weighted filter's `checksum` is the CRC-32 of its tables, which is what names the field.
#[test]
fn every_filter_checksum_is_the_crc_of_its_tables() {
    let Some(paths) = yfd_files("the .yfd checksum census") else {
        return;
    };
    let (mut filters, mut agree) = (0usize, 0usize);
    for path in &paths {
        let opened = Opened::read(path);
        let dict =
            View::<Raw<CrFrameFilterDictionary>>::at_offset(opened.image(), 0).expect("root");
        let entries = dict.entries().expect("entries");
        for i in 0..entries.len() {
            let filter = entries.follow(i).expect("a filter").expect("not null");
            let w = View::variant::<Raw<CrFrameFilterMultiWeight>>(&filter)
                .expect("reads")
                .expect("weighted");
            filters += 1;
            let crc = frame_filter_dict::checksum_raw(
                &w.track_id_indices().unwrap(),
                &w.weights().unwrap(),
            );
            agree += usize::from(crc == w.base.signature);
        }
    }
    println!("{agree} of {filters} filters carry the CRC-32 of their entry and weight tables");
    assert!(filters > 0);
    assert_eq!(agree, filters);
}

#[test]
fn an_owned_field_one_byte_late_does_not_save_as_the_tree() {
    let Some(paths) = yfd_files("a perturbed owned .yfd") else {
        return;
    };
    let file = std::fs::read(&paths[0]).unwrap();
    let mut owned = CrFrameFilterDictionary::from_bytes(&file).unwrap();
    owned.codes[0] ^= 1;
    let tree = gates::format(EXT)
        .read(T, Opened::of(&file).image())
        .unwrap();
    let from_tree = gates::format(EXT).save(T, &tree).unwrap();
    assert_ne!(
        gates::payload(&owned.to_bytes().unwrap()),
        gates::payload(&from_tree),
        "a changed field saved as the original"
    );
}

#[test]
fn a_yfd_the_game_would_misread_is_refused_before_it_is_written() {
    let Some(paths) = yfd_files("a broken .yfd refused") else {
        return;
    };
    let path = paths
        .iter()
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .expect("a file");
    let yfd = gates::format(EXT);
    let opened = Opened::read(path);
    let mut tree = yfd.read(T, opened.image()).expect("reads");
    yfd.save(T, &tree).expect("the file as shipped saves");
    // Swap two hashes: the dictionary is no longer searchable.
    let root = tree.record_mut().expect("a root");
    let hashes = root.child_mut("codes").expect("hashes");
    match hashes.target_mut().expect("a run") {
        Value::Array { elements, .. } => {
            assert!(elements.len() >= 2, "a dictionary with one filter");
            elements.swap(0, 1);
        }
        Value::Bytes(bytes) => {
            assert!(bytes.len() >= 8, "a dictionary with one filter");
            let (a, b) = bytes.split_at_mut(4);
            a.swap_with_slice(&mut b[..4]);
        }
        other => panic!("the hashes are held as {other:?}"),
    }
    let why = match yfd.save(T, &tree) {
        Ok(saved) => panic!("an unsearchable dictionary saved, {} bytes", saved.len()),
        Err(why) => why,
    };
    println!("{why}");
    assert!(why.to_string().contains("not ascending"), "{why}");
}

/// Every entry field and weight of every filter, through the typed API from the root.
#[inline(never)]
fn typed_sum(image: Image<'_>) -> u64 {
    let dict = View::<Raw<CrFrameFilterDictionary>>::at_offset(image, 0).expect("root");
    let table = dict.entries().expect("entries");
    let mut total = 0u64;
    for i in 0..table.len() {
        let Ok(Some(filter)) = table.follow(i) else {
            continue;
        };
        let Ok(Some(w)) = View::variant::<Raw<CrFrameFilterMultiWeight>>(&filter) else {
            continue;
        };
        for e in w.track_id_indices().expect("entries").iter() {
            total += u64::from(e.track) + u64::from(e.id) + e.index as u64;
        }
        for x in w.weights().expect("weights").iter() {
            total += u64::from(x.to_bits());
        }
    }
    total
}

#[derive(Clone, Copy)]
struct PlainEntry {
    track: u8,
    id: u16,
    index: i32,
}

/// The same sum over entries and weights already copied out into plain vectors.
#[inline(never)]
fn plain_sum(filters: &[(Vec<PlainEntry>, Vec<f32>)]) -> u64 {
    let mut total = 0u64;
    for (entries, weights) in filters {
        for e in entries {
            total += u64::from(e.track) + u64::from(e.id) + e.index as u64;
        }
        for x in weights {
            total += u64::from(x.to_bits());
        }
    }
    total
}

type Runs<'a> = Vec<(&'a [Raw<TrackIdIndex>], &'a [f32])>;

/// The same sum over typed runs already resolved: field reads alone.
#[inline(never)]
fn resolved_sum(runs: &Runs<'_>) -> u64 {
    let mut total = 0u64;
    for (entries, weights) in runs {
        for e in entries.iter() {
            total += u64::from(e.track) + u64::from(e.id) + e.index as u64;
        }
        for x in weights.iter() {
            total += u64::from(x.to_bits());
        }
    }
    total
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_yfd_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(paths) = yfd_files("the .yfd read cost") else {
        return;
    };
    let opened: Vec<Opened> = paths.iter().map(|p| Opened::read(p)).collect();
    let mut plain: Vec<(Vec<PlainEntry>, Vec<f32>)> = Vec::new();
    let mut resolved: Runs<'_> = Vec::new();
    let mut fields = 0usize;
    for o in &opened {
        let dict = View::<Raw<CrFrameFilterDictionary>>::at_offset(o.image(), 0).expect("root");
        let table = dict.entries().expect("entries");
        for i in 0..table.len() {
            let filter = table.follow(i).expect("reads").expect("a filter");
            let w = View::variant::<Raw<CrFrameFilterMultiWeight>>(&filter)
                .expect("reads")
                .expect("weighted");
            let (entries, weights) = (
                w.track_id_indices().expect("entries").as_slice(),
                w.weights().expect("weights").as_slice(),
            );
            fields += entries.len() * 3 + weights.len();
            plain.push((
                entries
                    .iter()
                    .map(|e| PlainEntry {
                        track: e.track,
                        id: e.id,
                        index: e.index,
                    })
                    .collect(),
                weights.to_vec(),
            ));
            resolved.push((entries, weights));
        }
    }
    let expected: u64 = opened.iter().map(|o| typed_sum(o.image())).sum();
    assert_eq!(expected, plain_sum(&plain));
    assert_eq!(expected, resolved_sum(&resolved));

    const ROUNDS: usize = 400;
    let best = |f: &mut dyn FnMut() -> u64| {
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let mut sink = 0u64;
            for _ in 0..ROUNDS {
                sink = sink.wrapping_add(f());
            }
            black_box(sink);
            best = best.min(start.elapsed());
        }
        best.as_secs_f64() * 1e9 / (ROUNDS * fields) as f64
    };
    let plain_ns = best(&mut || plain_sum(black_box(&plain)));
    let resolved_ns = best(&mut || resolved_sum(black_box(&resolved)));
    let typed_ns = best(&mut || opened.iter().map(|o| typed_sum(black_box(o.image()))).sum());
    let tree_ns = best(&mut || {
        opened
            .iter()
            .map(|o| {
                let tree = gates::format(EXT).read(T, o.image()).expect("reads");
                u64::from(matches!(tree, Value::Struct(_)))
            })
            .sum()
    });
    println!(
        "{} files, {fields} fields: plain vectors {plain_ns:.3} ns/field; typed runs already \
         resolved {resolved_ns:.3} ({:.2}x); typed from the root {typed_ns:.3} ({:.2}x); whole \
         tree read {tree_ns:.3} ({:.1}x)",
        paths.len(),
        resolved_ns / plain_ns,
        typed_ns / plain_ns,
        tree_ns / plain_ns
    );
}
