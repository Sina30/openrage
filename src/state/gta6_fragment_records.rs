//! What the inspector says of the records a GTA VI fragment's root points at besides its physics
//! level: each record's content, or why it is not read.

use or_io::gta6::headerless::Fragment;

/// A line for each record of `f` that was read or refused, and one for its wheels.
pub(super) fn record_lines(f: &Fragment, lines: &mut Vec<String>) {
    if let Some(glass) = &f.vehicle_glass {
        lines.push(match glass {
            Ok(g) => tr_n!(
                "{count} vehicle glass window at 0xF0: a child of the fragment with its shattermap.",
                "{count} vehicle glass windows at 0xF0: each a child of the fragment with its shattermap.",
                g.windows.len()
            ),
            Err(e) => tr!("Its vehicle glass at 0xF0 is not read: {why}", why = e),
        });
    }
    match &f.panes {
        Some(Ok(panes)) => {
            lines.push(tr_n!(
                "{count} glass pane at 0x98, outlined at its owner group's bone while Collision is on.",
                "{count} glass panes at 0x98, each outlined at its owner group's bone while Collision is on.",
                panes.len()
            ));
            let unread = panes.iter().filter(|p| p.owner.is_none()).count();
            if unread > 0 {
                lines.push(tr_n!(
                    "{count} glass pane shares its owner word with its outlines' data, so its group is not read and it is not drawn.",
                    "{count} glass panes share their owner word with their outlines' data, so their groups are not read and they are not drawn.",
                    unread
                ));
            }
        }
        Some(Err(e)) => lines.push(tr!("Its glass panes at 0x98 are not read: {why}", why = e)),
        None => {}
    }
    if !f.wheels.is_empty() {
        lines.push(tr_n!(
            "{count} wheel: a group on a wheel bone, drawn by the drawable's geometry skinned to that bone, its collider its first child's bound.",
            "{count} wheels: groups on wheel bones, each drawn by the drawable's geometry skinned to its bone, its collider its first child's bound.",
            f.wheels.len()
        ));
    }
    if let Some(pairs) = &f.self_collision {
        lines.push(match pairs {
            Ok(p) => tr_n!(
                "{count} pair of groups at 0x58, as GTA V's self-collision pairs.",
                "{count} pairs of groups at 0x58, as GTA V's self-collision pairs.",
                p.len()
            ),
            Err(e) => tr!("Its group pairs at 0x58 are not read: {why}", why = e),
        });
    }
    if let Some(links) = &f.group_links {
        lines.push(match links {
            Ok(l) => tr_n!(
                "{count} link of two groups at 0x50, with a value whose meaning is not known.",
                "{count} links of two groups at 0x50, each with a value whose meaning is not known.",
                l.links.len()
            ),
            Err(e) => tr!("Its group links at 0x50 are not read: {why}", why = e),
        });
    }
    if let Some(lattice) = &f.lattice {
        lines.push(match lattice {
            Ok(l) => tr!(
                "Its lattice at 0xA0: {cells} hexahedral cells over {vertices} vertices, each boxed by its corners and naming the cells across its faces; what it deforms and its other arrays are not known.",
                cells = l.cells.len(),
                vertices = l.vertices.len(),
            ),
            Err(e) => tr!("Its lattice at 0xA0 is not read: {why}", why = e),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::headerless::{GlassPane, GroupLink, GroupLinks, HeaderlessError, Wheel};

    #[test]
    fn a_fragment_s_records_each_say_what_they_hold_or_why_they_are_not_read() {
        let pane = |owner| GlassPane {
            base: [0.0; 3],
            width: [1.0, 0.0, 0.0],
            height: [0.0, 0.0, 1.0],
            uv_min: [0.0; 2],
            uv_max: [1.0; 2],
            outlines: vec![4],
            owner,
            corners: None,
        };
        let link = GroupLink {
            groups: [0, 1],
            value: 0.5,
        };
        let fragment = Fragment {
            panes: Some(Ok(vec![pane(Some(1)), pane(None), pane(Some(2))])),
            self_collision: Some(Err(HeaderlessError::Fragment("a pair repeats"))),
            group_links: Some(Ok(GroupLinks {
                links: vec![link; 3],
                capacity: 5,
            })),
            wheels: vec![Wheel { group: 4, child: 7 }; 2],
            ..Fragment::default()
        };
        let mut lines = Vec::new();
        record_lines(&fragment, &mut lines);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(lines[0].starts_with("3 glass panes at 0x98"), "{lines:?}");
        assert!(lines[1].starts_with("1 glass pane shares"), "{lines:?}");
        assert!(lines[2].starts_with("2 wheels"), "{lines:?}");
        assert!(lines[3].ends_with("a pair repeats"), "{lines:?}");
        assert!(lines[4].starts_with("3 links of two groups"), "{lines:?}");
    }
}
