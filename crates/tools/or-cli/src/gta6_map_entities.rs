//! `rpf scan-map-entities`: a scanned GTA VI map's entity copied in as an orphan, then removed, and
//! a level-of-detail parent's child count raised and lowered, in memory; whether the map reads, and
//! its sections are, as stored again.

use or_io::container::rpf::Rpf9Sections;
use or_io::gta6::map::grow::{add_entity, child_count_changed, remove_entity};
use or_io::gta6::map::{self, MapBody, MapData};

use super::gta6_inverse::{compared, said, sections, verdict, Outcome, READ};

const USAGE: &str = "usage: rpf scan-map-entities <archive.rpf> <offset> [--limit <bytes>]";

pub(crate) fn cmd_scan_map_entities(args: &[String]) -> Result<(), String> {
    let (_, body) = sections(args, USAGE)?;
    let stored = match map::open(&body.virtual_section, &body.physical) {
        Ok(MapBody::Data(m)) => m,
        Ok(MapBody::Types(_)) => {
            return Err("the body is a CMapTypes, which places no entity".to_string())
        }
        Err(e) => return Err(format!("the map does not read: {e}")),
    };
    let (what, outcome) = copied_and_removed(&body, &stored);
    let mut held = said(&what, outcome);
    let (what, outcome) = counted_up_and_down(&body, &stored);
    held &= said(&what, outcome);
    verdict(held)
}

/// The first entity that is no interior copied in as an orphan, then the copy removed.
fn copied_and_removed(body: &Rpf9Sections, stored: &MapData) -> (String, Outcome) {
    let Some(from) = stored.entities.iter().position(|e| !e.interior) else {
        let none = Outcome::Refused("every entity places an interior".to_string());
        return ("an entity copied in".to_string(), none);
    };
    let what = format!("entity {from} copied in as an orphan, then the copy removed");
    let mut work = body.clone();
    let at = match add_entity(&mut work, from, true) {
        Ok(at) => at,
        Err(e) => return (what, Outcome::Refused(e.to_string())),
    };
    if let Err(e) = remove_entity(&mut work, at) {
        return (
            what,
            Outcome::Differs(format!("the copy's removal is refused: {e}")),
        );
    }
    let outcome = match map::open(&work.virtual_section, &work.physical) {
        Ok(MapBody::Data(again)) if again == *stored => Outcome::Same(READ),
        Ok(_) => Outcome::Differs("the map reads otherwise than stored".to_string()),
        Err(e) => Outcome::Differs(format!("the map does not read: {e}")),
    };
    (what, outcome)
}

/// The first entity with children given one more, then one fewer; written in place.
fn counted_up_and_down(body: &Rpf9Sections, stored: &MapData) -> (String, Outcome) {
    let parent = (stored.entities.iter().position(|e| e.num_children > 0))
        .and_then(|k| i16::try_from(k).ok());
    let Some(parent) = parent else {
        let none = Outcome::Refused("no entity of the map has children".to_string());
        return ("a parent's child count changed".to_string(), none);
    };
    let what = format!("entity {parent}'s child count raised by one, then lowered by one");
    let mut work = body.clone();
    if let Err(e) = child_count_changed(&mut work, parent, 1) {
        return (what, Outcome::Refused(e.to_string()));
    }
    if let Err(e) = child_count_changed(&mut work, parent, -1) {
        return (
            what,
            Outcome::Differs(format!("the count's return is refused: {e}")),
        );
    }
    (what, compared(&work, body))
}
