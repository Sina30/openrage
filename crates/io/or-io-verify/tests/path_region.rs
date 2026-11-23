//! `.ynd` through its declarations, over the corpus: typed reads against the
//! tree, the chains a region's runs state about each other, the recorded
//! testimony re-keyed by offset, the canonical fixed point and the alignment
//! census.
//!
//! ⚠ Both editions ship `.ynd` at version 1, so every corpus file is read as
//! Legacy's layout — which is the claim: the version decides, not the install.

mod declared;

use std::collections::BTreeMap;

use declared::{file_tree, files, moved_by, name, Filed, Opened};
use or_io::decl::{Plain, Raw, Record, View};
use or_io::desc::{Image, Pack};
use or_io::gta5::path_region::*;
use or_io::Generation;
use or_io_verify::census;

const EXT: &str = "ynd";
const T: Generation = Generation::Gen8;

type E = or_io::decl::ViewError;

#[derive(Default)]
struct Reached(BTreeMap<(usize, usize), Vec<u8>>);

impl Reached {
    fn put<R: Record>(&mut self, v: &View<'_, R>) {
        let bytes = Plain::as_bytes(View::get(v)).to_vec();
        self.0.insert((View::offset(v), bytes.len()), bytes);
    }
    fn run<R: Record>(&mut self, run: or_io::decl::Run<'_, R>) -> usize {
        let n = run.len();
        for v in run.views() {
            self.put(&v);
        }
        n
    }
}

#[derive(Default)]
struct Counts {
    nodes: usize,
    links: usize,
    junctions: usize,
    refs: usize,
    heightmap: usize,
}

fn typed(image: Image<'_>, out: &mut Reached, n: &mut Counts) -> Result<(), E> {
    let region = View::<Raw<CPathRegion>>::at_offset(image, 0)?;
    out.put(&region);
    if let Some(pages) = region.first_node()? {
        out.put(&pages);
    }
    n.nodes += out.run(region.nodes()?);
    n.links += out.run(region.links()?);
    n.junctions += out.run(region.virtual_junctions()?);
    n.refs += out.run(region.junction_map().junction_map().data()?);
    n.heightmap += region.height_samples()?.len();
    Ok(())
}

#[test]
fn every_ynd_reads_typed_as_the_tree_reads() {
    let Some(paths) = files(EXT, Some(T), "a typed .ynd read against the tree") else {
        return;
    };
    let (mut same, mut n, mut failures) = (0usize, Counts::default(), Vec::new());
    for path in &paths {
        let opened = Opened::read(path);
        let image = opened.image();
        let tree = declared::format(EXT).read(T, image).expect("reads");
        let mut filed = Filed::default();
        file_tree(image, &tree, Some(0), &mut filed);
        let from_tree: BTreeMap<(usize, usize), Vec<u8>> = filed
            .structs
            .into_iter()
            .map(|((at, _), b)| ((at, b.len()), b))
            .collect();
        let mut reached = Reached::default();
        if let Err(e) = typed(image, &mut reached, &mut n) {
            failures.push(format!("{}: {e}", name(path)));
            continue;
        }
        for (key, bytes) in &reached.0 {
            match from_tree.get(key) {
                Some(t) if t == bytes => same += 1,
                Some(_) => failures.push(format!("{}: {key:x?} reads differently", name(path))),
                None => failures.push(format!(
                    "{}: the tree holds nothing at {key:x?}",
                    name(path)
                )),
            }
        }
    }
    println!(
        "{} .ynd: {} nodes, {} links, {} junctions, {} junction refs, {} heightmap bytes; \
         {same} typed instances read as the tree reads them",
        paths.len(),
        n.nodes,
        n.links,
        n.junctions,
        n.refs,
        n.heightmap
    );
    assert!(same > 0 && n.nodes > 0);
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The node split, the link chain, the heightmap chain and the reference
/// order, each counted separately so a model that fits one and not another
/// says which.
#[test]
fn every_region_closes_its_chains() {
    let address = |key: u32| (key as u16, (key >> 16) as u16);
    let Some(paths) = files(EXT, Some(T), "the chains a .ynd states") else {
        return;
    };
    let (mut split, mut links, mut heights, mut sorted, mut z) = (0, 0, 0, 0, (0usize, 0usize));
    let mut failures = Vec::new();
    for path in &paths {
        let opened = Opened::read(path);
        let region = View::<Raw<CPathRegion>>::at_offset(opened.image(), 0).unwrap();
        split += usize::from(
            region.num_nodes_car_nodes + region.num_nodes_ped_nodes == region.num_nodes,
        );
        let nodes = region.nodes().unwrap();
        let mut chain = 0usize;
        let mut ok = true;
        for n in nodes.iter() {
            ok &= n.start_index_of_links as u16 as usize == chain;
            chain += ((n.as_integer2 >> 11) & 0x1f) as usize;
        }
        links += usize::from(ok && chain == region.links().unwrap().len());
        let junctions = region.virtual_junctions().unwrap();
        let mut chain = 0usize;
        let mut ok = true;
        for j in junctions.iter() {
            ok &= usize::from(j.start_index_of_height_samples) == chain;
            chain += usize::from(j.x_samples) * usize::from(j.y_samples);
            z.0 += usize::from(j.max_z as i16 >= j.height_base_world as i16);
            z.1 += 1;
        }
        heights += usize::from(ok && chain == region.height_samples().unwrap().len());
        let refs = region.junction_map().junction_map().data().unwrap();
        sorted += usize::from(
            refs.windows(2)
                .all(|w| address(w[0].key) <= address(w[1].key)),
        );
        if let Err(e) = validate(opened.image(), 0) {
            failures.push(format!("{}: {e}", name(path)));
        }
    }
    let files = paths.len();
    println!(
        "{files} regions: node split {split}, link chain {links}, heightmap chain {heights}, \
         references sorted {sorted}; max_z >= min_z on {} of {} junctions",
        z.0, z.1
    );
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        (split, links, heights, sorted),
        (files, files, files, files)
    );
    assert_eq!(z.0, z.1);
}

#[test]
fn every_ynd_opens_owned_and_saves_as_its_tree_saves() {
    let Some(paths) = files(EXT, Some(T), "the owned .ynd against the tree") else {
        return;
    };
    let got = declared::owned_saves_as_tree::<CPathRegion>(EXT, T, &paths);
    println!(
        ".ynd: {} files, {} opened owned, saved byte-identical to the tree's save and at a \
         fixed point, {} refused by both",
        got.files,
        got.same,
        got.refused.len()
    );
    assert!(got.failures.is_empty(), "{}", got.failures.join("\n"));
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(got.same, paths.len());
}

/// A region's page map keeps one zeroed `page_info` slot per page, so a save
/// lays `8 * (virtual_count + physical_count)` bytes after its sixteen: the
/// file states them, and the owned save writes them from the page counts.
#[test]
fn every_ynd_page_map_keeps_a_zeroed_slot_per_page() {
    use or_io::decl::{File, Resource};
    let Some(paths) = files(EXT, Some(T), "the .ynd page maps") else {
        return;
    };
    let mut slots = BTreeMap::<usize, usize>::new();
    for path in &paths {
        let file = std::fs::read(path).unwrap();
        let saved = CPathRegion::from_bytes(&file)
            .and_then(|r| r.to_bytes())
            .unwrap_or_else(|e| panic!("{}: {e}", name(path)));
        let mut held = Vec::new();
        for bytes in [&file, &saved] {
            let peek = CPathRegion::peek(bytes).unwrap();
            let pages = peek
                .root()
                .unwrap()
                .first_node()
                .unwrap()
                .expect("a page map");
            let info = pages.page_info().unwrap();
            let count = usize::from(pages.virtual_count) + usize::from(pages.physical_count);
            assert_eq!(info.len(), count, "{}", name(path));
            assert!(info.iter().all(|s| *s == 0), "{}", name(path));
            held.push(info.len());
        }
        assert_eq!(held[0], held[1], "{}", name(path));
        *slots.entry(held[0]).or_default() += 1;
    }
    let bytes: usize = slots.iter().map(|(n, files)| 8 * n * files).sum();
    println!(
        "{} .ynd: page_info slots per file {slots:?}; {bytes} bytes of slots saved in all",
        paths.len()
    );
    assert_eq!(slots.values().sum::<usize>(), paths.len());
    assert!(!slots.contains_key(&0));
}

/// A page slot the file fills, which a save would write zero, is refused open.
#[test]
fn a_ynd_page_map_with_a_filled_slot_is_refused_open() {
    let Some(paths) = files(EXT, Some(T), "a filled .ynd page slot refused") else {
        return;
    };
    let opened = Opened::read(&paths[0]);
    let region = View::<Raw<CPathRegion>>::at_offset(opened.image(), 0).unwrap();
    let pages = region.first_node().unwrap().expect("a page map");
    let at = View::offset(&pages) + std::mem::size_of::<Raw<PgBasePageMap>>();
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[at] = 1;
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = declared::format(EXT)
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = <CPathRegion as or_io::decl::Owned>::from_element(&tree)
        .expect_err("a filled slot")
        .to_string();
    println!("{}: {why}", name(&paths[0]));
    assert!(why.contains("a slot is not zero"), "{why}");
}

#[test]
fn every_ynd_saves_to_a_fixed_point() {
    let Some(paths) = files(EXT, Some(T), "the .ynd fixed point") else {
        return;
    };
    let got = declared::fixed_point(EXT, T, &paths, &[]);
    println!(
        ".ynd: {} file(s), {} at a fixed point",
        got.files, got.fixed
    );
    assert!(got.failures.is_empty(), "{}", got.failures.join("\n"));
    assert_eq!(got.fixed, got.files);
}

/// A region whose second node claims to start its links one late.
#[test]
fn a_region_whose_link_chain_is_broken_is_refused() {
    let Some(paths) = files(EXT, Some(T), "a broken .ynd refused") else {
        return;
    };
    let ynd = declared::format(EXT);
    let path = paths
        .iter()
        .find(|p| {
            let o = Opened::read(p);
            let r = View::<Raw<CPathRegion>>::at_offset(o.image(), 0).unwrap();
            r.num_nodes > 1
        })
        .expect("a region with two nodes");
    let opened = Opened::read(path);
    let region = View::<Raw<CPathRegion>>::at_offset(opened.image(), 0).unwrap();
    let second = region.nodes().unwrap().views().nth(1).unwrap();
    let at = View::offset(&second) + 0x1a;
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[at..at + 2].copy_from_slice(&(second.start_index_of_links + 1).to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = ynd
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = ynd
        .save(T, &tree)
        .expect_err("a link chain that does not close")
        .to_string();
    println!("{}: {why}", name(path));
    assert!(why.contains("node 1 starts its links at"), "{why}");
}

#[test]
fn every_ynd_instance_is_aligned_for_its_view() {
    let Some(paths) = files(EXT, Some(T), "the .ynd alignment census") else {
        return;
    };
    let pack = declared::pack(EXT, T);
    let root = declared::format(EXT).root_name(T).unwrap();
    let mut total = census::Census::default();
    for path in &paths {
        total.fold(census::take(&pack, Opened::read(path).image(), &root, 0));
    }
    for (name, tally) in &total.types {
        println!(
            "{name:<22} {:>8} instances, {} misaligned, alignment {}",
            tally.instances, tally.misaligned, tally.alignment
        );
    }
    assert!(total.unreached.is_empty(), "{:?}", total.unreached);
    let misaligned: Vec<_> = total.misaligned().collect();
    assert!(misaligned.is_empty(), "{misaligned:?}");
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_ynd_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(paths) = files(EXT, Some(T), "the .ynd read cost") else {
        return;
    };
    let opened: Vec<Opened> = paths.iter().map(|p| Opened::read(p)).collect();
    let best = |rounds: usize, f: &mut dyn FnMut() -> usize| {
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let mut sink = 0usize;
            for _ in 0..rounds {
                sink = sink.wrapping_add(f());
            }
            black_box(sink);
            best = best.min(start.elapsed());
        }
        best.as_secs_f64() * 1e3 / rounds as f64
    };
    // Every node's position and every link's target, typed from the root.
    #[inline(never)]
    fn typed_sum(image: Image<'_>) -> usize {
        let region = View::<Raw<CPathRegion>>::at_offset(image, 0).unwrap();
        let nodes: usize = region
            .nodes()
            .unwrap()
            .iter()
            .map(|n| {
                (n.coors_x as i32 + n.coors_y as i32 + (n.as_integer1 >> 16) as i16 as i32) as usize
            })
            .fold(0, usize::wrapping_add);
        let links: usize = region
            .links()
            .unwrap()
            .iter()
            .map(|l| usize::from(l.other_node.region) + usize::from(l.other_node.index))
            .sum();
        nodes.wrapping_add(links)
    }
    let typed_ms = best(200, &mut || {
        opened
            .iter()
            .map(|o| typed_sum(black_box(o.image())))
            .fold(0, usize::wrapping_add)
    });
    let tree_ms = best(3, &mut || {
        opened
            .iter()
            .map(|o| {
                usize::from(
                    declared::format(EXT)
                        .read(T, o.image())
                        .unwrap()
                        .record()
                        .is_some(),
                )
            })
            .sum()
    });
    println!(
        "{} files: every node's position and link's target typed from the root {typed_ms:.4} ms; \
         the whole tree read {tree_ms:.2} ms ({:.0}x)",
        paths.len(),
        tree_ms / typed_ms
    );
}

fn builtin(t: Generation) -> Option<Pack> {
    (t == T).then(|| (*declared::pack(EXT, T)).clone())
}

#[test]
fn the_ynd_testimony_holds_through_its_declarations() {
    let Some(rows) = declared::check_against(
        EXT,
        &builtin,
        &[],
        &[],
        "the .ynd testimony through the declarations",
    ) else {
        return;
    };
    let (total, mut failures) = declared::judge(&rows);
    assert!(total > 0, "no recorded field was asked");
    failures.truncate(30);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_declared_field_one_byte_late_is_named_by_the_testimony() {
    let late = moved_by(
        &declared::pack(EXT, T),
        "CPathNode",
        "start_index_of_links",
        1,
    );
    let Some(rows) = declared::caught_against(
        EXT,
        &|t| (t == T).then(|| late.clone()),
        &[],
        &[],
        "a misdeclared .ynd against the testimony",
    ) else {
        return;
    };
    for (label, t) in &rows {
        let caught = t.differ.len() + t.unanswered;
        println!("{label}: {caught} caught with a `CPathNode` first link one byte late");
        assert!(caught > 0, "{label} did not notice");
    }
}
