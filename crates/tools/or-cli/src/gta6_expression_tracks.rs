//! `rpf scan-expression-tracks`: a scanned GTA VI expression set laid out anew unchanged, and for
//! each group that takes a track, a copy of its first track added and removed again, in memory;
//! whether the virtual section is the stored one again.

use or_io::container::rpf::{Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta6::anim::{
    add_expression_track, lay_out_expressions, offered_facial_outputs, read_expressions,
    remove_expression_track, write_edited_expressions, ExpressionGroup, Expressions,
    EXPRESSIONS_SIGNATURE,
};

use super::gta6_inverse::{first_difference, said, sections, verdict, Outcome, BYTES};

const USAGE: &str = "usage: rpf scan-expression-tracks <archive.rpf> <offset> [--limit <bytes>]";

pub(crate) fn cmd_scan_expression_tracks(args: &[String]) -> Result<(), String> {
    let (find, body) = sections(args, USAGE)?;
    let signature = match find.kind {
        Rpf9BodyKind::Zlib(Rpf9ZlibRun { signature, .. }) => signature,
        _ => None,
    };
    if !signature.is_some_and(|s| s.to_string() == EXPRESSIONS_SIGNATURE) {
        return Err(format!(
            "the body is no expression set: its signature is not {EXPRESSIONS_SIGNATURE}"
        ));
    }
    let stored = &body.virtual_section;
    let physical_len = body.physical.len();
    let set = read_expressions(stored).map_err(|e| format!("the set does not read: {e}"))?;
    let relaid = match lay_out_expressions(&set, physical_len) {
        Ok(out) if out == *stored => Outcome::Same(BYTES),
        Ok(out) => Outcome::Differs(first_difference(&out, stored)),
        Err(e) => Outcome::Refused(e.to_string()),
    };
    let mut held = said("the set laid out anew unchanged", relaid);
    for group in ExpressionGroup::ALL
        .into_iter()
        .filter(|g| g.takes_tracks())
    {
        let Some(tracks) = group.of(&set).filter(|t| !t.tracks.is_empty()) else {
            continue;
        };
        // Group D takes only a facial control group A reads and D does not write.
        let id = match group {
            ExpressionGroup::FacialOutputs => match offered_facial_outputs(&set).first() {
                Some(id) => *id,
                None => continue,
            },
            _ => fresh_id(&tracks.tracks),
        };
        let what = format!("a copy of track 0 of {group:?} added as {id:#010X}, then removed");
        held &= said(
            &what,
            added_and_removed(stored, physical_len, &set, group, id),
        );
    }
    verdict(held)
}

/// An id no track of `tracks` states, the first past the first track's.
fn fresh_id(tracks: &[(u32, u32)]) -> u32 {
    let mut id = tracks.first().map_or(0, |t| t.0).wrapping_add(1);
    // At most `tracks.len()` ids are taken, so this ends within that many more steps.
    while tracks.iter().any(|t| t.0 == id) {
        id = id.wrapping_add(1);
    }
    id
}

/// A copy of `group`'s track 0 under `id` written into `stored`, then that copy removed and
/// written again.
fn added_and_removed(
    stored: &[u8],
    physical_len: usize,
    set: &Expressions,
    group: ExpressionGroup,
    id: u32,
) -> Outcome {
    let added = match add_expression_track(set, group, 0, id) {
        Ok(added) => added,
        Err(e) => return Outcome::Refused(e.to_string()),
    };
    let mut work = stored.to_vec();
    if let Err(e) = write_edited_expressions(&mut work, physical_len, &added) {
        return Outcome::Refused(e.to_string());
    }
    let again = match read_expressions(&work) {
        Ok(again) => again,
        Err(e) => return Outcome::Differs(format!("the set with the copy does not read: {e}")),
    };
    // Group D puts the copy in group A's order, so it is found by what it states.
    let before = group.of(set).map_or(&[][..], |t| t.tracks.as_slice());
    let last = (group.of(&again))
        .and_then(|t| (t.tracks.iter()).rposition(|t| t.0 == id && !before.contains(t)))
        .unwrap_or(0);
    let removed = match remove_expression_track(&again, group, last) {
        Ok(removed) => removed,
        Err(e) => return Outcome::Differs(format!("the copy's removal is refused: {e}")),
    };
    if let Err(e) = write_edited_expressions(&mut work, physical_len, &removed) {
        return Outcome::Differs(format!("the set without the copy is not written: {e}"));
    }
    match work == stored {
        true => Outcome::Same(BYTES),
        false => Outcome::Differs(first_difference(&work, stored)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_id_is_none_the_tracks_state() {
        let tracks = [(7, 1), (8, 1), (10, 2)];
        assert_eq!(fresh_id(&tracks), 9);
        assert_eq!(fresh_id(&[(u32::MAX, 5)]), 0);
        assert_eq!(fresh_id(&[]), 1);
    }
}
