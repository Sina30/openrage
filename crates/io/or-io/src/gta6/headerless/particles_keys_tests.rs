//! Keys added to a list's keyframed values, each value's keys written past the section's end.

use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

/// The body as block-mapped sections whose map counts the one virtual block it is cut into.
fn mapped(mut v: Vec<u8>) -> Rpf9Sections {
    let map = (u64_at(&v, 8) & 0x0FFF_FFFF) as usize;
    v[map + 8] = 1;
    Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: v,
        physical: Vec::new(),
    }
}

#[test]
fn a_key_added_takes_an_array_past_the_end_and_the_keys_after_it_their_steps_again() {
    let (v, at) = body();
    let mut s = mapped(v);
    let before = s.virtual_section.clone();
    let old = before.len();
    let mut list = read_list(&before).expect("it reads");
    let middle = KfpEntry {
        time: [0.5, 0.0, 0.0, 0.0],
        value: [15.0, 20.0, 0.0, 0.0],
    };
    list.emitters[0].kfps[0].entries.insert(1, middle);
    add_particle_keys(&mut s, &list).expect("it adds");

    let place = at.emitter + 0x48;
    let to = (u64_at(&s.virtual_section, place + 8) & 0x0FFF_FFFF) as usize;
    assert!(to >= old, "the keys lie past the old end");
    // Only the value's pointer and counts changed in what was there; its old keys stay unread.
    let mut kept = s.virtual_section[..old].to_vec();
    kept[place + 8..place + 0x14].copy_from_slice(&before[place + 8..place + 0x14]);
    assert_eq!(kept, before);

    let back = read_list(&s.virtual_section).expect("it reads again");
    let keys = &back.emitters[0].kfps[0].entries;
    let times: Vec<[f32; 2]> = keys.iter().map(|e| [e.time[0], e.time[1]]).collect();
    assert_eq!(
        times,
        [[0.0, 0.0], [0.5, 1.0 / 0.5], [1.0, 1.0 / (1.0 - 0.5)]]
    );
    assert_eq!(keys[1].value, middle.value);

    // The values are then written in place over the grown list, as the save does.
    let mut again = back.clone();
    again.emitters[0].kfps[0].entries[1].value[0] = 16.0;
    write_particle_effects(&mut s.virtual_section, &again).expect("it writes");
    let written = read_list(&s.virtual_section).expect("it reads");
    assert_eq!(written.emitters[0].kfps[0].entries[1].value[0], 16.0);
}

#[test]
fn a_value_of_no_keys_takes_its_first_and_a_list_with_no_key_added_is_left_as_it_was() {
    let (v, _) = body();
    let mut s = mapped(v);
    let mut list = read_list(&s.virtual_section).expect("it reads");
    let before = s.clone();
    add_particle_keys(&mut s, &list).expect("nothing to add");
    assert_eq!(s, before);

    let first = KfpEntry {
        time: [0.0; 4],
        value: [4.0, 0.0, 0.0, 0.0],
    };
    list.emitters[0].kfps[1].entries.push(first);
    let size = (list
        .behaviours
        .iter()
        .position(|b| b.class == Some("ptxu_Size")))
    .expect("a ptxu_Size");
    // Its maximum, which holds no key.
    list.behaviours[size].kfps[1].entries.push(first);
    add_particle_keys(&mut s, &list).expect("it adds");
    let back = read_list(&s.virtual_section).expect("it reads again");
    assert_eq!(back.emitters[0].kfps[1].entries, [first]);
    assert_eq!(back.behaviours[size].kfps[1].entries, [first]);
}

#[test]
fn keys_are_not_added_to_a_body_with_no_block_map_and_nothing_changes() {
    let (v, _) = body();
    let mut s = mapped(v);
    s.form = Rpf9Form::StoredRsc8 { config: 0 };
    let mut list = read_list(&s.virtual_section).expect("it reads");
    list.emitters[0].kfps[1].entries.push(KfpEntry {
        time: [0.0; 4],
        value: [1.0; 4],
    });
    let before = s.clone();
    let refused = add_particle_keys(&mut s, &list).expect_err("no block map");
    assert!(matches!(refused, ParticleKeysError::Grow(_)), "{refused}");
    assert_eq!(s, before);
}
