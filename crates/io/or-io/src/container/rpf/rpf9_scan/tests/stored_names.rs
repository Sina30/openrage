//! A resource whose stored last block runs on into the names of the archive that holds it.

use super::*;

/// A nested archive whose one resource ends in a block stored raw, physical or virtual, the
/// archive's names right after that block's bytes, and the next nested archive on a later page.
fn into_names(physical: bool) -> Layout {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(2, TABLE, 2);
    let nested = l.finds[1].offset;
    if physical {
        let small = virtual_section(0x180, (1, 1));
        l.resource(
            &[(&small, false, false), (&[0x33; 0x2C3], true, true)],
            (1, 1),
        );
    } else {
        let v = virtual_section(PIECE as usize + 0x2F5, (2, 0));
        let (head, tail) = v.split_at(PIECE as usize);
        l.resource(&[(head, false, false), (tail, true, false)], (2, 0));
    }
    l.names(NAMES as usize, nested);
    l.page();
    l.archive(2, TABLE, 3);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    l
}

#[test]
fn a_stored_last_block_ends_where_the_names_it_runs_into_start() {
    for physical in [true, false] {
        let l = into_names(physical);
        assert_eq!(scan_all(&l.bytes).0, l.finds, "physical {physical}");
        assert_eq!(
            scan_in_slices(&l.bytes, 0x800).0,
            l.finds,
            "physical {physical}"
        );
    }
}
