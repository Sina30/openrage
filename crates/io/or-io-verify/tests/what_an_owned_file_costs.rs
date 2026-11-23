//! What opening a file owned costs against reading its tree, what saving an
//! owned value costs against saving the tree, and what a peek costs.

use std::hint::black_box;
use std::time::{Duration, Instant};

use or_io::decl::{Raw, Record, Resource, View};
use or_io::gta5::shared::PgDictionary;
use or_io::gta5::texture_dict::gen8;
use or_io::gta5::CrFrameFilterDictionary;
use or_io::Generation;
use or_io_verify::gates;

fn best(rounds: usize, f: &mut dyn FnMut() -> usize) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        let mut sink = 0usize;
        for _ in 0..rounds {
            sink = sink.wrapping_add(f());
        }
        black_box(sink);
        best = best.min(start.elapsed() / rounds as u32);
    }
    best
}

fn costs<R>(ext: &str, target: Generation, rounds: usize, peek_walk: fn(&[u8]) -> usize)
where
    R: Resource,
    Raw<R>: Record,
{
    let Some(paths) = gates::files(ext, Some(target), "what an owned file costs") else {
        return;
    };
    let fmt = gates::format(ext);
    let files: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let owned: Vec<R> = files.iter().filter_map(|f| R::from_bytes(f).ok()).collect();
    let files: Vec<Vec<u8>> = files
        .into_iter()
        .filter(|f| R::from_bytes(f).is_ok())
        .collect();
    let opened: Vec<gates::Opened> = files.iter().map(|f| gates::Opened::of(f)).collect();
    let trees: Vec<_> = opened
        .iter()
        .map(|o| fmt.read(target, o.image()).unwrap())
        .collect();

    let tree_read = best(rounds, &mut || {
        files
            .iter()
            .map(|f| {
                let o = gates::Opened::of(f);
                usize::from(fmt.read(target, o.image()).is_ok())
            })
            .sum()
    });
    let owned_open = best(rounds, &mut || {
        files
            .iter()
            .map(|f| usize::from(R::from_bytes(f).is_ok()))
            .sum()
    });
    let tree_save = best(rounds, &mut || {
        trees
            .iter()
            .map(|t| fmt.save(target, t).map_or(0, |b| b.len()))
            .sum()
    });
    let owned_save = best(rounds, &mut || {
        owned
            .iter()
            .map(|o| o.to_bytes().map_or(0, |b| b.len()))
            .sum()
    });
    let peek = best(rounds, &mut || files.iter().map(|f| peek_walk(f)).sum());
    println!(
        "{ext} {}: {} files; per round: tree read {tree_read:?}, owned open {owned_open:?} \
         ({:.2}x); tree save {tree_save:?}, owned save {owned_save:?} ({:.2}x); peek open + walk {peek:?} \
         ({:.2}x of the tree read)",
        target.slug(),
        files.len(),
        owned_open.as_secs_f64() / tree_read.as_secs_f64(),
        owned_save.as_secs_f64() / tree_save.as_secs_f64(),
        peek.as_secs_f64() / tree_read.as_secs_f64(),
    );
}

fn yfd_peek(file: &[u8]) -> usize {
    let peek = CrFrameFilterDictionary::peek(file).unwrap();
    let root = peek.root().unwrap();
    let entries = root.entries().unwrap();
    let mut n = 0;
    for i in 0..entries.len() {
        if let Ok(Some(f)) = entries.follow(i) {
            if let Ok(Some(w)) = View::variant::<Raw<or_io::gta5::CrFrameFilterMultiWeight>>(&f) {
                n += w.track_id_indices().map_or(0, |t| t.len());
            }
        }
    }
    n
}

fn ytd_peek(file: &[u8]) -> usize {
    let peek = PgDictionary::<gen8::GrcTexture>::peek(file).unwrap();
    let root = peek.root().unwrap();
    let entries = root.entries().unwrap();
    let mut n = 0;
    for i in 0..entries.len() {
        if let Ok(Some(t)) = entries.follow(i) {
            n += usize::from(t.width);
        }
    }
    n
}

fn yed_peek(file: &[u8]) -> usize {
    let peek = or_io::gta5::CrExpressionsDictionary::peek(file).unwrap();
    let root = peek.root().unwrap();
    let entries = root.entries().unwrap();
    let mut n = 0;
    for i in 0..entries.len() {
        if let Ok(Some(e)) = entries.follow(i) {
            n += e.tracks().map_or(0, |t| t.len());
        }
    }
    n
}

fn ynd_peek(file: &[u8]) -> usize {
    let peek = or_io::gta5::CPathRegion::peek(file).unwrap();
    let root = peek.root().unwrap();
    root.nodes().map_or(0, |n| n.len()) + root.links().map_or(0, |l| l.len())
}

fn ybn_peek(file: &[u8]) -> usize {
    use or_io::gta5::bounds::{PhBoundAny, PhBoundBVH, PhBoundComposite, PhBoundGeometry};
    let peek = PhBoundAny::peek(file).unwrap();
    let mut stack = vec![peek.root().unwrap()];
    let mut n = 0;
    while let Some(b) = stack.pop() {
        if let Ok(Some(g)) = View::variant::<Raw<PhBoundGeometry>>(&b) {
            n += g.polygons().map_or(0, |p| p.len());
        } else if let Ok(Some(v)) = View::variant::<Raw<PhBoundBVH>>(&b) {
            n += v.base().polygons().map_or(0, |p| p.len());
        } else if let Ok(Some(c)) = View::variant::<Raw<PhBoundComposite>>(&b) {
            if let Ok(children) = c.bounds() {
                stack.extend((0..children.len()).filter_map(|i| children.follow(i).ok().flatten()));
            }
        }
    }
    n
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_frame_filter_dict_costs() {
    costs::<CrFrameFilterDictionary>("yfd", Generation::Gen8, 20, yfd_peek);
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_texture_dict_costs() {
    costs::<PgDictionary<gen8::GrcTexture>>("ytd", Generation::Gen8, 3, ytd_peek);
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_expression_dict_costs() {
    costs::<or_io::gta5::CrExpressionsDictionary>("yed", Generation::Gen8, 3, yed_peek);
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_path_region_costs() {
    costs::<or_io::gta5::CPathRegion>("ynd", Generation::Gen8, 3, ynd_peek);
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_bounds_costs() {
    costs::<or_io::gta5::bounds::PhBoundAny>("ybn", Generation::Gen8, 3, ybn_peek);
}

fn yld_peek(file: &[u8]) -> usize {
    use or_io::gta5::cloth_dict::CharacterCloth;
    let peek = PgDictionary::<CharacterCloth>::peek(file).unwrap();
    let dict = peek.root().unwrap();
    let Ok(entries) = dict.entries() else {
        return 0;
    };
    let mut n = 0;
    for cloth in (0..entries.len()).filter_map(|i| entries.follow(i).ok().flatten()) {
        let Ok(Some(c)) = cloth.cloth_controller() else {
            continue;
        };
        let c = c.base();
        for slot in [c.cloth_0(), c.cloth_1(), c.cloth_2(), c.cloth_3()] {
            if let Ok(Some(level)) = slot {
                n += level.edge_data().map_or(0, |e| e.len());
            }
        }
    }
    n
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_cloth_dict_costs() {
    costs::<PgDictionary<or_io::gta5::cloth_dict::CharacterCloth>>(
        "yld",
        Generation::Gen8,
        3,
        yld_peek,
    );
}

fn ydr_peek(file: &[u8]) -> usize {
    use or_io::gta5::GtaDrawable;
    let peek = GtaDrawable::<or_io::decl::Gen8>::peek(file).unwrap();
    let image = peek.image();
    let root = peek.root().unwrap();
    let mut n = 0;
    for slot in root.base().lod_group.lods.iter().filter(|s| !s.is_null()) {
        let Ok(lod) =
            View::<Raw<or_io::gta5::RmcLod<or_io::decl::Gen8>>>::at_address(image, slot.raw())
        else {
            continue;
        };
        let Ok(models) = lod.models() else {
            continue;
        };
        for model in (0..models.len()).filter_map(|i| models.follow(i).ok().flatten()) {
            n += model.geometries().map_or(0, |g| g.len());
        }
    }
    n
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_drawable_costs() {
    costs::<or_io::gta5::GtaDrawable>("ydr", Generation::Gen8, 3, ydr_peek);
}

fn yft_peek(file: &[u8]) -> usize {
    use or_io::gta5::GtaFragType;
    let peek = GtaFragType::<or_io::decl::Gen8>::peek(file).unwrap();
    let root = peek.root().unwrap();
    let Ok(Some(group)) = root.base().physics_lod_group() else {
        return 0;
    };
    [group.high_lod(), group.medium_lod(), group.low_lod()]
        .into_iter()
        .filter_map(|l| l.ok().flatten())
        .map(|l| l.children().map_or(0, |c| c.len()))
        .sum()
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_fragment_costs() {
    costs::<or_io::gta5::GtaFragType>("yft", Generation::Gen8, 3, yft_peek);
}

fn ypt_peek(file: &[u8]) -> usize {
    use or_io::gta5::PtxFxList;
    let peek = PtxFxList::<or_io::decl::Gen8>::peek(file).unwrap();
    let root = peek.root().unwrap();
    let Ok(Some(dict)) = root.particle_rule_dict() else {
        return 0;
    };
    let Ok(rules) = dict.entries() else {
        return 0;
    };
    (0..rules.len())
        .filter_map(|i| rules.follow(i).ok().flatten())
        .map(|r| r.all_behaviours().map_or(0, |b| b.len()))
        .sum()
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_owned_particle_costs() {
    costs::<or_io::gta5::PtxFxList>("ypt", Generation::Gen8, 3, ypt_peek);
}
