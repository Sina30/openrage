//! `rpf scan-fragment-regroup`: a scanned GTA VI fragment's child given another group, written,
//! read again and given its own group back at its own place, in memory; whether the virtual
//! section is the stored one again.

use or_io::gta6::headerless::{
    fragment_child_place, read_drawable_with_bound, regroup_fragment_child, write_fragment_level,
};

use super::gta6_inverse::{first_difference, said, sections, verdict, Outcome, BYTES};

const USAGE: &str = "usage: rpf scan-fragment-regroup <archive.rpf> <offset> [--limit <bytes>]";

pub(crate) fn cmd_scan_fragment_regroup(args: &[String]) -> Result<(), String> {
    let (_, body) = sections(args, USAGE)?;
    let (stored, physical) = (&body.virtual_section, &body.physical[..]);
    let mut level = read_drawable_with_bound(stored, Some(physical))
        .map_err(|e| format!("the fragment does not read: {e}"))?;
    let (children, groups) = (level.fragment.children.len(), level.fragment.groups.len());
    // The first child and other group the value edit takes, the child keeping its own transform.
    let mut moved = None;
    'search: for child in 0..children {
        let c = &level.fragment.children[child];
        let (from, q, t) = (c.group, c.rotation, c.translation);
        let Some(place) = fragment_child_place(&level.fragment, child) else {
            continue;
        };
        for group in (0..groups).filter_map(|g| u16::try_from(g).ok()) {
            if group == from {
                continue;
            }
            if let Ok(now) = regroup_fragment_child(&mut level, child, group, 0, q, t) {
                moved = Some((child, from, group, place, now, q, t));
                break 'search;
            }
        }
    }
    let Some((child, from, group, place, now, q, t)) = moved else {
        let none = Outcome::Refused("no child of the fragment takes another group".to_string());
        said("a child given another group, then its own", none);
        return verdict(true);
    };
    let what = format!(
        "child {child} given group {group}, then group {from} again at place {place} of its set"
    );
    let mut work = stored.clone();
    let outcome = 'check: {
        if let Err(e) = write_fragment_level(&mut work, physical, &level) {
            break 'check Outcome::Refused(e.to_string());
        }
        let mut again = match read_drawable_with_bound(&work, Some(physical)) {
            Ok(again) => again,
            Err(e) => break 'check Outcome::Differs(format!("it does not read regrouped: {e}")),
        };
        if let Err(e) = regroup_fragment_child(&mut again, now, from, place, q, t) {
            break 'check Outcome::Differs(format!("the way back is refused: {e}"));
        }
        if let Err(e) = write_fragment_level(&mut work, physical, &again) {
            break 'check Outcome::Differs(format!("the way back is not written: {e}"));
        }
        match work == *stored {
            true => Outcome::Same(BYTES),
            false => Outcome::Differs(first_difference(&work, stored)),
        }
    };
    verdict(said(&what, outcome))
}
