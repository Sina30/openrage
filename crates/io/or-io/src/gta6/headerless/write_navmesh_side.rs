//! A side record's chain made again when a polygon proper it names is cut off or goes: each piece
//! naming the polygon leads nowhere, and the corners the chain no longer needs are dropped.
//!
//! ⚠ Corner 0's piece leads nowhere in every shipped record and corner 1's never does; no two
//! pieces after corner 0's lead nowhere in a row. Corner 0 is never dropped.

use super::{NavEdge, NOWHERE};

/// A record's corner: its vertex, the edge from it to the next corner, the record that edge leads
/// to, and the stored edge it was.
#[derive(Debug, Clone, Copy)]
pub(super) struct Corner {
    pub vertex: u16,
    pub edge: NavEdge,
    pub to: Option<usize>,
    pub stored: usize,
}

impl Corner {
    pub fn leads_nowhere(&self) -> bool {
        self.edge.mesh == NOWHERE
    }

    /// The edge as every shipped edge that leads nowhere: +0x06's high byte and +0x08 kept.
    pub fn cut(&mut self) {
        self.edge.mesh = NOWHERE;
        self.edge.polygon = 0xFFFF;
        self.edge.word_06 &= 0xFF00;
        self.to = None;
    }
}

/// `run`, a side record's corners, with each piece after corner 0's that leads to `polygon` cut and
/// the corners its chain then drops; whether a piece still names a polygon.
pub(super) fn rechained(run: &mut Vec<Corner>, polygon: usize) -> bool {
    for c in run.iter_mut().skip(1).filter(|c| c.to == Some(polygon)) {
        c.cut();
    }
    // The next corner takes corner 1's place, and the span's end moves with it.
    while run.len() > 2 && run[1].leads_nowhere() {
        run.remove(1);
    }
    let mut k = 2;
    while k < run.len() {
        if run[k - 1].leads_nowhere() && run[k].leads_nowhere() {
            run.remove(k);
        } else {
            k += 1;
        }
    }
    run.iter().skip(1).any(|c| !c.leads_nowhere())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: u32 = 7;

    /// Corners of vertices 10, 11, … whose pieces after corner 0's lead to `to`, `None` nowhere.
    fn run(to: &[Option<usize>]) -> Vec<Corner> {
        let nowhere = NavEdge {
            mesh: NOWHERE,
            polygon: 0xFFFF,
            word_06: 0x3300,
            flags: 0x18,
        };
        let mut out = vec![Corner {
            vertex: 10,
            edge: nowhere,
            to: None,
            stored: 0,
        }];
        for (k, t) in to.iter().enumerate() {
            let edge = match t {
                Some(t) => NavEdge {
                    mesh: CELL,
                    polygon: *t as u16,
                    word_06: 0x3307,
                    flags: 0x10,
                },
                None => nowhere,
            };
            out.push(Corner {
                vertex: 11 + k as u16,
                edge,
                to: *t,
                stored: k + 1,
            });
        }
        out
    }

    fn vertices(run: &[Corner]) -> Vec<u16> {
        run.iter().map(|c| c.vertex).collect()
    }

    #[test]
    fn a_cut_piece_between_named_ones_leads_nowhere_with_its_high_byte_and_flags_kept() {
        let mut r = run(&[Some(1), Some(2), Some(3)]);
        assert!(rechained(&mut r, 2));
        assert_eq!(vertices(&r), [10, 11, 12, 13]);
        assert_eq!(
            r[2].edge,
            NavEdge {
                mesh: NOWHERE,
                polygon: 0xFFFF,
                word_06: 0x3300,
                flags: 0x10,
            }
        );
        assert_eq!(r[3].to, Some(3));
    }

    #[test]
    fn corner_one_s_piece_cut_drops_corner_one_until_a_named_piece_starts_there() {
        // The piece after it leads nowhere already, so its corner goes too.
        let mut r = run(&[Some(2), None, Some(4)]);
        assert!(rechained(&mut r, 2));
        assert_eq!(vertices(&r), [10, 13]);
        assert_eq!((r[1].to, r[1].stored), (Some(4), 3));
        assert_eq!(r[0].stored, 0, "corner 0's piece kept as read");
    }

    #[test]
    fn two_pieces_in_a_row_that_lead_nowhere_lose_the_corner_between_them() {
        let mut r = run(&[Some(1), None, Some(2), Some(3)]);
        assert!(rechained(&mut r, 2));
        assert_eq!(vertices(&r), [10, 11, 12, 14]);
        let nowhere: Vec<bool> = r.iter().map(Corner::leads_nowhere).collect();
        assert_eq!(nowhere, [true, false, true, false]);
    }

    #[test]
    fn a_record_whose_every_piece_named_the_polygon_names_none() {
        let mut r = run(&[Some(2), Some(2)]);
        assert!(!rechained(&mut r, 2));
        assert_eq!(vertices(&r), [10, 12]);
        let mut two = run(&[Some(2)]);
        assert!(!rechained(&mut two, 2));
        assert_eq!(two.len(), 2, "a record keeps two corners at least");
        let mut kept = run(&[Some(1), Some(3)]);
        assert!(rechained(&mut kept, 2));
        assert_eq!(vertices(&kept), [10, 11, 12], "nothing named it");
    }
}
