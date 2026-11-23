//! `.yld` through its declarations, over the corpus: typed reads against the
//! tree, the owned form against the tree's save, the recorded testimony
//! re-keyed by offset, the canonical fixed point and the alignment census;
//! then what the game assumes of a cloth, and the refusals that say so.

use or_io::decl::{Described, File, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::bounds::{PhBound, PhBoundComposite};
use or_io::gta5::cloth_dict::{
    CharacterCloth, CharacterClothController, ClothBridgeSimGfx, ClothVertexTags, PhClothData,
    PhMorphController, PhVerletCloth, LOD_COUNT,
};
use or_io::gta5::shared::PgDictionary;
use or_io::Generation;
use or_io_verify::gates::{self, files, name, offset_of_run, Filed, Gates, Opened};
use std::mem::offset_of;

const T: Generation = Generation::Gen8;

type E = ViewError;

mod bound_walk;

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

/// A cloth's bound: a composite whose runs the game may leave null, as every shipped cloth's
/// `type_and_include_flags` is, and whose children read as a `.ybn`'s do.
fn bound(image: Image<'_>, b: View<'_, Raw<PhBound>>, out: &mut Filed) -> Result<(), E> {
    let Some(c) = View::variant::<Raw<PhBoundComposite>>(&b)? else {
        return bound_walk::walk(image, b, out);
    };
    put(out, &c);
    let children = c.bounds()?;
    for i in 0..children.len() {
        if let Some(child) = children.follow(i)? {
            bound(image, child, out)?;
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
    Ok(())
}

fn level(image: Image<'_>, v: &View<'_, Raw<PhVerletCloth>>, out: &mut Filed) -> Result<(), E> {
    put(out, v);
    for b in [v.custom_bound()?, v.virtual_bound()?]
        .into_iter()
        .flatten()
    {
        bound(image, b, out)?;
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
    c: &View<'_, Raw<CharacterClothController>>,
    out: &mut Filed,
) -> Result<(), E> {
    put(out, c);
    let base = c.base();
    if let Some(b) = base.bridge()? {
        bridge(image, &b, out)?;
    }
    if let Some(m) = base.morph_controller()? {
        morph(image, &m, out)?;
    }
    for l in [
        base.cloth_0()?,
        base.cloth_1()?,
        base.cloth_2()?,
        base.cloth_3()?,
    ]
    .into_iter()
    .flatten()
    {
        level(image, &l, out)?;
    }
    bytes(image, out, &c.tri_indices()?);
    run(out, c.original_pos()?);
    bytes(image, out, &c.bone_index_map()?);
    run(out, c.binding_info()?);
    bytes(image, out, &c.bone_id_map()?);
    Ok(())
}

fn typed_yld(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: E| e.to_string();
    let mut out = Filed::default();
    let dict = View::<Raw<PgDictionary<CharacterCloth>>>::at_offset(image, 0).map_err(e)?;
    put(&mut out, &dict);
    bytes(image, &mut out, &dict.codes().map_err(e)?);
    let entries = dict.entries().map_err(e)?;
    for i in 0..entries.len() {
        let Some(cloth) = entries.follow(i).map_err(e)? else {
            continue;
        };
        put(&mut out, &cloth);
        run(&mut out, cloth.poses().map_err(e)?);
        bytes(image, &mut out, &cloth.bone_id().map_err(e)?);
        bytes(image, &mut out, &cloth.bone_index().map_err(e)?);
        if let Some(b) = cloth.bound_composite().map_err(e)? {
            let b = View::<Raw<PhBound>>::at_offset(image, View::offset(&b)).map_err(e)?;
            bound(image, b, &mut out).map_err(e)?;
        }
        if let Some(c) = cloth.cloth_controller().map_err(e)? {
            controller(image, &c, &mut out).map_err(e)?;
        }
    }
    Ok(out)
}

const YLD: Gates = Gates {
    census_types: 12,
    ..Gates::new("yld", T, ("PhEdgeData", "weight0"))
};

or_io_verify::declared_gates! {
    gates: YLD,
    typed: typed_yld,
    reads_typed: every_yld_reads_typed_as_the_tree_reads,
    owned: PgDictionary<CharacterCloth> => every_yld_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_yld_saves_to_a_fixed_point,
    census: every_yld_instance_is_aligned_for_its_view,
    testimony: the_yld_testimony_holds_through_its_declarations,
    one_byte_late: a_yld_field_one_byte_late_is_named_by_the_testimony,
}

/// Every staged `.yld` holds one cloth, so the dictionary is broken by
/// stating no hash for it.
#[test]
fn a_cloth_dictionary_whose_hashes_disagree_with_its_cloths_is_refused() {
    let Some(paths) = files("yld", Some(T), "a broken .yld refused") else {
        return;
    };
    let yld = gates::format("yld");
    let opened = Opened::read(&paths[0]);
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[0x28..0x2a].copy_from_slice(&0u16.to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = yld
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = yld
        .save(T, &tree)
        .expect_err("no hash for its cloth")
        .to_string();
    println!("{}: {why}", name(&paths[0]));
    assert!(why.contains("holds 0 hashes and 1 cloths"), "{why}");
}

/// A level's two counts are written from their runs by an owned save, so a
/// file that states another is broken through the tree.
#[test]
fn a_level_whose_counts_disagree_with_its_runs_is_refused() {
    let Some(paths) = files("yld", Some(T), "a level's counts refused") else {
        return;
    };
    let yld = gates::format("yld");
    let opened = Opened::read(&paths[0]);
    let level = {
        let dict = View::<Raw<PgDictionary<CharacterCloth>>>::at_offset(opened.image(), 0).unwrap();
        let cloth = dict.entries().unwrap().follow(0).unwrap().unwrap();
        let c = cloth.cloth_controller().unwrap().unwrap();
        View::offset(&c.base().cloth_0().unwrap().expect("a level"))
    };
    for (f, field, what) in [
        (
            "num_edges",
            offset_of!(Raw<PhVerletCloth>, num_edges),
            "edge(s)",
        ),
        (
            "num_vertices",
            offset_of!(Raw<PhVerletCloth>, num_vertices),
            "vertex/vertices",
        ),
    ] {
        let mut bytes = opened.bytes.bytes().to_vec();
        let at = level + field;
        let stated = i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) + 1;
        bytes[at..at + 4].copy_from_slice(&stated.to_le_bytes());
        let aligned = or_io::decl::Aligned::from_bytes(&bytes);
        let tree = yld
            .read(T, Image::new(aligned.bytes(), opened.virtual_size))
            .expect("reads");
        let why = yld.save(T, &tree).expect_err(f).to_string();
        assert!(why.contains(&format!("states {stated} {what}")), "{why}");
    }
}

/// Every `.yld` opened owned, by file name.
///
/// ⚠ **Every refusal is fatal.** Skipping a file that will not open lets a
/// present corpus yield nothing, and every check below then passes having
/// opened nothing.
fn owned_yld(what: &str) -> Option<Vec<(String, PgDictionary<CharacterCloth>)>> {
    let paths = files("yld", Some(T), what)?;
    let out: Vec<_> = paths
        .iter()
        .map(|p| {
            let dict = PgDictionary::<CharacterCloth>::open(p)
                .unwrap_or_else(|e| panic!("{}: {e}", name(p)));
            (name(p), dict)
        })
        .collect();
    assert!(!out.is_empty());
    Some(out)
}

fn controllers(
    dicts: &mut [(String, PgDictionary<CharacterCloth>)],
) -> impl Iterator<Item = (&String, &mut CharacterClothController)> {
    dicts.iter_mut().flat_map(|(file, d)| {
        let file: &String = file;
        d.entries
            .iter_mut()
            .flatten()
            .filter_map(|c| c.cloth_controller.as_deref_mut())
            .map(move |c| (file, c))
    })
}

/// The refusal an owned save makes of `dict`, as text.
fn refusal(dict: &PgDictionary<CharacterCloth>) -> String {
    dict.to_bytes()
        .expect_err("the save must refuse")
        .to_string()
}

/// ⚠ A blend index addresses the bone map, which is much shorter than the
/// vertex list, and nothing on the read path compares them.
#[test]
fn a_binding_naming_a_bone_the_map_does_not_hold_is_refused() {
    let Some(mut dicts) = owned_yld("a binding past the bone map refused") else {
        return;
    };
    let (file, i) = dicts
        .iter()
        .enumerate()
        .find_map(|(i, (f, d))| {
            d.entries
                .iter()
                .flatten()
                .filter_map(|c| c.cloth_controller.as_ref())
                .any(|c| !c.binding_info.is_empty() && !c.bone_index_map.is_empty())
                .then(|| (f.clone(), i))
        })
        .expect("a staged cloth carries a binding list and a bone map");
    let dict = &mut dicts[i].1;
    let c = dict
        .entries
        .iter_mut()
        .flatten()
        .filter_map(|c| c.cloth_controller.as_deref_mut())
        .find(|c| !c.binding_info.is_empty() && !c.bone_index_map.is_empty())
        .unwrap();
    let beyond = c.bone_index_map.len() as i32;
    c.binding_info[0].blend_indices[0] = beyond;
    c.binding_info[0].weights.x = 1.0;
    let why = refusal(dict);
    assert!(why.contains("bone-map entry"), "{file}: {why}");
}

/// A per-level bridge run that has parted company with its sibling.
#[test]
fn a_per_lod_run_of_its_own_length_is_refused() {
    let Some(mut dicts) = owned_yld("a per-level run of its own length refused") else {
        return;
    };
    let i = dicts
        .iter()
        .position(|(_, d)| {
            d.entries
                .iter()
                .flatten()
                .filter_map(|c| c.cloth_controller.as_ref()?.base.bridge.as_ref())
                .any(|b| b.vertex_weight_0.len() > 1 && !b.inflation_scale_0.is_empty())
        })
        .expect("a staged cloth carries a bridge with per-vertex weights");
    let (file, dict) = &mut dicts[i];
    let b = dict
        .entries
        .iter_mut()
        .flatten()
        .filter_map(|c| c.cloth_controller.as_mut()?.base.bridge.as_deref_mut())
        .find(|b| b.vertex_weight_0.len() > 1 && !b.inflation_scale_0.is_empty())
        .unwrap();
    b.vertex_weight_mut(0).expect("level 0 exists").pop();
    let why = refusal(dict);
    assert!(why.contains("inflation scale"), "{file}: {why}");
}

/// The cloth data's carried vertex count against the run it counts, through the tree.
#[test]
fn a_cloth_stating_a_vertex_count_it_does_not_hold_is_refused() {
    let Some(paths) = files("yld", Some(T), "a vertex count it does not hold refused") else {
        return;
    };
    let yld = gates::format("yld");
    let at = offset_of!(Raw<PhVerletCloth>, cloth_data) + offset_of!(Raw<PhClothData>, num_verts);
    let (opened, level, verts) = paths
        .iter()
        .find_map(|p| {
            let opened = Opened::read(p);
            let found = {
                let dict =
                    View::<Raw<PgDictionary<CharacterCloth>>>::at_offset(opened.image(), 0).ok()?;
                let entries = dict.entries().ok()?;
                (0..entries.len())
                    .filter_map(|i| entries.follow(i).ok().flatten())
                    .filter_map(|cloth| cloth.cloth_controller().ok().flatten())
                    .filter_map(|c| c.base().cloth_0().ok().flatten())
                    .map(|l| {
                        (
                            View::offset(&l),
                            { l.cloth_data().vertex_positions }.count(),
                        )
                    })
                    .find(|(_, verts)| *verts > 1)
            };
            found.map(|(level, verts)| (opened, level, verts))
        })
        .expect("a staged cloth simulates something");
    // ⚠ Not one more than stated: shipped files hold zero here, which has to stay legal.
    let stated = u16::try_from(verts - 1).expect("a u16 count");
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[level + at..level + at + 2].copy_from_slice(&stated.to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = yld
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = yld.save(T, &tree).expect_err("num_verts").to_string();
    assert!(
        why.contains(&format!(
            "states {stated} vertex/vertices and holds {verts}"
        )),
        "{why}"
    );
}

/// An edge joining a vertex the level does not hold.
#[test]
fn an_edge_past_the_vertices_is_refused() {
    let Some(mut dicts) = owned_yld("an edge past the vertices refused") else {
        return;
    };
    let i = dicts
        .iter()
        .position(|(_, d)| {
            d.entries
                .iter()
                .flatten()
                .filter_map(|c| c.cloth_controller.as_ref())
                .flat_map(|c| c.base.cloth().into_iter().flatten())
                .any(|v| !v.edge_data.is_empty())
        })
        .expect("a staged level holds edges");
    let (file, dict) = &mut dicts[i];
    let level = dict
        .entries
        .iter_mut()
        .flatten()
        .filter_map(|c| c.cloth_controller.as_deref_mut())
        .flat_map(|c| c.base.cloth_mut().into_iter().flatten())
        .find(|v| !v.edge_data.is_empty())
        .unwrap();
    level.edge_data[0].vert_indices[1] = level.cloth_data.vertex_positions.len() as u16;
    let why = refusal(dict);
    assert!(why.contains("joins vertices"), "{file}: {why}");
}

/// An edge removed saves with the count beside the run written from it.
#[test]
fn an_edge_removed_saves_its_count() {
    let Some(mut dicts) = owned_yld("an edge removed") else {
        return;
    };
    let (file, dict) = &mut dicts[0];
    let level = dict
        .entries
        .iter_mut()
        .flatten()
        .filter_map(|c| c.cloth_controller.as_deref_mut())
        .flat_map(|c| c.base.cloth_mut().into_iter().flatten())
        .find(|v| !v.edge_data.is_empty())
        .expect("a staged level holds edges");
    level.edge_data.pop();
    let left = level.edge_data.len();
    let back = PgDictionary::<CharacterCloth>::from_bytes(
        &dict.to_bytes().unwrap_or_else(|e| panic!("{file}: {e}")),
    )
    .unwrap();
    let level = back
        .entries
        .iter()
        .flatten()
        .filter_map(|c| c.cloth_controller.as_ref())
        .flat_map(|c| c.base.cloth().into_iter().flatten())
        .next()
        .unwrap();
    assert_eq!(level.edge_data.len(), left);
}

/// No shipped cloth is refused, the walk reaches a binding and a simulated
/// vertex, and a garment states its bones as tags as well as indices: every
/// weighted slot resolves to a tag, and the tags are wider than any index.
#[test]
fn a_garment_states_its_bones_as_tags_as_well_as_indices() {
    let Some(mut dicts) = owned_yld("a garment's two bone maps") else {
        return;
    };
    let (mut count, mut with_index_map, mut agree) = (0usize, 0usize, 0usize);
    let (mut bound, mut tagged, mut fully) = (0usize, 0usize, 0usize);
    let (mut widest_index, mut widest_tag) = (0i32, 0u16);
    for (_, c) in controllers(&mut dicts) {
        count += 1;
        if !c.bone_index_map.is_empty() {
            with_index_map += 1;
            widest_index = widest_index.max(c.bone_index_map.iter().copied().max().unwrap_or(0));
            if c.bone_index_map.len() == c.bone_id_map.len() {
                agree += 1;
            }
        }
        widest_tag = widest_tag.max(
            c.bone_id_map
                .iter()
                .filter_map(|t| u16::try_from(*t).ok())
                .max()
                .unwrap_or(0),
        );
        bound += c.binding_info.len();
        for v in 0..c.binding_info.len() {
            let tags = c.vertex_weight_tags(v).expect("a bound vertex");
            tagged += 1;
            fully += usize::from(tags.is_fully_tagged());
        }
    }
    let vertices: usize = dicts
        .iter()
        .flat_map(|(_, d)| d.entries.iter().flatten())
        .map(|c| (0..LOD_COUNT).map(|l| c.lod_vertex_count(l)).sum::<usize>())
        .sum();
    println!(
        "{count} controller(s), {with_index_map} with an index map, {agree} agreeing with \
         their tag map; {fully} of {tagged} bound vertices fully tagged; {vertices} simulated \
         vertices; widest index {widest_index}, widest tag {widest_tag}"
    );
    assert!(vertices > 0 && bound > 0, "the walk reached nothing");
    assert_eq!(agree, with_index_map);
    assert_eq!(fully, tagged);
    assert!(i32::from(widest_tag) > widest_index);
}

/// Setting a vertex's bones by tag stores the blend index the file holds, and a tag outside the garment's map has none.
#[test]
fn a_vertex_can_be_weighted_by_tag() {
    let Some(mut dicts) = owned_yld("a vertex weighted by tag") else {
        return;
    };
    let (file, c) = controllers(&mut dicts)
        .find(|(_, c)| !c.binding_info.is_empty() && c.bone_id_map.len() > 1)
        .expect("a staged garment carries a binding list and two bone tags");
    let second = u16::try_from(c.bone_id_map[1]).expect("a tag fits sixteen bits");
    let wanted = ClothVertexTags {
        tags: [Some(second), None, None, None],
        weights: [1.0, 0.0, 0.0, 0.0],
    };
    c.binding_info[0] = c
        .binding_for_tags(&wanted)
        .unwrap_or_else(|| panic!("{file}: tag {second:#x} has no blend index"));
    let back = c.vertex_weight_tags(0).expect("it reads back");
    assert_eq!(back.tags[0], Some(second));
    assert_eq!(back.weights[0], 1.0);
    assert!(back.is_fully_tagged());
    assert_eq!(c.binding_info[0].blend_indices[0], 1);
    let absent = ClothVertexTags {
        tags: [Some(0xFFFE), None, None, None],
        weights: [1.0, 0.0, 0.0, 0.0],
    };
    assert!(
        !c.bone_id_map
            .iter()
            .any(|t| u16::try_from(*t) == Ok(0xFFFE))
            && c.binding_for_tags(&absent).is_none()
    );
}
