//! `.ynv` through its declarations, over the corpus: typed reads against the
//! tree, the owned form against the tree's save, the recorded testimony
//! re-keyed by offset, the canonical fixed point and the alignment census;
//! then the split arrays' partitions, a broken index run refused, and where a
//! canonical save lays the packed types.

use or_io::decl::{Described, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::navmesh::*;
use or_io::Generation;
use or_io_verify::gates::{self, files, name, offset_of_run, Filed, Gates, Opened};

const EXT: &str = "ynv";
const T: Generation = Generation::Gen8;

type E = ViewError;

fn put<R: Record + Described>(out: &mut Filed, v: &View<'_, R>) {
    out.structs.insert(
        (View::offset(v), R::name()),
        Plain::as_bytes(View::get(v)).to_vec(),
    );
}

fn run<R: Record + Described>(out: &mut Filed, run: or_io::decl::Run<'_, R>) {
    for v in run.views() {
        put(out, &v);
    }
}

fn bytes<T: Plain>(image: Image<'_>, out: &mut Filed, run: &[T]) {
    if let Some(at) = offset_of_run(image, run) {
        out.runs
            .insert(at, run.iter().flat_map(|v| v.as_bytes().to_vec()).collect());
    }
}

/// Files one sub-array's items.
type Items<'a, T> = dyn Fn(&mut Filed, View<'_, Raw<SubArray<T>>>) -> Result<(), E> + 'a;

/// A split array, its sub-arrays and their items.
fn split<T>(
    image: Image<'_>,
    list: Option<View<'_, Raw<AiSplitArray<T>>>>,
    out: &mut Filed,
    items: &Items<'_, T>,
) -> Result<(), E>
where
    T: or_io::decl::Owned,
    Raw<T>: Plain + or_io::decl::Element + or_io::decl::Param,
{
    let Some(list) = list else {
        return Ok(());
    };
    put(out, &list);
    for c in list.sub_arrays()?.views() {
        put(out, &c);
        items(out, c)?;
    }
    bytes(image, out, &list.start_indices()?);
    Ok(())
}

/// A quadtree node and everything under it. ⚠ The node is packed, so its
/// pointer slots are copied out, not viewed in place.
fn node(image: Image<'_>, n: View<'_, Raw<CNavMeshQuadTree>>, out: &mut Filed) -> Result<(), E> {
    put(out, &n);
    if let Some(leaf) = n.leaf_data()? {
        put(out, &leaf);
        bytes(image, out, &leaf.polys()?);
        run(out, leaf.cover_points()?);
    }
    let children = { View::get(&n).children };
    for slot in children.iter().filter(|p| !p.is_null()) {
        node(image, View::at_address(image, slot.raw())?, out)?;
    }
    Ok(())
}

/// What the typed API reads of a mesh and everything it reaches, in the shape [`gates::file_tree`] files a tree.
fn walk(image: Image<'_>) -> Result<Filed, E> {
    let mut out = Filed::default();
    let mesh = View::<Raw<CNavMesh>>::at_offset(image, 0)?;
    put(&mut out, &mesh);
    if let Some(pages) = mesh.first_node()? {
        put(&mut out, &pages);
        bytes(image, &mut out, &pages.page_info()?);
    }
    split::<CNavMeshCompressedVertex>(
        image,
        mesh.compressed_vertex_array()?,
        &mut out,
        &|o, c| {
            run(o, c.elements()?);
            Ok(())
        },
    )?;
    split::<u16>(image, mesh.vertex_index_array()?, &mut out, &|o, c| {
        bytes(image, o, &c.elements()?);
        Ok(())
    })?;
    split::<TAdjPoly>(image, mesh.adjacent_polys_array()?, &mut out, &|o, c| {
        run(o, c.elements()?);
        Ok(())
    })?;
    split::<TNavMeshPoly>(image, mesh.polys_array()?, &mut out, &|o, c| {
        run(o, c.elements()?);
        Ok(())
    })?;
    if let Some(root) = mesh.quad_tree()? {
        node(image, root, &mut out)?;
    }
    run(&mut out, mesh.special_links()?);
    bytes(image, &mut out, &mesh.special_links_indices()?);
    Ok(out)
}

fn typed(image: Image<'_>) -> Result<Filed, String> {
    walk(image).map_err(|e| e.to_string())
}

const GATES: Gates = Gates {
    census_types: 12,
    ..Gates::new(EXT, T, ("TNavMeshPoly", "as_integer4"))
};

or_io_verify::declared_gates! {
    gates: GATES,
    typed: typed,
    reads_typed: every_ynv_reads_typed_as_the_tree_reads,
    owned: CNavMesh => every_ynv_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_ynv_saves_to_a_fixed_point,
    census: every_ynv_instance_is_aligned_for_its_view,
    testimony: the_ynv_testimony_holds_through_its_declarations,
    one_byte_late: a_ynv_field_one_byte_late_is_named_by_the_testimony,
}

/// The identities a navmesh's runs state about themselves, which is what
/// `validate` refuses a save for.
#[test]
fn every_split_array_states_the_partition_its_stride_implies() {
    let Some(paths) = files(EXT, Some(T), "the .ynv partitions") else {
        return;
    };
    let mut failures = Vec::new();
    for path in &paths {
        if let Err(e) = validate(Opened::read(path).image(), 0) {
            failures.push(format!("{}: {e}", name(path)));
        }
    }
    println!(
        "{} .ynv: every split array's partition, every polygon's corners, every edge and every \
         index hold on {} of them",
        paths.len(),
        paths.len() - failures.len()
    );
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A polygon whose corners start one index late.
#[test]
fn a_polygon_whose_corners_do_not_follow_the_last_is_refused() {
    let Some(paths) = files(EXT, Some(T), "a broken .ynv refused") else {
        return;
    };
    fn polygons(image: Image<'_>) -> Vec<usize> {
        let mesh = View::<Raw<CNavMesh>>::at_offset(image, 0).unwrap();
        mesh.polys_array().unwrap().map_or(Vec::new(), |l| {
            items::<TNavMeshPoly>(&l, "polygon")
                .unwrap()
                .into_iter()
                .map(|p| p as *const _ as usize - image.bytes().as_ptr() as usize)
                .collect()
        })
    }
    let ynv = gates::format(EXT);
    let path = paths
        .iter()
        .find(|p| {
            let o = Opened::read(p);
            polygons(o.image()).len() > 1
        })
        .expect("a mesh with two polygons");
    let opened = Opened::read(path);
    let second = polygons(opened.image())[1];
    let mut bytes = opened.bytes.bytes().to_vec();
    let at = second + std::mem::offset_of!(Raw<TNavMeshPoly>, as_integer2);
    let start = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    bytes[at..at + 2].copy_from_slice(&(start + 1).to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = ynv
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = ynv.save(T, &tree).expect_err("one late").to_string();
    println!("{}: {why}", name(path));
    assert!(why.contains("polygon 1 starts its corners"), "{why}");
}

#[test]
fn every_saved_ynv_opens_its_views() {
    let Some(paths) = files(EXT, Some(T), "the saved .ynv views") else {
        return;
    };
    let fmt = gates::format(EXT);
    for path in &paths {
        let opened = Opened::read(path);
        let bytes = fmt.save(T, &fmt.read(T, opened.image()).unwrap()).unwrap();
        validate(Opened::of(&bytes).image(), 0)
            .unwrap_or_else(|e| panic!("{}: a saved file's views: {e}", name(path)));
    }
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_ynv_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(paths) = files(EXT, Some(T), "the .ynv read cost") else {
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
    // Every polygon's corner count and first corner, typed from the root through the sub-arrays.
    #[inline(never)]
    fn typed_sum(image: Image<'_>) -> usize {
        let mesh = View::<Raw<CNavMesh>>::at_offset(image, 0).unwrap();
        let Some(list) = mesh.polys_array().unwrap() else {
            return 0;
        };
        items::<TNavMeshPoly>(&list, "polygon")
            .unwrap()
            .iter()
            .map(|p| ((p.as_integer1 >> 21) & 0xf) as usize + (p.as_integer2 & 0xffff) as usize)
            .fold(0, usize::wrapping_add)
    }
    let typed_ms = best(20, &mut || {
        opened.iter().map(|o| typed_sum(black_box(o.image()))).sum()
    });
    let tree_ms = best(2, &mut || {
        opened
            .iter()
            .map(|o| {
                usize::from(
                    gates::format(EXT)
                        .read(T, o.image())
                        .unwrap()
                        .record()
                        .is_some(),
                )
            })
            .sum()
    });
    println!(
        "{} files: every polygon's corners typed from the root {typed_ms:.3} ms; the whole tree \
         read {tree_ms:.1} ms ({:.0}x)",
        paths.len(),
        tree_ms / typed_ms
    );
}
