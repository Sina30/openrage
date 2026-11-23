//! `openrage-cli rpf map-entity add|remove` over a GTA VI map body of an archive: an entity copied
//! in or removed, with its parent map's child count and every map of the archive that names it as
//! a parent renumbered; each body that changed written as a loose resource that `rpf scan-open`
//! reads at offset 0.

use std::fs::File;
use std::io::Cursor;
use std::path::Path;

use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Scan, Rpf9Sections, Rpf9ZlibRun,
};
use or_io::gta6::map::grow::{add_entity_across, lod_parent, remove_entity_across};
use or_io::gta6::map::links::{self, Links};
use or_io::gta6::map::{self, MapBody};

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
    let (offsets, mut others) = related(&file, offset, m.name, parent, limit)?;
    let (said, changed) = match add {
        true => {
            let (at, changed) = add_entity_across(&mut target, &mut others, entity, orphan)
                .map_err(|e| e.to_string())?;
            (format!("entity {entity} copied as entity {at}"), changed)
        }
        false => {
            let changed = remove_entity_across(&mut target, &mut others, entity)
                .map_err(|e| e.to_string())?;
            (format!("entity {entity} removed"), changed)
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

/// Every CMapData body of the archive but the one at `skip` that names the map `name` as a parent
/// or is the map `parent`, with its offset; refused where a body does not read, as it may be one.
fn related(
    file: &File,
    skip: u64,
    name: u32,
    parent: Option<u32>,
    limit: u64,
) -> Result<(Vec<u64>, Vec<Rpf9Sections>), String> {
    let cancelled = super::jobs::on_interrupt();
    let (mut offsets, mut bodies) = (Vec::new(), Vec::new());
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
        if map::root_type(&s.virtual_section) != Some(map::C_MAP_DATA) {
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
    Ok((offsets, bodies))
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
