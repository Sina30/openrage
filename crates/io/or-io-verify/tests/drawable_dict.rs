//! `.ydd` through its declarations, both targets, over the corpus: typed
//! reads against the tree, the owned form against the tree's save, the
//! recorded testimony re-keyed by offset, the canonical fixed point and the
//! alignment census. The drawables are the `.ydr` declarations.

mod bound_walk;

use std::mem::{offset_of, size_of};

use or_io::decl::{Described, Gen8, Gen9, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::bounds::PhBound;
use or_io::gta5::drawable::{gen8, gen9, DrawableTarget, GrmGeometry, GtaDrawable, RmcDrawable};
use or_io::gta5::texture_dict::gen9::GrcTextureG9;
use or_io::gta5::{PgDictionary, RmcLod};
use or_io::Generation;
use or_io_verify::gates::{
    self, files, name, offset_of_run, FieldMoved, Filed, Gates, Opened, Overread, Relocated,
};

const EXT: &str = "ydd";

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

/// What each target's typed walk reads of the four types Enhanced changed.
trait Walked: DrawableTarget + Sized {
    fn buffers(g: &View<'_, Raw<GrmGeometry<Self>>>, out: &mut Filed) -> Result<(), E>;
    fn shader(s: &View<'_, Raw<Self::InstanceData>>, out: &mut Filed) -> Result<(), E>;
}

impl Walked for Gen8 {
    fn buffers(g: &View<'_, Raw<GrmGeometry<Gen8>>>, out: &mut Filed) -> Result<(), E> {
        if let Some(vb) = g.vertex_buffer()? {
            put(out, &vb);
            if let Some(fvf) = vb.fvf()? {
                put(out, &fvf);
            }
        }
        if let Some(ib) = g.index_buffer()? {
            put(out, &ib);
        }
        Ok(())
    }

    fn shader(s: &View<'_, Raw<gen8::GrcInstanceData>>, out: &mut Filed) -> Result<(), E> {
        put(out, s);
        Ok(())
    }
}

impl Walked for Gen9 {
    fn buffers(g: &View<'_, Raw<GrmGeometry<Gen9>>>, out: &mut Filed) -> Result<(), E> {
        if let Some(vb) = g.vertex_buffer()? {
            put(out, &vb);
            if let Some(srv) = vb.srv()? {
                put(out, &srv);
            }
            if let Some(info) = vb.info()? {
                put(out, &info);
            }
        }
        if let Some(ib) = g.index_buffer()? {
            put(out, &ib);
            if let Some(srv) = ib.srv()? {
                put(out, &srv);
            }
        }
        Ok(())
    }

    fn shader(s: &View<'_, Raw<gen9::GrcInstanceDataG9>>, out: &mut Filed) -> Result<(), E> {
        put(out, s);
        if let Some(infos) = s.param_infos()? {
            put(out, &infos);
            run(out, infos.params()?);
        }
        Ok(())
    }
}

/// Every non-null level of detail a drawable holds, highest first.
fn lods<'a, G: DrawableTarget>(
    image: Image<'a>,
    d: &View<'a, Raw<RmcDrawable<G>>>,
) -> Result<Vec<View<'a, Raw<RmcLod<G>>>>, E> {
    d.lod_group
        .lods
        .iter()
        .filter(|slot| !slot.is_null())
        .map(|slot| View::at_address(image, slot.raw()))
        .collect()
}

/// Every drawable the dictionary holds, as views.
fn drawables<G: DrawableTarget>(image: Image<'_>) -> Result<Vec<View<'_, Raw<GtaDrawable<G>>>>, E> {
    let dict = View::<Raw<PgDictionary<GtaDrawable<G>>>>::at_offset(image, 0)?;
    let entries = dict.entries()?;
    (0..entries.len())
        .filter_map(|i| entries.follow(i).transpose())
        .collect()
}

fn drawable<G: Walked>(
    image: Image<'_>,
    root: View<'_, Raw<GtaDrawable<G>>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, &root);
    run(out, root.lights()?);
    if let Some(b) = root.bound()? {
        let b = View::<Raw<PhBound>>::at_offset(image, View::offset(&b))?;
        bound_walk::walk(image, b, out)?;
    }
    let d = root.base();
    if let Some(group) = d.shader_group()? {
        put(out, &group);
        if let Some(dict) = group.texture_dictionary()? {
            put(out, &dict);
            let entries = dict.entries()?;
            for i in 0..entries.len() {
                if let Some(t) = entries.follow(i)? {
                    put(out, &t);
                }
            }
        }
        let shaders = group.shaders()?;
        for i in 0..shaders.len() {
            if let Some(s) = shaders.follow(i)? {
                G::shader(&s, out)?;
            }
        }
    }
    if let Some(skeleton) = d.skeleton()? {
        put(out, &skeleton);
        run(out, skeleton.bones()?);
        run(out, skeleton.inverse_transforms()?);
        run(out, skeleton.default_transforms()?);
    }
    if let Some(joints) = d.joint_data()? {
        put(out, &joints);
        run(out, joints.rotation_limits()?);
        run(out, joints.translation_limits()?);
        run(out, joints.scale_limits()?);
    }
    for lod in lods(image, &d)? {
        put(out, &lod);
        let models = lod.models()?;
        for m in 0..models.len() {
            let Some(model) = models.follow(m)? else {
                continue;
            };
            put(out, &model);
            run(out, model.aabbs()?);
            let geometries = model.geometries()?;
            for g in 0..geometries.len() {
                if let Some(geometry) = geometries.follow(g)? {
                    put(out, &geometry);
                    G::buffers(&geometry, out)?;
                }
            }
        }
    }
    Ok(())
}

fn typed<G: Walked>(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: E| e.to_string();
    let mut out = Filed::default();
    let dict = View::<Raw<PgDictionary<GtaDrawable<G>>>>::at_offset(image, 0).map_err(e)?;
    put(&mut out, &dict);
    let codes = dict.codes().map_err(e)?;
    if let Some(at) = offset_of_run(image, &codes) {
        out.runs
            .insert(at, codes.iter().flat_map(|c| c.to_le_bytes()).collect());
    }
    for d in drawables::<G>(image).map_err(e)? {
        drawable::<G>(image, d, &mut out).map_err(e)?;
    }
    Ok(out)
}

/// As the `.ydr` suite: the older record's reader took one box past the end
/// of a single-geometry model's run, out of the geometry or the Enhanced
/// buffer view allocated after it.
const OVERREAD: &[Overread] = &[
    Overread {
        record: "corpus-cda05819",
        inside: "ShaderResourceViewG9",
        delta: 0,
        bytes: 0x20,
        because: "the same box past an Enhanced model's run, read from the buffer view after it",
    },
    Overread {
        record: "corpus-cda05819",
        inside: "ShaderResourceViewG9",
        delta: 0,
        bytes: 0x10,
        because: "the near corner of that box",
    },
    Overread {
        record: "corpus-cda05819",
        inside: "ShaderResourceViewG9",
        delta: 0x10,
        bytes: 0x10,
        because: "the far corner of that box",
    },
    Overread {
        record: "corpus-cda05819",
        inside: "GrmGeometry",
        delta: 0,
        bytes: 0x20,
        because: "a box past a one-geometry model's run, read from the geometry after it",
    },
    Overread {
        record: "corpus-cda05819",
        inside: "GrmGeometry",
        delta: 0,
        bytes: 0x10,
        because: "the near corner of that box",
    },
    Overread {
        record: "corpus-cda05819",
        inside: "GrmGeometry",
        delta: 0x10,
        bytes: 0x10,
        because: "the far corner of that box",
    },
];

/// An Enhanced texture's view, as the `.ytd` suite records it.
const MOVED: &[FieldMoved] = &[FieldMoved {
    target: "gen9",
    inside: "GrcTextureG9",
    now: offset_of!(Raw<GrcTextureG9>, view),
    was: 0x50,
    len: size_of::<Raw<or_io::gta5::gen9::ShaderResourceViewG9>>(),
}];

const RELOCATED: &[Relocated] = &[Relocated {
    record: "corpus-cda05819",
    inside: "GrcTextureG9",
    delta: 0x50,
    because: "the view is embedded at 0x58; the recorder took 0x50 from the description of the day",
}];

const LEGACY: Gates = Gates {
    census_types: 27,
    relocated: RELOCATED,
    overread: OVERREAD,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen8, ("GrmGeometry", "indices_count"))
};

const ENHANCED: Gates = Gates {
    census_types: 22,
    relocated: RELOCATED,
    overread: OVERREAD,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen9, ("GrmGeometry", "indices_count"))
};

or_io_verify::declared_gates! {
    gates: LEGACY,
    typed: typed::<Gen8>,
    reads_typed: every_ydd_reads_typed_as_the_tree_reads,
    owned: PgDictionary<GtaDrawable<Gen8>> => every_legacy_ydd_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_ydd_saves_to_a_fixed_point,
    census: every_ydd_instance_is_aligned_for_its_view,
    testimony: the_ydd_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_field_one_byte_late_is_named_by_the_testimony,
}

or_io_verify::declared_gates! {
    gates: ENHANCED,
    typed: typed::<Gen9>,
    reads_typed: every_enhanced_ydd_reads_typed_as_the_tree_reads,
    owned: PgDictionary<GtaDrawable<Gen9>> => every_enhanced_ydd_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_enhanced_ydd_saves_to_a_fixed_point,
    census: every_enhanced_ydd_instance_is_aligned_for_its_view,
    testimony: the_enhanced_ydd_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_enhanced_field_one_byte_late_is_named_by_the_testimony,
}

/// A dictionary stating one code fewer than it holds drawables.
#[test]
fn a_drawable_dictionary_whose_hashes_disagree_with_its_drawables_is_refused() {
    let Some(paths) = files(EXT, Some(Generation::Gen8), "a broken .ydd refused") else {
        return;
    };
    let ydd = gates::format(EXT);
    let opened = Opened::read(&paths[0]);
    let held = drawables::<Gen8>(opened.image()).unwrap().len();
    // The count follows the run's pointer.
    let count = offset_of!(Raw<PgDictionary<GtaDrawable>>, codes) + size_of::<u64>();
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[count..count + 2].copy_from_slice(&((held - 1) as u16).to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = ydd
        .read(
            Generation::Gen8,
            Image::new(aligned.bytes(), opened.virtual_size),
        )
        .expect("reads");
    let why = ydd
        .save(Generation::Gen8, &tree)
        .expect_err("one hash short")
        .to_string();
    println!("{}: {why}", name(&paths[0]));
    assert!(
        why.contains(&format!("holds {} hashes and {held} drawables", held - 1)),
        "{why}"
    );
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_ydd_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    // Every geometry's counts, typed from the root.
    #[inline(never)]
    fn typed_sum<G: DrawableTarget>(image: Image<'_>) -> usize {
        let mut total = 0usize;
        for d in drawables::<G>(image).unwrap() {
            for lod in lods(image, &d.base()).unwrap() {
                let models = lod.models().unwrap();
                for model in (0..models.len()).filter_map(|i| models.follow(i).ok().flatten()) {
                    let run = model.geometries().unwrap();
                    for g in (0..run.len()).filter_map(|i| run.follow(i).ok().flatten()) {
                        total += g.indices_count as usize + usize::from(g.vertices_count);
                    }
                }
            }
        }
        total
    }
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
    for t in [Generation::Gen8, Generation::Gen9] {
        let Some(paths) = files(EXT, Some(t), "the .ydd read cost") else {
            continue;
        };
        let opened: Vec<Opened> = paths.iter().map(|p| Opened::read(p)).collect();
        let sum = if t == Generation::Gen9 {
            typed_sum::<Gen9>
        } else {
            typed_sum::<Gen8>
        };
        let typed_ms = best(2_000, &mut || {
            opened.iter().map(|o| sum(black_box(o.image()))).sum()
        });
        let tree_ms = best(5, &mut || {
            opened
                .iter()
                .map(|o| {
                    usize::from(
                        gates::format(EXT)
                            .read(t, o.image())
                            .unwrap()
                            .record()
                            .is_some(),
                    )
                })
                .sum()
        });
        println!(
            "{} {} files: every geometry's counts typed from the root {typed_ms:.4} ms; the whole \
             tree read {tree_ms:.2} ms ({:.0}x)",
            paths.len(),
            t.edition_label(),
            tree_ms / typed_ms
        );
    }
}

/// Mod `.ydd` files staged in the corpus's `mods/`: two whose geometries state
/// no primitives and no vertices beside the runs they draw, and one whose counts agree.
const INCOHERENT: [&str; 2] = ["Naruto4tails.ydd", "susanoo_2.ydd"];
const COHERENT: &str = "Madara_Susanoo_big.ydd";

/// The staged mod file named `file`, opened owned; `None`, recorded as a skip, where it is not staged.
fn staged_mod(file: &str) -> Option<(Vec<u8>, PgDictionary<GtaDrawable<Gen8>>)> {
    use or_io::decl::File as _;
    let what = format!("the staged mod {file}");
    let mods = or_test_support::mods_corpus_origin(env!("CARGO_MANIFEST_DIR"));
    let path = (or_test_support::Corpus::with_root(mods.path(), EXT).all_targets())
        .find()
        .into_iter()
        .find(|p| name(p) == file);
    let Some(path) = path else {
        or_test_support::skip_corpus(&what, format!("not staged in {}", mods.describe()));
        return None;
    };
    let bytes = std::fs::read(&path).expect("a corpus file reads");
    let owned = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes).expect("it opens");
    Some((bytes, owned))
}

#[test]
fn a_mod_ydd_whose_counts_contradict_its_runs_is_refused_by_name_without_consent() {
    use or_io::decl::File as _;
    for file in INCOHERENT {
        let Some((_, owned)) = staged_mod(file) else {
            continue;
        };
        let why = owned
            .to_bytes()
            .expect_err("a save that would rewrite a stated count")
            .to_string();
        println!("{file}: {why}");
        assert!(
            why.contains("level0/model0/geometry0") && why.contains("primitive_count 0"),
            "{why}"
        );
    }
}

/// Every geometry of every drawable, with where it is.
fn geometries(dict: &PgDictionary<GtaDrawable<Gen8>>) -> Vec<(String, &GrmGeometry<Gen8>)> {
    let mut out = Vec::new();
    for (d, drawable) in dict.entries.iter().enumerate() {
        let Some(drawable) = drawable.as_deref() else {
            continue;
        };
        for (l, level) in drawable.lod_group.lods.iter().enumerate() {
            for (m, model) in level.models.iter().enumerate() {
                for (g, geometry) in model.geometries.iter().enumerate() {
                    out.push((
                        format!("drawable{d}/level{l}/model{m}/geometry{g}"),
                        &**geometry,
                    ));
                }
            }
        }
    }
    out
}

#[test]
fn asked_to_repair_a_mod_ydd_rewrites_its_counts_from_its_runs_and_says_so() {
    use or_io::decl::{File as _, Repair};
    use or_io::gta5::drawable::each_defect;
    for file in INCOHERENT {
        let Some((_, owned)) = staged_mod(file) else {
            continue;
        };
        let (bytes, repairs) = owned
            .to_bytes_repairing(Repair::Lossless)
            .expect("every rewrite is lossless");
        println!(
            "{file}: {} counts rewritten, first {:?}",
            repairs.len(),
            repairs.first()
        );
        assert!(
            repairs
                .iter()
                .any(|r| r.contains("primitive_count 0 rewritten as")),
            "{repairs:?}"
        );
        assert!(
            repairs
                .iter()
                .any(|r| r.contains("vertices_count 0 rewritten as")),
            "{repairs:?}"
        );

        let reopened = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes).expect("reopens");
        let (was, now) = (geometries(&owned), geometries(&reopened));
        assert_eq!(was.len(), now.len());
        for ((at, a), (_, b)) in was.iter().zip(&now) {
            assert_eq!(
                a.index_buffer.indices, b.index_buffer.indices,
                "{at}: no index dropped"
            );
            assert_eq!(
                a.vertex_buffer.data, b.vertex_buffer.data,
                "{at}: no vertex byte dropped"
            );
        }
        let saved = Opened::of(&bytes);
        let mut defects = Vec::new();
        for d in drawables::<Gen8>(saved.image()).expect("the repair reads") {
            each_defect(&d.base(), &mut |f| defects.push(f.to_string())).expect("walks");
        }
        assert!(defects.is_empty(), "{file}: {defects:?}");
        assert_eq!(
            reopened.to_bytes().expect("a repaired file saves unasked"),
            bytes,
            "{file}: the repair is a fixed point"
        );
    }
}

#[test]
fn a_mod_ydd_whose_counts_agree_saves_the_same_asked_or_not() {
    use or_io::decl::{File as _, Repair};
    let Some((_, owned)) = staged_mod(COHERENT) else {
        return;
    };
    let unasked = owned.to_bytes().expect("a coherent file saves unasked");
    let (asked, repairs) = owned
        .to_bytes_repairing(Repair::Lossless)
        .expect("a coherent file saves asked");
    assert!(repairs.is_empty(), "{repairs:?}");
    assert_eq!(asked, unasked);
}
