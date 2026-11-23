//! `rpf scan-open` on a headerless GTA VI resource the scan gave a signature: the facts its reader
//! reads, by role and with `hash_XXXXXXXX` for a name only its hash gives.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use or_io::container::rpf::{Rpf9Resource, Rpf9Signature};
use or_io::gta5::shaders::sgd5::{drawable_parameter, holds_floats, parameter_names};
use or_io::gta5::shaders::shader_defs::{self, ShaderDefinitions};
use or_io::gta6::hash::name_hash64;
use or_io::gta6::headerless::{
    material_key, read, read_skeleton, Bone, Border, Bound, ChildBound, DictionaryTexture,
    Drawable, Entry, Geometry, HeaderlessError, NavMesh, NavOctree, Palette, ParameterValue,
    Primitive, Resource, Shader, UnknownShape, NOWHERE, SMALL, WATER,
};
use or_io::gta6::texture_names::{name_keys_with, Rule};

/// Print what the reader of `signature` reads from `bytes`, a read of `resource`: its virtual
/// section, then its physical section.
pub(crate) fn print(
    resource: &Rpf9Resource,
    bytes: &[u8],
    signature: Rpf9Signature,
    effects: Option<&ShaderDefinitions>,
) -> Result<(), String> {
    let role = signature.role().map_or("not known".to_string(), |r| {
        format!("{}, placed by its layout", r.label())
    });
    println!("  resource      headerless, signature {signature}, role {role}");
    let (virtual_section, physical) = resource.sections(bytes);
    println!(
        "  sections      virtual {} bytes in {} block(s), physical {} bytes in {} block(s), as the block map counts them",
        virtual_section.len(),
        resource.virtual_blocks,
        physical.len(),
        resource.physical_blocks
    );
    if signature.to_string() == or_io::gta6::anim::EXPRESSIONS_SIGNATURE {
        return super::gta6_anim::print_expressions(virtual_section);
    }
    let resource = read(signature, virtual_section, Some(physical)).map_err(|e| e.to_string())?;
    let parameters = (shader_defs::staged().as_ref()).map_or_else(HashMap::new, parameter_names);
    let naming = Naming {
        effects,
        parameters: &parameters,
        textures: HashMap::new(),
    };
    match resource {
        Resource::Bound(b) => print_bound("", &b),
        Resource::Drawable(d) => {
            print_drawable("", &d, &[], &naming);
            print_skeleton(virtual_section, false);
            print_embedded(virtual_section, physical, false);
        }
        Resource::Dictionary(dict) => {
            println!("  entries       {}", dict.entries.len());
            for (code, entry) in &dict.entries {
                match entry {
                    Ok(Entry::Drawable(d)) => {
                        println!("  entry code {code:#010X}");
                        print_drawable("  ", d, &[], &naming);
                    }
                    Ok(Entry::Texture(t)) => print_texture(*code, t),
                    Ok(Entry::FrameFilter { name, tracks }) => {
                        println!(
                            "  entry code {code:#010X}  frame filter {name:?}, {} track(s), {} with a weight in 0 to 1",
                            tracks.len(),
                            tracks.iter().filter(|t| (0.0..=1.0).contains(&t.weight)).count()
                        );
                        for t in tracks {
                            println!(
                                "    track {}  weight {}, group {}, word at 0x2C {:#010X}",
                                or_io::gta6::anim::track_label(t.track),
                                t.weight,
                                or_io::gta6::anim::track_label(t.group),
                                t.word_2c
                            );
                        }
                    }
                    Ok(Entry::AnimationRecord { name, bones }) => {
                        println!(
                            "  entry code {code:#010X}  animation record {name:?}, {} bone record(s)",
                            bones.len()
                        );
                        for b in bones {
                            println!(
                                "    bone hash_{:08X}  kind {}, flags {:#010X}, values {:?}",
                                b.bone, b.kind, b.flags, b.values
                            );
                        }
                    }
                    Ok(Entry::Word64(word)) => println!(
                        "  entry code {code:#010X}  64-bit word {word:016X}, role not known"
                    ),
                    Err(e) => println!("  entry code {code:#010X}  not read: {e}"),
                }
            }
            super::gta6_anim::print_dictionary_skeletons(virtual_section);
        }
        Resource::DrawableWithBound(r) => {
            println!("  name          {}", r.name);
            print_drawable("", &r.drawable, &[r.name.as_str()], &naming);
            print_skeleton(virtual_section, true);
            print_embedded(virtual_section, physical, true);
            match &r.bound {
                Some(bound) => print_bound("", bound),
                None => println!("  bound         none: the record at 0x10 is null"),
            }
            let skeleton = read_skeleton(virtual_section, true).ok().flatten();
            crate::gta6_fragment::print_fragment(&r.fragment, skeleton.as_ref());
        }
        Resource::Instances(set) => {
            println!(
                "  instances     {}, key {:#018X}; their structures tile {} bytes of the physical section",
                set.instances.len(),
                set.key,
                set.records_size()
            );
            for (i, instance) in set.instances.iter().enumerate() {
                let t = instance.transform;
                let structure = match &instance.structure {
                    Some(s) => format!(
                        "serialized {} bytes, {} primitives under a count of {}, box {:?} to {:?}, +0x108 {:#010X}, +0x128 {:#X}",
                        s.serialized_size, s.primitives, s.count, s.box_min, s.box_max, s.word_108, s.offset_128
                    ),
                    None => "no structure".to_string(),
                };
                println!(
                    "  instance {i:<4} size {:#X}, word {:#010X}, rows [{:?}, {:?}, {:?}]; {structure}",
                    instance.size,
                    instance.word,
                    &t[0..4],
                    &t[4..8],
                    &t[8..12]
                );
            }
            println!("  not read      each structure's nodes past +0x140, which are the console driver's");
        }
        Resource::MorphTarget(m) => {
            let extent = [24, 24, 16].map(|bits| 2f64.powi(bits));
            let extent: Vec<String> = (0..3)
                .map(|l| format!("{:.6}", f64::from(m.step[l]) * extent[l]))
                .collect();
            println!(
                "  morph target  of mesh hash_{:08X}: {} elements, lane origins {:?}, lane extents [{}]",
                m.mesh_key,
                m.elements.len(),
                m.origin,
                extent.join(", ")
            );
            if let Some((mesh, lod)) = m.mesh() {
                println!("  mesh          {mesh}, the base head at level of detail {lod}");
            }
            if m.is_empty() {
                println!("  empty         every step is zero, so the target moves nothing");
            }
            println!(
                "  not read      which axis each lane holds, so no delta is given as a vector"
            );
            println!("  not applied   which vertex each delta moves is not known");
        }
        Resource::NavMesh(m) => print_navmesh(&m),
        Resource::NavOctree(t) => print_octree(&t),
        Resource::Cloth(c) => crate::gta6_fragment::print_cloth(&c),
        Resource::VertexBuffers(set) => {
            for (i, (vertices, channels)) in set.buffers.iter().enumerate() {
                let channels: Vec<String> = channels.iter().map(|c| c.index.to_string()).collect();
                println!(
                    "  buffer {i:<4}   {vertices} vertices, channels {}",
                    channels.join(", ")
                );
            }
            println!("  not read      which drawable's index data these buffers serve: the body names none");
        }
        Resource::KeyedBlob(b) => {
            println!(
                "  keyed blob    key {:016X}, {} bytes in the {} section",
                b.key,
                b.len,
                if b.physical { "physical" } else { "virtual" }
            );
            println!("  not read      what the key hashes and what the bytes hold");
        }
        Resource::KeyedBlobDescriptor(d) => {
            println!(
                "  descriptor    of the keyed blob of key {:016X}: stream {} bytes, pair {} and {} at 0x50, words at 0x30 to 0x44 {}",
                d.key,
                d.stream.len(),
                d.pair[0],
                d.pair[1],
                d.words.map(|w| format!("{w:#X}")).join(", ")
            );
            println!("  not read      the stream's grammar, and the units of 0x30 to 0x44");
        }
        Resource::AssetIndex(a) => {
            println!(
                "  asset index   {} assets in table 0, each with a value record",
                a.assets.len()
            );
            for (t, group) in a.groups.iter().enumerate().filter(|(_, g)| !g.is_empty()) {
                println!(
                    "  table {}       {} keys, which list {} asset keys",
                    t + 1,
                    group.len(),
                    group.iter().map(|(_, l)| l.len()).sum::<usize>()
                );
            }
            println!("  not read      what a value record holds");
        }
        Resource::NeighbourTable(n) => {
            println!(
                "  neighbours    {} elements of four indices, each of another element",
                n.elements.len()
            );
            println!("  not read      which mesh the elements are of");
        }
        Resource::ParticleEffects(list) => {
            println!(
                "  particles     list {}: {} effect rules, {} emitter rules, {} particle rules, {} behaviours",
                list.name,
                list.effects.len(),
                list.emitters.len(),
                list.particles.len(),
                list.behaviours.len()
            );
            for e in &list.effects {
                println!("  effect        {} ({} events)", e.name, e.events.len());
            }
            let by_bit = |bit| or_io::gta6::headerless::class_of_bit(bit).and_then(|(c, _)| c);
            let open = (list.behaviours.iter())
                .filter(|b| b.class.is_none() && b.class_bit.and_then(by_bit).is_none())
                .count();
            println!("  not read      what {open} behaviours of a class bit GTA V has no class for hold, and what the GPU buffers' data holds past its shape (`rpf scan-particles` lists the buffers)");
        }
    }
    println!("  written       as a loose resource of its own (`rpf scan-body --stored`), never into the archive: its header is in the archive's encrypted table");
    Ok(())
}

/// The skeleton of the drawable at the root, or of a fragment's drawable, and how many of its
/// stated inverse binds undo the bind world in each order of composition.
fn print_skeleton(virtual_section: &[u8], fragment: bool) {
    let skeleton = match read_skeleton(virtual_section, fragment) {
        Ok(Some(s)) => s,
        Ok(None) => return println!("  skeleton      none: the drawable's 0x18 is null"),
        Err(e) => return println!("  skeleton      not read: {e}"),
    };
    let local = |b: &Bone| {
        Mat4::from_scale_rotation_translation(
            Vec3::from(b.scale),
            Quat::from_array(b.rotation),
            Vec3::from(b.translation),
        )
    };
    let (mut forward, mut reverse) = (Vec::<Mat4>::new(), Vec::<Mat4>::new());
    let undoes = |world: &Mat4, b: &Bone| {
        let [x, y, z, w] = b.inverse_bind.map(Vec3::from);
        let inverse = Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), w.extend(1.0));
        (*world * inverse).abs_diff_eq(Mat4::IDENTITY, 1e-3)
    };
    let (mut held, mut held_reversed) = (0, 0);
    // The reader refuses a parent that does not come before its bone.
    for b in &skeleton.bones {
        let parent = |worlds: &[Mat4]| b.parent.map_or(Mat4::IDENTITY, |p| worlds[usize::from(p)]);
        let (world, world_reversed) = (parent(&forward) * local(b), local(b) * parent(&reverse));
        held += usize::from(undoes(&world, b));
        held_reversed += usize::from(undoes(&world_reversed, b));
        forward.push(world);
        reverse.push(world_reversed);
    }
    println!(
        "  skeleton      {} bones; the stated inverse bind undoes parent world x local in {held}, local x parent world in {held_reversed}",
        skeleton.bones.len()
    );
}

fn print_texture(code: u32, t: &DictionaryTexture) {
    println!(
        "  entry code {code:#010X}  texture {:?}, pack key {:016X}: {}x{}x{}, dimension {}, DXGI {}, {} mip(s), pixels at physical {:#X}, {} bytes",
        t.name,
        name_hash64(t.name.as_bytes()),
        t.width,
        t.height,
        t.depth,
        t.dimension,
        t.format,
        t.mip_levels,
        t.pixels_at,
        t.pixels_len
    );
    match &t.texture {
        Ok(read) => println!(
            "    read        {} slice(s) of {} mip(s), {}",
            read.faces,
            read.levels,
            read.format.name()
        ),
        Err(e) => println!("    not read    {e}"),
    }
}

/// Each texture the shader group of the drawable at the root, or of a fragment's drawable, embeds;
/// nothing where it embeds none.
fn print_embedded(virtual_section: &[u8], physical: &[u8], fragment: bool) {
    let head =
        or_io::gta6::headerless::read_drawable_head(virtual_section, Some(physical), fragment);
    let textures = match head {
        Ok(head) => head.textures,
        Err(e) => return println!("  embedded      not read: {e}"),
    };
    for texture in &textures {
        let t = match texture {
            Ok(t) => t,
            Err(e) => {
                println!("  embedded      a texture not read: {e}");
                continue;
            }
        };
        let d = &t.texture;
        println!(
            "  embedded      texture key {:016X}, hash_{:08X} in its dictionary: {}x{}x{}, dimension {}, DXGI {}, {} mip(s), pixels at physical {:#X}, {} bytes",
            t.key,
            t.hash,
            d.width,
            d.height,
            d.depth,
            d.dimension,
            d.format,
            d.mip_levels,
            d.pixels_at,
            d.pixels_len
        );
        match &d.texture {
            Ok(read) => println!(
                "    read        {} slice(s) of {} mip(s), {}; --export writes it as hash_{:016X}",
                read.faces,
                read.levels,
                read.format.name(),
                t.key
            ),
            Err(e) => println!("    not read    {e}"),
        }
    }
}

fn print_bound(indent: &str, b: &Bound) {
    let bvh = b.bvh.map_or("no BVH".to_string(), |h| {
        format!("BVH of {} nodes, {} leaves", h.nodes, h.leaves)
    });
    println!(
        "{indent}  bound         {} children, box {:?} to {:?}, radius {}, {bvh}",
        b.children.len(),
        b.box_min,
        b.box_max,
        b.radius
    );
    for (i, child) in b.children.iter().enumerate() {
        let flags = child
            .flags
            .map_or("none".to_string(), |f| format!("{:#X} {:#X}", f[0], f[1]));
        let g = match &child.bound {
            None => {
                println!("{indent}    child {i}  null");
                continue;
            }
            Some(Err(e)) => {
                println!("{indent}    child {i}  not read: {e}");
                continue;
            }
            Some(Ok(ChildBound::Geometry(g))) => g,
            Some(Ok(primitive)) => {
                let key = child.material_id.map_or("none".to_string(), |id| {
                    format!("{:#06X}", material_key(id))
                });
                println!("{indent}    child {i}  {primitive:?}; material key {key}; flags {flags}");
                continue;
            }
        };
        let mut kinds = [0usize; 5];
        for p in &g.primitives {
            kinds[match p {
                Primitive::Triangle { .. } => 0,
                Primitive::Sphere { .. } => 1,
                Primitive::Capsule { .. } => 2,
                Primitive::Box(_) => 3,
                Primitive::Cylinder { .. } => 4,
            }] += 1;
        }
        let keys: std::collections::BTreeSet<u16> = (0..g.primitives.len())
            .filter_map(|p| g.primitive_key(p))
            .collect();
        let keys: Vec<String> = keys.iter().map(|k| format!("{k:#06X}")).collect();
        let ends = (g.unknown_shapes.iter())
            .filter(|(_, why)| *why == UnknownShape::EndsOnOneVertex)
            .count();
        let bvh = g.bvh.map_or("no BVH".to_string(), |h| {
            format!("BVH of {} nodes, {} leaves", h.nodes, h.leaves)
        });
        let open = (g.primitives.iter())
            .map(|p| match p {
                Primitive::Triangle { edges, .. } => edges.iter().filter(|e| **e == 0xFF).count(),
                _ => 0,
            })
            .sum::<usize>();
        println!(
            "{indent}    child {i}  {} vertices; {} triangles ({open} edges marked open), {} spheres, {} capsules, {} boxes, {} cylinders; {} shapes not known ({ends} with one vertex as both ends, {} off their BVH leaf); {} materials, {} per-primitive material indices, keys {}; {bvh}; flags {flags}",
            g.vertices.len(),
            kinds[0],
            kinds[1],
            kinds[2],
            kinds[3],
            kinds[4],
            g.unknown_shapes.len(),
            g.unknown_shapes.len() - ends,
            g.material_ids.len(),
            g.primitive_materials.len(),
            keys.join(" "),
        );
    }
    println!("{indent}  not read      the root's 0x9C, a geometry's +0xD8 and its tables at +0x138 and +0x148, whose meanings are not known; a material key's name");
}

fn print_navmesh(m: &NavMesh) {
    let mut corners = std::collections::BTreeMap::new();
    for p in &m.polygons[..m.polygons_proper.min(m.polygons.len())] {
        *corners.entry(p.corners).or_insert(0usize) += 1;
    }
    let (small, water) = (m.polygons.iter()).fold((0, 0), |(s, w), p| {
        (
            s + usize::from(p.flags & SMALL != 0),
            w + usize::from(p.flags & WATER != 0),
        )
    });
    let (into, nowhere) = (m.edges.iter()).fold((0, 0), |(i, n), e| {
        (
            i + usize::from(e.mesh != NOWHERE),
            n + usize::from(e.mesh == NOWHERE),
        )
    });
    println!(
        "  navigation    cell {} (x {}, y {} of a 512 x 512 grid of 128 m); box {:?} to {:?}; flags {}; build {:08X}",
        m.cell,
        m.cell % 512,
        m.cell / 512,
        m.tree.box_min,
        m.tree.box_max,
        m.flags,
        m.build_id
    );
    println!(
        "  arrays        vertices {}, indices {}, adjacency {}, polygons {}; corners per polygon {corners:?}",
        m.vertices.len(),
        m.indices.len(),
        m.edges.len(),
        m.polygons.len()
    );
    println!(
        "  polygons      {} named by the leaves ({small} small, {water} water of all), {} side records after them",
        m.polygons_proper,
        m.polygons.len() - m.polygons_proper.min(m.polygons.len())
    );
    println!(
        "  quadtree      {} nodes, {} leaves, depth {}",
        m.nodes, m.leaves, m.depth
    );
    println!("  adjacency     {into} in-cell links; {nowhere} without neighbour");
    println!(
        "  records       {} special links, {} special link indices, {} footprints",
        m.special_links.len(),
        m.special_link_indices.len(),
        m.footprints.len()
    );
    let edges = m.border_edges();
    for border in [Border::East, Border::West, Border::North, Border::South] {
        let on: Vec<_> = edges.iter().filter(|e| e.border == border).collect();
        if on.is_empty() {
            continue;
        }
        let records: std::collections::BTreeSet<usize> = on.iter().map(|e| e.record).collect();
        let named = on.iter().filter(|e| e.polygon.is_some()).count();
        println!(
            "  border        {border:?}, cell {} across: {} side records, {} chain edges, {named} naming a polygon proper",
            border.across(m.cell).map_or("none".to_string(), |c| c.to_string()),
            records.len(),
            on.len()
        );
    }
    // A link's heading against the direction from its start to its end, for links of 0.5 m or more.
    let (mut long, mut along) = (0, 0);
    for l in &m.special_links {
        let (from, to) = (m.position(l.from), m.position(l.to));
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        if dx.hypot(dy) < 0.5 {
            continue;
        }
        long += 1;
        let turn = (dy.atan2(dx).to_degrees() - 90.0).rem_euclid(360.0) * 256.0 / 360.0;
        let off = (turn - l.heading as f32).rem_euclid(256.0);
        along += usize::from(off.min(256.0 - off) <= 8.0);
    }
    let low_bits = (m.special_links.iter())
        .filter(|l| l.word_18 & 7 != 0)
        .count();
    println!(
        "  links         heading within 8/256 turn of the start-to-end direction: {along} of {long} links of 0.5 m or more; {low_bits} with +0x18 bits 0 to 2 set"
    );
    let mut kinds = std::collections::BTreeMap::new();
    for f in &m.footprints {
        *kinds.entry(format!("{:#06X}", f.kind)).or_insert(0usize) += 1;
    }
    let eight = (m.footprints.iter())
        .filter(|f| f.corners().len() == 8)
        .count();
    println!(
        "  footprints    kinds {kinds:?}; {eight} of eight corners, {} of four, in 1/512 m",
        m.footprints.len() - eight
    );
    let mut flags = std::collections::BTreeMap::new();
    let proper = &m.polygons[..m.polygons_proper.min(m.polygons.len())];
    for p in proper {
        let first = p.first as usize;
        for e in m.edges.iter().skip(first).take(usize::from(p.corners)) {
            *flags.entry(e.flags & 0x1F).or_insert(0usize) += 1;
        }
    }
    let steep = (proper.iter()).fold((u8::MAX, 0), |(lo, hi), p| {
        (lo.min(p.steepness), hi.max(p.steepness))
    });
    println!(
        "  open, raw     polygons' steepness (+0x25) {} to {}; polygons' edge flags (low five bits of +0x08) {flags:?}",
        steep.0, steep.1
    );
    println!("  not read      polygon +0x20 but its link run (bits 0 to 21), +0x24 but byte 1, and +0x28, a footprint's +0x54 and +0x58, whose meanings are not known");
}

fn print_octree(t: &NavOctree) {
    println!(
        "  octree        cell {}, corner {:?}, root edge {} m, depth {}",
        t.cell, t.corner, t.edge, t.depth
    );
    println!(
        "  nodes         {} in {} hash groups: {} internal, {} placeholder, {} open, {} surface",
        t.nodes, t.groups, t.internal, t.placeholder, t.open, t.surface
    );
    println!("  not read      a surface leaf's byte 1 but bit 0, an open leaf's byte 1 and layer unit, and the x and y order of a code's bits, which are not settled");
}

/// What names a drawable's parts: the effects of an `SGD5` container, a GTA V database's parameter
/// names by hash, and the texture names the naming rules give from the names the drawable states.
struct Naming<'a> {
    effects: Option<&'a ShaderDefinitions>,
    parameters: &'a HashMap<u32, String>,
    textures: HashMap<u64, (String, Rule)>,
}

/// A shader's effect and counts, then a line per parameter with what names and joins it; a name
/// nothing gives is `hash_` and the hash.
fn shader_lines(s: &Shader, naming: &Naming) -> Vec<String> {
    let effect = naming.effects.and_then(|e| e.shader(s.name_hash));
    let parameter = |hash: u32| {
        let name = naming.parameters.get(&hash);
        name.map_or_else(
            || format!("hash_{hash:08X}"),
            |n| format!("{n} (hash_{hash:08X})"),
        )
    };
    let joined: Vec<_> = (s.parameters.iter())
        .map(|p| effect.and_then(|e| drawable_parameter(e, p.hash)))
        .collect();
    // Each buffer's size, as the reader placed the buffers: its last constant's end, 16-aligned.
    let mut buffers: Vec<usize> = Vec::new();
    let mut constants = 0;
    for p in &s.parameters {
        if let ParameterValue::Constant {
            buffer,
            offset,
            words,
        } = &p.value
        {
            let b = usize::from(*buffer);
            if buffers.len() <= b {
                buffers.resize(b + 1, 0);
            }
            buffers[b] =
                buffers[b].max((usize::from(*offset) + 4 * words.len()).next_multiple_of(16));
            constants += 1;
        }
    }
    let effect_name = effect.map_or_else(
        || format!("hash_{:08X}, effect not given", s.name_hash),
        |e| format!("{} (hash_{:08X})", e.name, s.name_hash),
    );
    let mut lines = vec![format!(
        "{effect_name}, buffers {buffers:?}, {} texture slot(s), {constants} constant(s); {} of {} parameter(s) joined to the effect by their +0x14 hash",
        s.texture_keys.len(),
        joined.iter().flatten().count(),
        s.parameters.len()
    )];
    for (p, def) in s.parameters.iter().zip(&joined) {
        let declared = match (effect, def) {
            (None, _) => String::new(),
            (Some(_), Some(def)) => format!("; effect type {}", def.kind.as_str()),
            (Some(_), None) => "; not in the effect's table".to_string(),
        };
        lines.push(match &p.value {
            ParameterValue::Texture { slot } => {
                let key = s.texture_keys.get(usize::from(*slot)).copied().flatten();
                let key = key.map_or("empty slot".to_string(), |k| {
                    let named = naming.textures.get(&k);
                    let named = named.map_or("not named by a rule".to_string(), |(n, r)| {
                        format!("named {n} by rule {}", r.label())
                    });
                    format!("key {k:016X}, {named}")
                });
                format!("texture {slot:<3} {}: {key}{declared}", parameter(p.hash))
            }
            ParameterValue::Constant {
                buffer,
                offset,
                words,
            } => {
                let floats = def.is_some_and(holds_floats);
                let values: Vec<String> = (words.iter())
                    .map(|w| match floats {
                        true => f32::from_bits(*w).to_string(),
                        false => format!("{w:#010X}"),
                    })
                    .collect();
                format!(
                    "constant    {}: buffer {buffer}, offset {offset}, [{}]{declared}",
                    parameter(p.hash),
                    values.join(", ")
                )
            }
        });
    }
    lines
}

/// A drawable, its shaders and geometries; `stated` are the other names the resource states it by.
fn print_drawable(indent: &str, d: &Drawable, stated: &[&str], naming: &Naming) {
    println!(
        "{indent}  drawable      {:?}, box {:?} to {:?}",
        d.name, d.box_min, d.box_max
    );
    let keys = (d.shaders.iter())
        .flat_map(|s| s.texture_keys.iter().flatten().copied())
        .collect();
    let names = stated.iter().copied().chain([d.name.as_str()]);
    let naming = Naming {
        textures: name_keys_with(super::gta6_names::vocabulary(), names, &keys),
        ..*naming
    };
    for (i, s) in d.shaders.iter().enumerate() {
        let lines = shader_lines(s, &naming);
        println!("{indent}    shader {i}  {}", lines[0]);
        for line in &lines[1..] {
            println!("{indent}      {line}");
        }
    }
    println!("{indent}    distances {:?} m", d.lod_distances);
    if let Some(bound) = &d.bound {
        print_bound(&format!("{indent}  "), bound);
    }
    if let Some(surfaces) = &d.embedded {
        crate::gta6_fragment::print_surfaces(indent, surfaces);
    }
    for (l, level) in d.levels.iter().enumerate() {
        let Some(models) = level else { continue };
        println!("{indent}    level {l}   {} model(s)", models.len());
        for (m, model) in models.iter().enumerate() {
            let palette = match &model.palette {
                Ok(Palette::Hashes(h)) => format!("{} bone-name hashes", h.len()),
                Ok(Palette::Own) => "the own skeleton's bones by index".to_string(),
                Err(e) => format!("not read: {e}"),
            };
            println!(
                "{indent}      model {m}  palette {palette}; matrix index {}",
                model.matrix_index
            );
            for (g, geometry) in model.geometries.iter().enumerate() {
                match geometry {
                    Ok(geometry) => print_geometry(indent, m, g, geometry),
                    Err(e) => println!("{indent}      model {m} geometry {g}  not read: {e}"),
                }
            }
        }
    }
}

fn print_geometry(indent: &str, m: usize, g: usize, geometry: &Geometry) {
    let channels: Vec<String> = geometry
        .channels
        .iter()
        .map(|c| format!("{}:{}x{:#04X}", c.index, c.size, c.code))
        .collect();
    let positions = match &geometry.positions {
        Ok(p) => format!("{} positions read", p.len()),
        Err(e @ HeaderlessError::Positions { .. }) => format!("positions refused: {e}"),
        Err(e) => format!("positions not read: {e}"),
    };
    println!(
        "{indent}      model {m} geometry {g}  shader {}, {} vertices, {} triangles, channels [{}]; {positions}",
        geometry.shader,
        geometry.vertex_count,
        geometry.triangles.len(),
        channels.join(" ")
    );
    let v = &geometry.vertices;
    let read: Vec<String> = [
        (!v.normals.is_empty()).then(|| "normal 4".to_string()),
        (!v.tangents.is_empty()).then(|| {
            let from = match geometry.channels.iter().any(|c| c.index == 8) {
                true => 8,
                false => 4,
            };
            format!("tangent of texcoord 0 in {from}")
        }),
        (!v.reference.is_empty()).then(|| {
            let held = v.reference.iter().flatten().count();
            format!(
                "reference position 33 on {held} of {} vertices",
                v.reference.len()
            )
        }),
    ]
    .into_iter()
    .flatten()
    .chain(v.uvs.iter().map(|(c, _)| format!("texcoord {c}")))
    .chain(v.uv_tangents.iter().map(|(c, _)| format!("tangent {c}")))
    .chain(v.colours.iter().map(|(c, _)| format!("colour {c}")))
    .chain(v.weights.iter().map(|(c, _)| format!("weights {c}")))
    .chain(v.joints.iter().map(|(c, _)| format!("indices {c}")))
    .collect();
    println!("{indent}        decoded  [{}]", read.join(", "));
    if let Some(top) = v.joints.iter().flat_map(|(_, j)| j.iter().flatten()).max() {
        println!("{indent}        highest blend index {top}");
    }
    let lowest = (0..geometry.vertex_count as usize)
        .map(|i| {
            let lanes = v.weights.iter().filter_map(|(_, w)| w.get(i)).flatten();
            lanes.map(|w| (w * 255.0).round() as u32).sum::<u32>()
        })
        .min();
    if let (false, Some(lowest)) = (v.weights.is_empty(), lowest) {
        println!(
            "{indent}        weights over {} lanes, lowest sum {lowest} of 255",
            4 * v.weights.len()
        );
    }
    if let Ok(p) = &geometry.positions {
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for &i in geometry.triangles.iter().flatten() {
            for a in 0..3 {
                low[a] = low[a].min(p[i as usize][a]);
                high[a] = high[a].max(p[i as usize][a]);
            }
        }
        println!("{indent}        used vertices span {low:?} to {high:?}");
    }
    for (c, why) in &v.skipped {
        println!("{indent}        channel {c} not read: {why}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use or_io::gta5::shaders::shader_defs::{ShaderDef, ShaderParamDef, ShaderParamKind};
    use or_io::gta6::hash::name_hash32;
    use or_io::gta6::headerless::Parameter;

    fn param(name_hash: u32, semantic_hash: Option<u32>, kind: ShaderParamKind) -> ShaderParamDef {
        ShaderParamDef {
            name: format!("hash_{name_hash:08X}"),
            name_hash,
            kind,
            array_len: 1,
            register: 0,
            cbuffer: None,
            drawable_place: None,
            semantic: None,
            semantic_hash,
            ui_name: None,
            default: None,
            preset_default: None,
        }
    }

    #[test]
    fn a_shader_s_lines_join_the_effect_by_0x14_and_name_parameters_and_textures_where_known() {
        let effect = "effect_of_a_test";
        let (texture, glow, stale) = (0x1111_0001, 0x2222_0002, 0x3333_0003);
        let key = name_hash64(b"zq_shed_07_ab");
        let constant = |hash, words: Vec<u32>| Parameter {
            hash,
            value: ParameterValue::Constant {
                buffer: 1,
                offset: 8,
                words,
            },
        };
        let shader = Shader {
            name_hash: name_hash32(effect.as_bytes()),
            texture_keys: vec![Some(key), None],
            parameters: vec![
                Parameter {
                    hash: texture,
                    value: ParameterValue::Texture { slot: 0 },
                },
                constant(glow, vec![1.5f32.to_bits(), 2.0f32.to_bits()]),
                constant(stale, vec![7]),
            ],
        };
        let none = HashMap::new();
        let bare = Naming {
            effects: None,
            parameters: &none,
            textures: HashMap::new(),
        };
        let lines = shader_lines(&shader, &bare);
        assert!(
            lines[0].starts_with(&format!("hash_{:08X},", shader.name_hash)),
            "{lines:?}"
        );
        assert!(lines[0].contains("buffers [0, 16]"), "{lines:?}");
        assert!(
            lines[1].contains(&format!("hash_{texture:08X}: key {key:016X}, not named")),
            "{lines:?}"
        );
        assert!(lines[2].contains("[0x3FC00000, 0x40000000]"), "{lines:?}");

        // The effect states the glow constant's +0x14 as its semantic and another name hash; the
        // stale constant's hash is another parameter's name hash, which joins nothing.
        let mut defs = ShaderDefinitions::new("gta6", "gta6");
        defs.shaders.push(ShaderDef {
            name: effect.to_string(),
            name_hash: shader.name_hash,
            effect: effect.to_string(),
            draw_bucket: None,
            vertex_type: 0,
            traits: Vec::new(),
            parameters: vec![
                param(texture, None, ShaderParamKind::Texture),
                param(0x4444_0004, Some(glow), ShaderParamKind::Float2),
                param(stale, Some(0x5555_0005), ShaderParamKind::Float),
            ],
        });
        let gta5 = HashMap::from([(texture, "DiffuseTex".to_string())]);
        let named = Naming {
            effects: Some(&defs),
            parameters: &gta5,
            textures: name_keys_with(
                &Default::default(),
                ["zq_shed_07.#dr"],
                &HashSet::from([key]),
            ),
        };
        let lines = shader_lines(&shader, &named);
        assert!(lines[0].starts_with(effect), "{lines:?}");
        assert!(lines[0].contains("2 of 3 parameter(s) joined"), "{lines:?}");
        assert!(
            lines[1].contains("DiffuseTex (hash_11110001): key")
                && lines[1].contains("named zq_shed_07_ab by rule stem and map kind"),
            "{lines:?}"
        );
        assert!(
            lines[2].contains("[1.5, 2]; effect type float2"),
            "{lines:?}"
        );
        assert!(
            lines[3].contains("[0x00000007]; not in the effect's table"),
            "{lines:?}"
        );
    }
}
