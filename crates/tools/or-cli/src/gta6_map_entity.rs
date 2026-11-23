//! `openrage-cli rpf map-entity add|remove` over a GTA VI map body of an archive: an entity copied
//! in or removed, with its parent map's child count and every map of the archive that names it as
//! a parent renumbered; each body that changed written as a loose resource that `rpf scan-open`
//! reads at offset 0.

use std::collections::HashMap;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

use glam::{Quat, Vec3};
use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Scan, Rpf9Sections, Rpf9ZlibRun,
};
use or_io::gta6::map::grow::{
    add_entity_across, entities_union, lod_parent, remove_entity_across, set_entities_extents,
};
use or_io::gta6::map::links::{self, Links};
use or_io::gta6::map::{self, Aabb, Entity, MapBody};
use or_world::transform::{placed_corners, scale_of};

/// Archetype boxes in model space, by the word an entity names its archetype by.
type Boxes = HashMap<u64, ([f32; 3], [f32; 3])>;

const USAGE: &str = "usage: rpf map-entity add|remove <archive.rpf> <offset> <entity> <out_dir> [--orphan] [--limit <bytes>]";

pub(crate) fn cmd_map_entity(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let orphan = args.iter().any(|a| a == "--orphan");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--orphan").collect();
    let [verb, path, offset, entity, out] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let add = match verb.as_str() {
        "add" => true,
        "remove" => false,
        _ => return Err(USAGE.to_string()),
    };
    if orphan && !add {
        return Err("--orphan is the form of an added entity".to_string());
    }
    let offset = super::rpf::parse_offset(offset)?;
    let entity: usize =
        (entity.parse()).map_err(|_| format!("'{entity}' is not an entity number"))?;
    let file = File::open(path.as_str()).map_err(|e| format!("open({path}): {e}"))?;
    let mut target = body_at(&file, offset, limit)?;
    let Ok(MapBody::Data(m)) = map::open(&target.virtual_section, &target.physical) else {
        return Err(format!("the body at {offset:#X} is no CMapData that reads"));
    };
    let parent = (lod_parent(&target, entity).map_err(|e| e.to_string())?).map(|(name, _)| name);
    let (offsets, mut others, boxes) = related(&file, offset, m.name, parent, limit)?;
    let (said, changed) = match add {
        true => {
            let (at, changed) = add_entity_across(&mut target, &mut others, entity, orphan)
                .map_err(|e| e.to_string())?;
            (format!("entity {entity} copied as entity {at}"), changed)
        }
        false => {
            let changed = remove_entity_across(&mut target, &mut others, entity)
                .map_err(|e| e.to_string())?;
            let Ok(MapBody::Data(left)) = map::open(&target.virtual_section, &target.physical)
            else {
                return Err("the edited map does not read back".to_string());
            };
            let extents = match entities_union(&left, |e| placed(&boxes, e)) {
                Some(union) => {
                    set_entities_extents(&mut target, union).map_err(|e| e.to_string())?;
                    "its entitiesExtents recomputed"
                }
                None => "its entitiesExtents kept, as no map types body of the archive holds the box of every entity left",
            };
            (format!("entity {entity} removed; {extents}"), changed)
        }
    };
    // Every body is written only once each reads back as a map.
    let mut files = vec![(offset, loose(&target)?)];
    for (k, (at, body)) in offsets.iter().zip(&others).enumerate() {
        if changed.contains(&k) {
            files.push((*at, loose(body)?));
        }
    }
    std::fs::create_dir_all(out.as_str()).map_err(|e| format!("create {out}: {e}"))?;
    println!("{said}; {} other maps changed", files.len() - 1);
    for (at, bytes) in &files {
        let to = Path::new(out.as_str()).join(format!("{at:#X}.rsc"));
        std::fs::write(&to, bytes).map_err(|e| format!("write {}: {e}", to.display()))?;
        println!("wrote {} ({} bytes)", to.display(), bytes.len());
    }
    println!("This checks the maps' bytes. It does not test that GTA VI loads them.");
    Ok(())
}

fn body_at(file: &File, offset: u64, limit: u64) -> Result<Rpf9Sections, String> {
    let (find, bytes) = read_rpf9_body(file, offset, limit).map_err(|e| e.to_string())?;
    Rpf9Sections::of(&find, &bytes)
        .ok_or_else(|| format!("the body at {offset:#X} is not a resource"))
}

/// `e`'s archetype box of `boxes` scaled, turned and placed, as a map's extents hold it.
fn placed(boxes: &Boxes, e: &Entity) -> Option<Aabb> {
    let (lo, hi) = boxes.get(&e.archetype_word)?;
    let scale = scale_of(e.scale_xy, e.scale_z);
    let turn = Quat::from_array(e.model_rotation()).normalize();
    let (lo, hi) = (Vec3::from(*lo) * scale, Vec3::from(*hi) * scale);
    let corners = placed_corners(lo, hi, turn, Vec3::from(e.position));
    let (min, max) = (corners.iter()).fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(a, b), c| {
        (a.min(*c), b.max(*c))
    });
    Some(Aabb {
        min: min.to_array(),
        max: max.to_array(),
    })
}

/// Every CMapData body of the archive but the one at `skip` that names the map `name` as a parent
/// or is the map `parent`, with its offset, and the box of each archetype its map types bodies
/// hold; refused where a body does not read, as it may be one.
fn related(
    file: &File,
    skip: u64,
    name: u32,
    parent: Option<u32>,
    limit: u64,
) -> Result<(Vec<u64>, Vec<Rpf9Sections>, Boxes), String> {
    let cancelled = super::jobs::on_interrupt();
    let (mut offsets, mut bodies) = (Vec::new(), Vec::new());
    let mut boxes = Boxes::new();
    for find in Rpf9Scan::new(file, 0..u64::MAX).map_err(|e| e.to_string())? {
        if cancelled.is_set() {
            return Err("cancelled".to_string());
        }
        let find = find.map_err(|e| e.to_string())?;
        let starts = matches!(
            find.kind,
            Rpf9BodyKind::Zlib(Rpf9ZlibRun {
                resource: Some(_),
                signature: None,
                ..
            })
        );
        if !starts || find.stored_len.is_none() || find.offset == skip {
            continue;
        }
        let at = find.offset;
        let unknown = |why: String| {
            format!("the body at {at:#X} does not read, so whether it names the map is not known: {why}")
        };
        let s = body_at(file, at, limit).map_err(unknown)?;
        let root = map::root_type(&s.virtual_section);
        if root == Some(map::C_MAP_TYPES) {
            if let Ok(MapBody::Types(t)) = map::open(&s.virtual_section, &s.physical) {
                boxes.extend((t.archetypes.iter()).map(|a| (a.name_word, (a.bb_min, a.bb_max))));
            }
            continue;
        }
        if root != Some(map::C_MAP_DATA) {
            continue;
        }
        let named = match links::open(&s.virtual_section, &s.physical) {
            Ok((MapBody::Data(m), Ok(Links::Data(l)))) => {
                Some(m.name) == parent || l.lod_parents.iter().any(|p| p.name == name)
            }
            Ok((_, Err(e))) | Err(e) => return Err(unknown(e.to_string())),
            Ok(_) => false,
        };
        if named {
            offsets.push(at);
            bodies.push(s);
        }
    }
    Ok((offsets, bodies, boxes))
}

/// `s` as a loose resource, once it reads back as a map.
fn loose(s: &Rpf9Sections) -> Result<Vec<u8>, String> {
    let saved = write_rpf9_resource(s).map_err(|e| e.to_string())?;
    let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    let back = Rpf9Sections::of(&find, &bytes).ok_or("the written resource does not read back")?;
    match map::open(&back.virtual_section, &back.physical) {
        Ok(MapBody::Data(_)) => Ok(saved),
        Ok(MapBody::Types(_)) => Err("the written map reads back as map types".to_string()),
        Err(e) => Err(format!("the written map does not read back: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn an_entity_s_box_is_its_archetype_s_scaled_turned_and_placed_or_none_when_not_held() {
        let mut e = Entity {
            interior: false,
            id: 1,
            // A quarter turn about z, stored as its conjugate.
            rotation: [
                0.0,
                0.0,
                -std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            archetype_word: 7,
            position: [10.0, 0.0, 0.0],
            archetype_name: 3,
            flags: 0,
            scale_xy: 2.0,
            scale_z: 1.0,
            parent_index: -1,
            hash_e74c3c23: -1,
            lod_dist: -1.0,
            child_lod_dist: 0.0,
            num_children: 0,
            tint_value: 0,
            priority_level: 0,
            lod_level: 5,
        };
        let boxes = Boxes::from([(7, ([-1.0, -2.0, 0.0], [1.0, 2.0, 3.0]))]);
        let b = placed(&boxes, &e).expect("held");
        // Scaled to 4 m by 8 m, then turned: 8 m along x and 4 m along y.
        let near = |a: [f32; 3], w: [f32; 3]| (0..3).all(|i| (a[i] - w[i]).abs() < 1e-4);
        assert!(near(b.min, [6.0, -2.0, 0.0]), "{b:?}");
        assert!(near(b.max, [14.0, 2.0, 3.0]), "{b:?}");
        e.archetype_word = 8;
        assert_eq!(placed(&boxes, &e), None);
    }

    #[test]
    fn a_verb_entity_or_form_it_does_not_know_is_refused_before_any_file_is_read() {
        let missing = "no-such-archive.rpf";
        let refused = |args: &[&str]| cmd_map_entity(&words(args)).expect_err("refused");
        assert_eq!(refused(&["move", missing, "0", "1", "out"]), USAGE);
        assert_eq!(refused(&["add", missing, "0", "1"]), USAGE);
        let err = refused(&["remove", missing, "0", "1", "out", "--orphan"]);
        assert!(err.contains("an added entity"), "{err}");
        let err = refused(&["add", missing, "0", "x", "out"]);
        assert!(err.contains("not an entity number"), "{err}");
        let err = refused(&["add", missing, "0x10", "1", "out", "--orphan"]);
        assert!(err.contains("open(no-such-archive.rpf)"), "{err}");
    }
}
