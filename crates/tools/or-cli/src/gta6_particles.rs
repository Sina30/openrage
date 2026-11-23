//! `rpf scan-particles`: a scanned GTA VI particle effects list's rules, a line each, and with
//! `--validate` whether it and its GPU buffers read and its in-place writer gives its section
//! back unchanged.

use std::fs;

use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9Role, Rpf9Sections};
use or_io::gta6::headerless::{
    class_of_bit, read_particle_effects, read_particle_gpu_buffers, write_particle_effects,
    HeaderlessError, ParticleEffects,
};

const USAGE: &str =
    "usage: rpf scan-particles <archive.rpf> <offset> [--validate] [--limit <bytes>]";

pub(crate) fn cmd_scan_particles(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let validate = args.iter().any(|a| a == "--validate");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--validate").collect();
    let [path, offset] = args.as_slice() else {
        return Err(USAGE.to_string());
    };
    let offset = super::rpf::parse_offset(offset)?;
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    let sections = Rpf9Sections::of(&find, &bytes)
        .ok_or_else(|| format!("the body at {offset:#X} is not a resource"))?;
    let role = match find.kind {
        Rpf9BodyKind::Zlib(run) => run.signature.and_then(|s| s.role()),
        _ => None,
    };
    if role.is_some_and(|r| r != Rpf9Role::ParticleEffects) {
        return Err(format!(
            "the body at {offset:#X} is not a particle effects list"
        ));
    }
    let physical = Some(sections.physical.as_slice());
    let read = read_particle_effects(&sections.virtual_section, physical);
    if validate {
        return check(read, sections.virtual_section, physical);
    }
    print_rules(
        &read.map_err(|e| e.to_string())?,
        &sections.virtual_section,
        physical,
    );
    Ok(())
}

fn print_rules(list: &ParticleEffects, virtual_section: &[u8], physical: Option<&[u8]>) {
    println!(
        "  particles     list {}: {} effect rules, {} emitter rules, {} particle rules, {} behaviours",
        list.name,
        list.effects.len(),
        list.emitters.len(),
        list.particles.len(),
        list.behaviours.len()
    );
    for e in &list.effects {
        println!(
            "  effect        {}  {} event(s), loops {}, duration {} to {}",
            e.name,
            e.events.len(),
            e.num_loops,
            e.times[2],
            e.times[3]
        );
        for ev in &e.events {
            let emitter = list.emitters.get(ev.emitter).map(|r| r.name.as_str());
            let particle = list.particles.get(ev.particle).map(|r| r.name.as_str());
            println!(
                "    event       {} to {}  emitter {}, particle {}",
                ev.start_ratio,
                ev.end_ratio,
                emitter.unwrap_or("not a rule"),
                particle.unwrap_or("not a rule")
            );
        }
    }
    for e in &list.emitters {
        let domains: Vec<String> = (e.domains.iter())
            .map(|d| match d {
                None => "none".to_string(),
                Some(d) => match (d.shape, &d.model) {
                    (4, Some((model, _))) => format!("mesh {model:?}"),
                    (0, _) => "box".to_string(),
                    (1, _) => "sphere".to_string(),
                    (2, _) => "cylinder".to_string(),
                    (3, _) => "attractor".to_string(),
                    (shape, _) => format!("shape {shape}"),
                },
            })
            .collect();
        println!(
            "  emitter       {}  creation, target, attractor: {}; flags {:#010X}",
            e.name,
            domains.join(", "),
            e.flags
        );
    }
    for p in &list.particles {
        println!(
            "  particle      {}  draw type {}, sort type {}, {} behaviour(s), {} drawable(s)",
            p.name,
            p.draw_type,
            p.sort_type,
            p.behaviours.len(),
            p.drawables.len()
        );
        for b in p.behaviours.iter().filter_map(|&b| list.behaviours.get(b)) {
            let by_bit = (b.class_bit.and_then(class_of_bit)).and_then(|(class, _)| class);
            println!(
                "    behaviour   {}  class bit {}, {} bytes, {} keyframed value(s)",
                b.class.or(by_bit).unwrap_or("class not known"),
                b.class_bit
                    .map_or("not known".to_string(), |bit| bit.to_string()),
                b.size,
                b.kfps.len()
            );
        }
        for d in &p.drawables {
            println!("    drawable    {}", d.name);
        }
        match read_particle_gpu_buffers(virtual_section, physical, p) {
            Ok(buffers) => {
                for b in buffers {
                    println!(
                        "    gpu buffer  +{:#X}  {} bytes at physical {:#X}, words {} and {}, bind flags {:#X}; what its data holds is not read",
                        b.slot,
                        b.len,
                        b.data_at,
                        b.words.0,
                        b.words.1,
                        b.words.2
                    );
                }
            }
            Err(e) => println!("    gpu buffer  refused: {e}"),
        }
    }
}

/// Whether the list and its GPU buffers read, and whether its values written back in place leave its section as it is.
fn check(
    read: Result<ParticleEffects, HeaderlessError>,
    mut virtual_section: Vec<u8>,
    physical: Option<&[u8]>,
) -> Result<(), String> {
    let list = match read {
        Ok(list) => list,
        Err(e) => {
            println!("  FAIL read: {e}");
            return Err("the list does not read".to_string());
        }
    };
    println!(
        "  ok   read: {} effect rules, {} emitter rules, {} particle rules, {} behaviours",
        list.effects.len(),
        list.emitters.len(),
        list.particles.len(),
        list.behaviours.len()
    );
    let mut buffers = 0;
    for p in &list.particles {
        match read_particle_gpu_buffers(&virtual_section, physical, p) {
            Ok(read) => buffers += read.len(),
            Err(e) => {
                println!("  FAIL GPU buffers of {}: {e}", p.name);
                return Err("a check failed".to_string());
            }
        }
    }
    println!("  ok   {buffers} GPU buffers, each of the words measured at its slot, with its data in the physical section");
    let before = virtual_section.clone();
    match write_particle_effects(&mut virtual_section, &list) {
        Ok(()) if virtual_section == before => {
            println!("  ok   written back in place and read again: the section is unchanged")
        }
        Ok(()) => {
            println!("  FAIL written back in place: the section changed");
            return Err("a check failed".to_string());
        }
        Err(e) => {
            println!("  FAIL written back in place: {e}");
            return Err("a check failed".to_string());
        }
    }
    println!("This checks the list's bytes. It does not test that GTA VI loads them.");
    Ok(())
}
