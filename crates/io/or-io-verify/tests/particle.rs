//! `.ypt` through its declarations, both targets, over the corpus: typed
//! reads against the tree, the owned form against the tree's save, the
//! recorded testimony re-keyed by offset, the canonical fixed point and the
//! alignment census; then the pointers a save must keep. The drawables are
//! the `.ydr` declarations.

use std::mem::{offset_of, size_of};

use or_io::decl::{Described, File, Gen8, Gen9, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::drawable::{gen8, gen9, DrawableTarget, GrmGeometry, RmcDrawable};
use or_io::gta5::particle::*;
use or_io::gta5::texture_dict::gen9::GrcTextureG9;
use or_io::gta5::{PgDictionary, RmcLod};
use or_io::Generation;
use or_io_verify::gates::{self, files, name, FieldMoved, Filed, Gates, Opened, Overread};

const EXT: &str = "ypt";

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

/// What each target's typed walk reads of the types Enhanced changed.
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

fn drawable<'a, G: Walked>(
    image: Image<'a>,
    d: &View<'a, Raw<RmcDrawable<G>>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, d);
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
    for lod in lods(image, d)? {
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

/// A keyframe's keys, from where the keyframe starts: a packed record has no view of what it embeds.
fn keys(image: Image<'_>, at: usize, out: &mut Filed) -> Result<(), E> {
    run(
        out,
        View::<Raw<PtxKeyframe>>::at_offset(image, at)?.entries()?,
    );
    Ok(())
}

/// The keys of every keyframed value a record lists as pointers at its own members.
fn props(
    image: Image<'_>,
    list: or_io::decl::Run<'_, or_io::decl::MemberPtr<Raw<PtxKeyframeProp>>>,
    out: &mut Filed,
) -> Result<(), E> {
    for slot in list.as_slice() {
        if slot.is_null() {
            continue;
        }
        let at = image
            .resolve(slot.raw())
            .map_err(|e| ViewError::BadPointer {
                ty: "PtxKeyframeProp".into(),
                field: "keyframe_prop_list",
                address: slot.raw(),
                reason: e.to_string().into(),
            })?;
        keys(image, at + offset_of!(Raw<PtxKeyframeProp>, keyframe), out)?;
    }
    Ok(())
}

fn evolutions(
    image: Image<'_>,
    list: &View<'_, Raw<PtxEvolutionList>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, list);
    run(out, list.evolutions()?);
    for p in list.evolved_keyframe_props()?.views() {
        put(out, &p);
        for k in p.evolved_keyframes()?.views() {
            put(out, &k);
            keys(image, View::offset(&k), out)?;
        }
    }
    run(out, list.evolved_keyframe_prop_map().data()?);
    Ok(())
}

macro_rules! each_variant {
    ($b:expr, $out:expr, $($v:ty),*) => {{
        let mut found = false;
        $(
            if !found {
                if let Some(v) = View::variant::<Raw<$v>>($b)? {
                    put($out, &v);
                    found = true;
                }
            }
        )*
        found
    }};
}

fn behaviour(image: Image<'_>, b: &View<'_, Raw<PtxBehaviour>>, out: &mut Filed) -> Result<(), E> {
    let found = each_variant!(
        b,
        out,
        PtxuAge,
        PtxuAcceleration,
        PtxuVelocity,
        PtxuRotation,
        PtxuSize,
        PtxuDampening,
        PtxuMatrixWeight,
        PtxuCollision,
        PtxuAnimateTexture,
        PtxuColour,
        PtxdSprite,
        PtxuWind,
        PtxuLight,
        PtxdModel,
        PtxuDecal,
        PtxuZCull,
        PtxuNoise,
        PtxuAttractor,
        PtxdTrail,
        PtxuFogVolume,
        PtxuRiver,
        PtxuDecalPool,
        PtxuLiquid,
        PtxuCameraEvolution
    );
    if !found {
        put(out, b);
    }
    props(image, b.keyframe_prop_list()?, out)
}

fn particle_rule<G: Walked>(
    image: Image<'_>,
    rule: &View<'_, Raw<PtxParticleRule<G>>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, rule);
    for spawner in [
        rule.effect_spawner_at_ratio(),
        rule.effect_spawner_on_coln(),
    ] {
        if let Some(effect) = spawner.effect_rule()? {
            effect_rule(image, &effect, out)?;
        }
    }
    for list in [
        rule.all_behaviours()?,
        rule.init_behaviours()?,
        rule.update_behaviours()?,
        rule.update_finalize_behaviours()?,
        rule.draw_behaviours()?,
        rule.release_behaviours()?,
    ] {
        for i in 0..list.len() {
            if let Some(b) = list.follow(i)? {
                behaviour(image, &b, out)?;
            }
        }
    }
    run(out, rule.bias_links()?);
    let vars = rule.shader_inst().inst_vars()?;
    for i in 0..vars.len() {
        let Some(v) = vars.follow(i)? else {
            continue;
        };
        if let Some(x) = View::variant::<Raw<PtxShaderVarVector<G>>>(&v)? {
            put(out, &x);
        } else if let Some(x) = View::variant::<Raw<PtxShaderVarTexture<G>>>(&v)? {
            put(out, &x);
        } else if let Some(x) = View::variant::<Raw<PtxShaderVarKeyframe<G>>>(&v)? {
            put(out, &x);
            keys(
                image,
                View::offset(&x) + offset_of!(Raw<PtxShaderVarKeyframe<G>>, keyframe),
                out,
            )?;
        } else {
            put(out, &v);
        }
    }
    run(out, rule.drawables()?);
    Ok(())
}

fn emitter_rule(
    image: Image<'_>,
    rule: &View<'_, Raw<PtxEmitterRule>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, rule);
    for slot in [
        rule.creation_domain_obj(),
        rule.target_domain_obj(),
        rule.attractor_domain_obj(),
    ] {
        if let Some(d) = slot.domain()? {
            put(out, &d);
            props(image, d.keyframe_prop_list()?, out)?;
        }
    }
    props(image, rule.keyframe_prop_list()?, out)
}

fn effect_rule<G: Walked>(
    image: Image<'_>,
    rule: &View<'_, Raw<PtxEffectRule<G>>>,
    out: &mut Filed,
) -> Result<(), E> {
    if out
        .structs
        .contains_key(&(View::offset(rule), <Raw<PtxEffectRule<G>>>::name()))
    {
        return Ok(());
    }
    put(out, rule);
    props(image, rule.keyframe_prop_list()?, out)?;
    if let Some(list) = rule.evolution_list()? {
        evolutions(image, &list, out)?;
    }
    let events = rule.timeline().events()?;
    for i in 0..events.len() {
        let Some(e) = events.follow(i)? else {
            continue;
        };
        put(out, &e);
        if let Some(list) = e.base().evolution_list()? {
            evolutions(image, &list, out)?;
        }
        if let Some(emitter) = e.emitter_rule()? {
            emitter_rule(image, &emitter, out)?;
        }
        if let Some(particle) = e.particle_rule()? {
            if !out
                .structs
                .contains_key(&(View::offset(&particle), <Raw<PtxParticleRule<G>>>::name()))
            {
                particle_rule(image, &particle, out)?;
            }
        }
    }
    Ok(())
}

/// Each entry a dictionary holds, the dictionary filed first.
macro_rules! entries {
    ($dict:expr, $out:expr) => {{
        let mut got = Vec::new();
        if let Some(dict) = $dict {
            put($out, &dict);
            let run = dict.entries()?;
            for i in 0..run.len() {
                if let Some(e) = run.follow(i)? {
                    got.push(e);
                }
            }
        }
        got
    }};
}

fn typed<G: Walked>(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: E| e.to_string();
    let mut out = Filed::default();
    let list = View::<Raw<PtxFxList<G>>>::at_offset(image, 0).map_err(e)?;
    put(&mut out, &list);
    let walk = |out: &mut Filed| -> Result<(), E> {
        for t in entries!(list.texture_dict()?, out) {
            put(out, &t);
        }
        for d in entries!(list.model_dict()?, out) {
            drawable(image, &d, out)?;
        }
        for rule in entries!(list.particle_rule_dict()?, out) {
            particle_rule(image, &rule, out)?;
        }
        for rule in entries!(list.effect_rule_dict()?, out) {
            effect_rule(image, &rule, out)?;
        }
        for rule in entries!(list.emitter_rule_dict()?, out) {
            emitter_rule(image, &rule, out)?;
        }
        Ok(())
    };
    walk(&mut out).map_err(e)?;
    Ok(out)
}

/// As the `.ydr` suite: the older record's box overread, here only by the
/// drawables a particle list holds.
const OVERREAD: &[Overread] = &[
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

const LEGACY: Gates = Gates {
    census_types: 78,
    overread: OVERREAD,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen8, ("PtxParticleRule", "file_version"))
};

const ENHANCED: Gates = Gates {
    census_types: 79,
    overread: OVERREAD,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen9, ("PtxParticleRule", "file_version"))
};

or_io_verify::declared_gates! {
    gates: LEGACY,
    typed: typed::<Gen8>,
    reads_typed: every_ypt_reads_typed_as_the_tree_reads,
    owned: PtxFxList<Gen8> => every_legacy_ypt_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_ypt_saves_to_a_fixed_point,
    census: every_ypt_instance_is_aligned_for_its_view,
    testimony: the_ypt_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_field_one_byte_late_is_named_by_the_testimony,
}

or_io_verify::declared_gates! {
    gates: ENHANCED,
    typed: typed::<Gen9>,
    reads_typed: every_enhanced_ypt_reads_typed_as_the_tree_reads,
    owned: PtxFxList<Gen9> => every_enhanced_ypt_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_enhanced_ypt_saves_to_a_fixed_point,
    census: every_enhanced_ypt_instance_is_aligned_for_its_view,
    testimony: the_enhanced_ypt_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_enhanced_field_one_byte_late_is_named_by_the_testimony,
}

/// How many texture variables' textures and effect spawners' rules point at an address in the file.
fn live_pointers<G: DrawableTarget>(image: Image<'_>) -> (usize, usize) {
    let live = |at: usize| {
        image
            .pointer_at(at)
            .is_ok_and(|w| w != 0 && image.resolve(w).is_ok())
    };
    let list = View::<Raw<PtxFxList<G>>>::at_offset(image, 0).unwrap();
    let Some(dict) = list.particle_rule_dict().unwrap() else {
        return (0, 0);
    };
    let rules = dict.entries().unwrap();
    let (mut textures, mut spawners) = (0, 0);
    for rule in (0..rules.len()).filter_map(|i| rules.follow(i).unwrap()) {
        for spawner in [
            offset_of!(Raw<PtxParticleRule<G>>, effect_spawner_at_ratio),
            offset_of!(Raw<PtxParticleRule<G>>, effect_spawner_on_coln),
        ] {
            let at =
                View::offset(&rule) + spawner + offset_of!(Raw<PtxEffectSpawner<G>>, effect_rule);
            spawners += usize::from(live(at));
        }
        let vars = rule.shader_inst().inst_vars().unwrap();
        for v in (0..vars.len()).filter_map(|i| vars.follow(i).unwrap()) {
            if let Some(x) = View::variant::<Raw<PtxShaderVarTexture<G>>>(&v).unwrap() {
                let at = View::offset(&x) + offset_of!(Raw<PtxShaderVarTexture<G>>, texture);
                textures += usize::from(live(at));
            }
        }
    }
    (textures, spawners)
}

/// A texture variable's texture and a spawner's effect rule are pointers a
/// save must keep, not addresses carried as numbers.
#[test]
fn a_saved_ypt_keeps_its_texture_and_spawner_pointers() {
    fn check<G: DrawableTarget>(file: &[u8]) -> ((usize, usize), (usize, usize))
    where
        PtxFxList<G>: File,
    {
        let saved = PtxFxList::<G>::from_bytes(file)
            .unwrap()
            .to_bytes()
            .unwrap();
        (
            live_pointers::<G>(Opened::of(file).image()),
            live_pointers::<G>(Opened::of(&saved).image()),
        )
    }
    for t in [Generation::Gen8, Generation::Gen9] {
        let Some(paths) = files(EXT, Some(t), "a saved .ypt's pointers") else {
            continue;
        };
        let (mut before, mut after) = ((0, 0), (0, 0));
        for path in &paths {
            let file = std::fs::read(path).unwrap();
            let (was, now) = if t == Generation::Gen9 {
                check::<Gen9>(&file)
            } else {
                check::<Gen8>(&file)
            };
            before = (before.0 + was.0, before.1 + was.1);
            after = (after.0 + now.0, after.1 + now.1);
        }
        println!(
            "{} .ypt: {} texture variables point at a texture and {} spawners at an effect rule; \
             {} and {} after an owned save",
            t.edition_label(),
            before.0,
            before.1,
            after.0,
            after.1
        );
        assert!(before.0 > 0 && before.1 > 0);
        assert_eq!(after, before);
    }
}

/// A particle drawable points at an entry of the list's drawable dictionary,
/// and a save must keep it that entry rather than write a copy.
#[test]
fn a_saved_particle_drawable_still_names_its_dictionary_entry() {
    fn check<G: DrawableTarget>(image: Image<'_>) -> (usize, usize) {
        let list = View::<Raw<PtxFxList<G>>>::at_offset(image, 0).unwrap();
        let entries: std::collections::BTreeSet<usize> = list
            .model_dict()
            .unwrap()
            .map(|d| {
                let run = d.entries().unwrap();
                (0..run.len())
                    .filter_map(|i| run.follow(i).unwrap())
                    .map(|v| View::offset(&v))
                    .collect()
            })
            .unwrap_or_default();
        let (mut pointing, mut named) = (0, 0);
        let Some(rules) = list.particle_rule_dict().unwrap() else {
            return (0, 0);
        };
        let run = rules.entries().unwrap();
        for rule in (0..run.len()).filter_map(|i| run.follow(i).unwrap()) {
            for d in rule.drawables().unwrap().views() {
                if let Some(drawable) = d.drawable().unwrap() {
                    pointing += 1;
                    named += usize::from(entries.contains(&View::offset(&drawable)));
                }
            }
        }
        (pointing, named)
    }
    for t in [Generation::Gen8, Generation::Gen9] {
        let Some(paths) = files(EXT, Some(t), "a saved .ypt's particle drawables") else {
            continue;
        };
        let (mut before, mut after) = ((0, 0), (0, 0));
        for path in &paths {
            let opened = Opened::read(path);
            let fmt = gates::format(EXT);
            let saved = Opened::of(&fmt.save(t, &fmt.read(t, opened.image()).unwrap()).unwrap());
            let sum = |a: (usize, usize), b: (usize, usize)| (a.0 + b.0, a.1 + b.1);
            let (was, now) = if t == Generation::Gen9 {
                (check::<Gen9>(opened.image()), check::<Gen9>(saved.image()))
            } else {
                (check::<Gen8>(opened.image()), check::<Gen8>(saved.image()))
            };
            before = sum(before, was);
            after = sum(after, now);
        }
        println!(
            "{} .ypt: {} of {} particle drawables name a dictionary entry, {} of {} after a save",
            t.edition_label(),
            before.1,
            before.0,
            after.1,
            after.0
        );
        assert!(before.0 > 0);
        assert_eq!(after, before);
    }
}

/// A particle rule dictionary whose first two hashes are swapped.
#[test]
fn a_rule_dictionary_out_of_order_is_refused() {
    let Some(paths) = files(EXT, Some(Generation::Gen8), "a broken .ypt refused") else {
        return;
    };
    let t = Generation::Gen8;
    let ypt = gates::format(EXT);
    let opened = Opened::read(&paths[0]);
    let image = opened.image();
    let list = offset_of!(Raw<PtxFxList<Gen8>>, particle_rule_dict);
    let dict = image.resolve(image.pointer_at(list).unwrap()).unwrap();
    let codes = offset_of!(Raw<PgDictionary<PtxParticleRule<Gen8>>>, codes);
    let hashes = image
        .resolve(image.pointer_at(dict + codes).unwrap())
        .unwrap();
    let mut bytes = opened.bytes.bytes().to_vec();
    let (a, b) = (hashes, hashes + 4);
    let first: [u8; 4] = bytes[a..a + 4].try_into().unwrap();
    bytes.copy_within(b..b + 4, a);
    bytes[b..b + 4].copy_from_slice(&first);
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = ypt
        .read(t, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = ypt.save(t, &tree).expect_err("out of order").to_string();
    println!("{}: {why}", name(&paths[0]));
    assert!(
        why.contains("particle rule dictionary's hashes are not ascending"),
        "{why}"
    );
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_ypt_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    // Every particle rule's texture frames and behaviour count, typed from the root.
    #[inline(never)]
    fn typed_sum<G: DrawableTarget>(image: Image<'_>) -> usize {
        let list = View::<Raw<PtxFxList<G>>>::at_offset(image, 0).unwrap();
        let Some(rules) = list.particle_rule_dict().unwrap() else {
            return 0;
        };
        let run = rules.entries().unwrap();
        let mut total = 0usize;
        for r in (0..run.len()).filter_map(|i| run.follow(i).ok().flatten()) {
            total += r.tex_frame_id_max as usize + r.all_behaviours().unwrap().len();
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
        let Some(paths) = files(EXT, Some(t), "the .ypt read cost") else {
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
        let tree_ms = best(3, &mut || {
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
            "{} {} files: every particle rule's frames and behaviours typed from the root \
             {typed_ms:.4} ms; the whole tree read {tree_ms:.2} ms ({:.0}x)",
            paths.len(),
            t.edition_label(),
            tree_ms / typed_ms
        );
    }
}
