//! `.yft` through its declarations, both targets, over the corpus: typed
//! reads against the tree, the owned form against the tree's save, the
//! recorded testimony re-keyed by offset, the canonical fixed point and the
//! alignment census; then the tables a save must keep. The drawables are the
//! `.ydr` declarations.

mod bound_walk;

use std::mem::{offset_of, size_of};

use or_io::decl::{Described, Gen8, Gen9, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::bounds::{PhBound, PhBoundComposite};
use or_io::gta5::cloth_dict::{
    ClothBridgeSimGfx, ClothController, PhMorphController, PhVerletCloth,
};
use or_io::gta5::drawable::{gen8, gen9, DrawableTarget, GrmGeometry, RmcDrawable};
use or_io::gta5::fragment::*;
use or_io::gta5::texture_dict::gen9::GrcTextureG9;
use or_io::gta5::RmcLod;
use or_io::Generation;
use or_io_verify::gates::{
    self, files, name, offset_of_run, FieldMoved, Filed, Gates, Opened, Overread, Relocated,
};

const EXT: &str = "yft";

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

/// A bound: a composite whose flag runs the game may leave null, as every
/// fragment's `type_and_include_flags` is, and whose children read as a `.ybn`'s do.
fn bound(image: Image<'_>, at: usize, out: &mut Filed) -> Result<(), E> {
    let b = View::<Raw<PhBound>>::at_offset(image, at)?;
    let Some(c) = View::variant::<Raw<PhBoundComposite>>(&b)? else {
        return bound_walk::walk(image, b, out);
    };
    put(out, &c);
    let children = c.bounds()?;
    for i in 0..children.len() {
        if let Some(child) = children.follow(i)? {
            bound(image, View::offset(&child), out)?;
        }
    }
    for r in [c.current_matrices(), c.last_matrices()]
        .into_iter()
        .flatten()
    {
        run(out, r);
    }
    if let Ok(r) = c.local_box_min_maxs() {
        run(out, r);
    }
    for r in [c.type_and_include_flags(), c.owned_type_and_include_flags()]
        .into_iter()
        .flatten()
    {
        run(out, r);
    }
    if let Some(t) = c.bvh_structure()? {
        put(out, &t);
        run(out, t.contiguous_nodes()?);
        run(out, t.subtree_headers()?);
    }
    Ok(())
}

fn drawable<G: Walked>(
    image: Image<'_>,
    root: View<'_, Raw<FragDrawable<G>>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, &root);
    if let Some(b) = root.bound()? {
        bound(image, View::offset(&b), out)?;
    }
    let extra = root.extra_bounds()?;
    for i in 0..extra.len() {
        if let Some(b) = extra.follow(i)? {
            bound(image, View::offset(&b), out)?;
        }
    }
    run(out, root.extra_bounds_matrices()?);
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

fn verlet(image: Image<'_>, v: &View<'_, Raw<PhVerletCloth>>, out: &mut Filed) -> Result<(), E> {
    put(out, v);
    for b in [v.custom_bound()?, v.virtual_bound()?]
        .into_iter()
        .flatten()
    {
        bound(image, View::offset(&b), out)?;
    }
    let data = v.cloth_data();
    for r in [
        data.vertex_pinned_positions()?,
        data.vertex_initial_normals()?,
        data.vertex_positions()?,
        data.vertex_prev_positions()?,
    ] {
        run(out, r);
    }
    run(out, v.custom_edge_data()?);
    run(out, v.edge_data()?);
    bytes(image, out, &v.collision_inst()?);
    bytes(image, out, &v.dynamic_pin_list().bits()?);
    Ok(())
}

fn bridge(
    image: Image<'_>,
    b: &View<'_, Raw<ClothBridgeSimGfx>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, b);
    for r in [
        b.pin_radius_0()?,
        b.pin_radius_1()?,
        b.pin_radius_2()?,
        b.pin_radius_3()?,
        b.vertex_weight_0()?,
        b.vertex_weight_1()?,
        b.vertex_weight_2()?,
        b.vertex_weight_3()?,
        b.inflation_scale_0()?,
        b.inflation_scale_1()?,
        b.inflation_scale_2()?,
        b.inflation_scale_3()?,
    ] {
        bytes(image, out, &r);
    }
    for r in [
        b.cloth_display_map_0()?,
        b.cloth_display_map_1()?,
        b.cloth_display_map_2()?,
        b.cloth_display_map_3()?,
    ] {
        bytes(image, out, &r);
    }
    bytes(image, out, &b.pinnable_list().bits()?);
    Ok(())
}

fn morph(image: Image<'_>, m: &View<'_, Raw<PhMorphController>>, out: &mut Filed) -> Result<(), E> {
    put(out, m);
    let slots = [
        m.map_data_0()?,
        m.map_data_1()?,
        m.map_data_2()?,
        m.map_data_3()?,
    ];
    for maps in slots.into_iter().flatten() {
        put(out, &maps);
        for map in [
            maps.m_map_0(),
            maps.m_map_1(),
            maps.m_map_2(),
            maps.m_map_3(),
        ] {
            run(out, map.weights()?);
            for r in [
                map.vtx_index()?,
                map.index0()?,
                map.index1()?,
                map.index2()?,
            ] {
                bytes(image, out, &r);
            }
        }
        for r in [
            maps.i_map_0()?,
            maps.i_map_1()?,
            maps.i_map_2()?,
            maps.i_map_3()?,
        ] {
            bytes(image, out, &r);
        }
    }
    Ok(())
}

fn controller(
    image: Image<'_>,
    c: &View<'_, Raw<ClothController>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, c);
    if let Some(b) = c.bridge()? {
        bridge(image, &b, out)?;
    }
    if let Some(m) = c.morph_controller()? {
        morph(image, &m, out)?;
    }
    for l in [c.cloth_0()?, c.cloth_1()?, c.cloth_2()?, c.cloth_3()?]
        .into_iter()
        .flatten()
    {
        verlet(image, &l, out)?;
    }
    Ok(())
}

fn events(out: &mut Filed, sets: [Option<View<'_, Raw<EvtSet>>>; 4]) {
    for s in sets.into_iter().flatten() {
        put(out, &s);
    }
}

fn lod<G: Walked>(
    image: Image<'_>,
    level: &View<'_, Raw<FragPhysicsLOD<G>>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, level);
    if let Some(body) = level.body_type()? {
        put(out, &body);
        let joints = body.ph_joint_types()?;
        for i in 0..joints.len() {
            let Some(j) = joints.follow(i)? else {
                continue;
            };
            if let Some(v) = View::variant::<Raw<PhJoint1DofType>>(&j)? {
                put(out, &v);
            } else if let Some(v) = View::variant::<Raw<PhJoint3DofType>>(&j)? {
                put(out, &v);
            } else {
                put(out, &j);
            }
        }
        run(out, body.resourced_ang_inertia_xyz_mass_w()?);
    }
    let groups = level.groups()?;
    for i in 0..groups.len() {
        if let Some(g) = groups.follow(i)? {
            put(out, &g);
            events(out, [g.death_eventset()?, None, None, None]);
        }
    }
    let children = level.children()?;
    for i in 0..children.len() {
        let Some(c) = children.follow(i)? else {
            continue;
        };
        put(out, &c);
        events(
            out,
            [
                c.continuous_eventset()?,
                c.collision_eventset()?,
                c.break_eventset()?,
                c.break_from_root_eventset()?,
            ],
        );
        for d in [c.undamaged_entity()?, c.damaged_entity()?]
            .into_iter()
            .flatten()
        {
            drawable(image, d, out)?;
        }
    }
    for a in [level.phys_damp_undamaged()?, level.phys_damp_damaged()?]
        .into_iter()
        .flatten()
    {
        put(out, &a);
        if let Some(b) = a.bound()? {
            bound(image, View::offset(&b), out)?;
        }
    }
    if let Some(b) = level.composite_bounds()? {
        bound(image, View::offset(&b), out)?;
    }
    run(out, level.undamaged_ang_inertia()?);
    run(out, level.damaged_ang_inertia()?);
    if let Some(links) = level.link_attachments()? {
        put(out, &links);
        run(out, links.elements()?);
    }
    Ok(())
}

fn typed<G: Walked>(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: E| e.to_string();
    let mut out = Filed::default();
    let root = View::<Raw<GtaFragType<G>>>::at_offset(image, 0).map_err(e)?;
    put(&mut out, &root);
    run(&mut out, root.lights().map_err(e)?);
    let f = root.base();
    let walk = |out: &mut Filed| -> Result<(), E> {
        for d in [f.common_drawable()?, f.common_cloth_drawable()?]
            .into_iter()
            .flatten()
        {
            drawable(image, d, out)?;
        }
        let extras = f.extra_drawables()?;
        for i in 0..extras.len() {
            if let Some(d) = extras.follow(i)? {
                drawable(image, d, out)?;
            }
        }
        let cloths = f.env_cloth()?;
        for i in 0..cloths.len() {
            let Some(c) = cloths.follow(i)? else {
                continue;
            };
            put(out, &c);
            if let Some(t) = c.tuning()? {
                put(out, &t);
            }
            if let Some(d) = c.referenced_drawable()? {
                drawable(image, d, out)?;
            }
            if let Some(ctl) = c.cloth_controller()? {
                controller(image, &ctl, out)?;
            }
        }
        events(out, [f.collision_eventset()?, None, None, None]);
        if let Some(set) = f.shared_matrix_set()? {
            put(out, &set);
            run(out, set.matrix_data()?);
        }
        let panes = f.glass_pane_model_infos()?;
        for i in 0..panes.len() {
            if let Some(p) = panes.follow(i)? {
                put(out, &p);
            }
        }
        if let Some(group) = f.physics_lod_group()? {
            put(out, &group);
            for level in [group.high_lod()?, group.medium_lod()?, group.low_lod()?]
                .into_iter()
                .flatten()
            {
                lod(image, &level, out)?;
            }
        }
        Ok(())
    };
    walk(&mut out).map_err(e)?;
    Ok(out)
}

/// As the `.ydr` suite: the older record's box overread, out of the geometry
/// or the Enhanced buffer view allocated after it. And that record's reader
/// took a glass pane for a 0x70-byte struct, which no later record does.
const OVERREAD: &[Overread] = &[
    Overread {
        record: "corpus-cda05819",
        inside: "BgPaneModelInfoBase",
        delta: 0,
        bytes: 0x70,
        because: "a glass pane read as a 0x70-byte struct the newer record no longer has",
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
    Overread {
        record: "corpus-cda05819",
        inside: "ShaderResourceViewG9",
        delta: 0,
        bytes: 0x20,
        because: "the same box on Enhanced, read from the buffer view after it",
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
    census_types: 75,
    relocated: RELOCATED,
    overread: OVERREAD,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen8, ("FragTypeChild", "bone_id"))
};

const ENHANCED: Gates = Gates {
    census_types: 57,
    relocated: RELOCATED,
    overread: OVERREAD,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen9, ("FragTypeChild", "bone_id"))
};

or_io_verify::declared_gates! {
    gates: LEGACY,
    typed: typed::<Gen8>,
    reads_typed: every_yft_reads_typed_as_the_tree_reads,
    owned: GtaFragType<Gen8> => every_legacy_yft_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_yft_saves_to_a_fixed_point,
    census: every_yft_instance_is_aligned_for_its_view,
    testimony: the_yft_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_field_one_byte_late_is_named_by_the_testimony,
}

or_io_verify::declared_gates! {
    gates: ENHANCED,
    typed: typed::<Gen9>,
    reads_typed: every_enhanced_yft_reads_typed_as_the_tree_reads,
    owned: GtaFragType<Gen9> => every_enhanced_yft_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_enhanced_yft_saves_to_a_fixed_point,
    census: every_enhanced_yft_instance_is_aligned_for_its_view,
    testimony: the_enhanced_yft_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_enhanced_field_one_byte_late_is_named_by_the_testimony,
}

/// What a save must keep beside the records: the extra drawables' names,
/// the vehicle glass block, and each physics level's group names.
#[derive(Debug, PartialEq, Eq, Default)]
struct Tables {
    extra_names: Vec<String>,
    vehicle_glass: Option<Vec<u8>>,
    group_names: Vec<Vec<String>>,
}

/// The tables read from the bytes by offset, so neither the reader nor the writer under test answers.
fn tables(image: Image<'_>) -> Tables {
    type Frag = Raw<FragType<Gen8>>;
    type Lod = Raw<FragPhysicsLOD<Gen8>>;
    type Group = Raw<FragPhysicsLODGroup<Gen8>>;
    let ptr = |at: usize| -> Option<usize> {
        let a = image.pointer_at(at).ok()?;
        (a != 0).then(|| image.resolve(a).ok()).flatten()
    };
    let text = |at: usize| String::from_utf8_lossy(image.c_str_at(at).unwrap()).into_owned();
    let u32_at = |at: usize| u32::from_le_bytes(image.bytes()[at..at + 4].try_into().unwrap());
    let mut out = Tables::default();
    if let Some(table) = ptr(offset_of!(Frag, extra_drawable_names)) {
        out.extra_names = (0..u32_at(offset_of!(Frag, num_extra_drawables)) as usize)
            .filter_map(|i| ptr(table + 8 * i))
            .map(text)
            .collect();
    }
    if let Some(glass) = ptr(offset_of!(Raw<GtaFragType<Gen8>>, vehicle_window_data)) {
        let len =
            u32_at(glass + offset_of!(Raw<FwVehicleGlassWindowDataHeader>, total_length)) as usize;
        out.vehicle_glass = Some(image.bytes()[glass..glass + len].to_vec());
    }
    if let Some(levels) = ptr(offset_of!(Frag, physics_lod_group)) {
        for slot in [
            offset_of!(Group, high_lod),
            offset_of!(Group, medium_lod),
            offset_of!(Group, low_lod),
        ] {
            let Some(level) = ptr(levels + slot) else {
                continue;
            };
            let Some(names) = ptr(level + offset_of!(Lod, group_names)) else {
                continue;
            };
            let count = image.bytes()[level + offset_of!(Lod, num_groups)] as usize;
            out.group_names.push(
                (0..count)
                    .map(|i| text(ptr(names + 8 * i).unwrap()))
                    .collect(),
            );
        }
    }
    out
}

#[test]
fn a_saved_yft_keeps_its_names_and_its_glass() {
    for t in [Generation::Gen8, Generation::Gen9] {
        let Some(paths) = files(EXT, Some(t), "a saved .yft's tables") else {
            continue;
        };
        let fmt = gates::format(EXT);
        let (mut names, mut glass, mut levels) = (0, 0, 0);
        let mut lost = Vec::new();
        for path in &paths {
            let opened = Opened::read(path);
            let before = tables(opened.image());
            let saved = Opened::of(&fmt.save(t, &fmt.read(t, opened.image()).unwrap()).unwrap());
            let after = tables(saved.image());
            if after != before {
                lost.push(name(path));
            }
            names += usize::from(!before.extra_names.is_empty());
            glass += usize::from(before.vehicle_glass.is_some());
            levels += before.group_names.len();
        }
        println!(
            "{} {} fragments: {names} name their extra drawables, {glass} hold vehicle glass, \
             {levels} physics levels name their groups; {} lose a table on a save",
            paths.len(),
            t.edition_label(),
            lost.len()
        );
        assert!(lost.is_empty(), "{lost:?}");
    }
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_yft_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    // Every fragment child's masses and bone id, typed from the root.
    #[inline(never)]
    fn typed_sum<G: DrawableTarget>(image: Image<'_>) -> usize {
        let root = View::<Raw<GtaFragType<G>>>::at_offset(image, 0).unwrap();
        let Some(group) = root.base().physics_lod_group().unwrap() else {
            return 0;
        };
        let mut total = 0usize;
        for level in [group.high_lod(), group.medium_lod(), group.low_lod()]
            .into_iter()
            .filter_map(|l| l.unwrap())
        {
            let run = level.children().unwrap();
            for c in (0..run.len()).filter_map(|i| run.follow(i).ok().flatten()) {
                total += usize::from(c.bone_id) + c.undamaged_mass as usize;
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
        let Some(paths) = files(EXT, Some(t), "the .yft read cost") else {
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
            "{} {} files: every child's masses and bone id typed from the root {typed_ms:.4} ms; \
             the whole tree read {tree_ms:.2} ms ({:.0}x)",
            paths.len(),
            t.edition_label(),
            tree_ms / typed_ms
        );
    }
}
