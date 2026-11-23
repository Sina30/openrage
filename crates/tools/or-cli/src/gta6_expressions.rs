//! `rpf scan-expressions`: a GTA VI expression set's tracks, group by group, and a track's id
//! written in place and saved as a loose resource that `rpf scan-open` reads again.

use std::fs;

use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Sections, Rpf9ZlibRun,
};
use or_io::gta6::anim::{
    read_expressions, track_label, write_expression_ids, ExpressionGroup, Expressions,
    EXPRESSIONS_SIGNATURE,
};

use crate::project::Failure::{self, Run, Usage};

const USAGE: &str = "usage: rpf scan-expressions <archive.rpf> <offset> [--limit <bytes>] \
     [--id <group> <track> <hash> --out <file>]   (group: inputs, bones, controls, facial-outputs \
     or outputs)";

/// The group a word names, as the usage spells it.
fn group_named(word: &str) -> Option<ExpressionGroup> {
    (ExpressionGroup::ALL.into_iter()).find(|g| group_word(*g) == word)
}

fn group_word(group: ExpressionGroup) -> &'static str {
    match group {
        ExpressionGroup::Inputs => "inputs",
        ExpressionGroup::Bones => "bones",
        ExpressionGroup::Controls => "controls",
        ExpressionGroup::FacialOutputs => "facial-outputs",
        ExpressionGroup::Outputs => "outputs",
    }
}

/// A hash typed as `hash_` and hex digits, `0x` and hex digits, or a decimal number.
fn parse_hash(word: &str) -> Option<u32> {
    let hex = (word.strip_prefix("hash_")).or_else(|| word.strip_prefix("0x"));
    match hex {
        Some(digits) => u32::from_str_radix(digits, 16).ok(),
        None => word.parse().ok(),
    }
}

pub(crate) fn cmd_scan_expressions(args: &[String]) -> Result<(), Failure> {
    let (limit, args) = super::rpf::split_limit(args).map_err(Usage)?;
    let usage = || Usage(USAGE.to_string());
    let (path, offset, edit) = match args.as_slice() {
        [path, offset] => (path, offset, None),
        [path, offset, id, group, track, hash, out_flag, out]
            if id == "--id" && out_flag == "--out" =>
        {
            let group = group_named(group).ok_or_else(usage)?;
            let track: usize = track.parse().map_err(|_| usage())?;
            let hash = parse_hash(hash).ok_or_else(usage)?;
            (path, offset, Some((group, track, hash, out)))
        }
        _ => return Err(usage()),
    };
    let offset = super::rpf::parse_offset(offset).map_err(Usage)?;
    if let Some((.., out)) = edit.filter(|(.., out)| super::navmesh_edit::same_file(path, out)) {
        return Err(Usage(format!(
            "the output {out} is the input: name another file"
        )));
    }
    let file = fs::File::open(path).map_err(|e| Run(format!("open({path}): {e}")))?;
    let (find, bytes) = read_rpf9_body(file, offset, limit).map_err(|e| Run(e.to_string()))?;
    let signature = match find.kind {
        Rpf9BodyKind::Zlib(Rpf9ZlibRun { signature, .. }) => signature,
        _ => None,
    };
    if !signature.is_some_and(|s| s.to_string() == EXPRESSIONS_SIGNATURE) {
        return Err(Run(format!(
            "the body at {offset:#X} is no expression set: its signature is not {EXPRESSIONS_SIGNATURE}"
        )));
    }
    let mut sections = Rpf9Sections::of(&find, &bytes)
        .ok_or_else(|| Run(format!("the body at {offset:#X} states no sections")))?;
    let mut set = read_expressions(&sections.virtual_section)
        .map_err(|e| Run(format!("the expression reader does not read it: {e}")))?;
    print_tables(&set);
    let Some((group, track, hash, out)) = edit else {
        return Ok(());
    };
    let held = (group.of_mut(&mut set))
        .and_then(|g| g.tracks.get_mut(track))
        .ok_or_else(|| {
            Run(format!(
                "the set holds no track {track} among its {}",
                group_word(group)
            ))
        })?;
    held.0 = hash;
    write_expression_ids(&mut sections.virtual_section, &set).map_err(|e| Run(e.to_string()))?;
    let written = write_rpf9_resource(&sections).map_err(|e| Run(e.to_string()))?;
    fs::write(out, &written).map_err(|e| Run(format!("write({out}): {e}")))?;
    println!(
        "  wrote {} bytes to {out}: track {track} of the {} now names hash_{hash:08X}, written where it lies; `rpf scan-open {out} 0` reads it again",
        written.len(),
        group_word(group)
    );
    Ok(())
}

/// Each group's tracks, a line each: its index, id, track word, components or byte or mask, and
/// slots.
fn print_tables(set: &Expressions) {
    println!(
        "  expressions   key {:#010X}, whose name is not known; it holds no program",
        set.key
    );
    for group in ExpressionGroup::ALL {
        let Some(tracks) = group.of(set) else {
            println!("  {:<15} none", group_word(group));
            continue;
        };
        let side = if group.reads() { "reads" } else { "writes" };
        println!(
            "  {:<15} {} track(s) it {side}",
            group_word(group),
            tracks.tracks.len()
        );
        for (k, (id, word)) in tracks.tracks.iter().enumerate() {
            let stated = match group.mask(set, k) {
                Some(mask) => format!("mask {mask:#04X}"),
                None => format!(
                    "component(s) {}",
                    tracks.components.get(k).copied().unwrap_or_default()
                ),
            };
            let slots: Vec<String> = (group.slots(set, k).unwrap_or_default().iter())
                .map(u16::to_string)
                .collect();
            println!(
                "    {k:>4} hash_{id:08X} {:<18} {stated}, slot(s) {}",
                track_label(*word),
                slots.join(", ")
            );
        }
    }
    if !set.control_slots.is_empty() {
        println!(
            "  further input slots of the unnamed word: {:?}, their words {:08X?}",
            set.control_slots, set.control_words
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_and_a_hash_are_read_as_the_usage_spells_them() {
        for group in ExpressionGroup::ALL {
            assert_eq!(group_named(group_word(group)), Some(group));
        }
        assert_eq!(group_named("faces"), None);
        assert_eq!(parse_hash("hash_0BADF00D"), Some(0x0BAD_F00D));
        assert_eq!(parse_hash("0x10"), Some(16));
        assert_eq!(parse_hash("17"), Some(17));
        assert_eq!(parse_hash("hash_xyz"), None);
    }
}
