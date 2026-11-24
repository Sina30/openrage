use or_io::gta6::world_state::{Condition, Config, Rule};

use super::super::{tests_support::index_with, MapRow};
use super::*;

const OPEN: u64 = 0xA1;
const CLOSED: u64 = 0xB2;
const STORY_ONLY: u64 = 0xC3;

/// A selector that opens a door by day, and by night from 22 to 6 closes it and opens the other.
fn door() -> Selector {
    Selector {
        key: 1,
        default: 10,
        configs: vec![
            Config {
                key: 10,
                on: vec![OPEN],
                ..Config::default()
            },
            Config {
                key: 20,
                off: vec![OPEN],
                state2: vec![CLOSED],
                ..Config::default()
            },
        ],
        rules: vec![Rule {
            config: 20,
            condition: Condition::TimeOfDay { start: 22, end: 6 },
        }],
    }
}

/// A selector of the story that turns on a group of its own and turns the open door off.
fn story() -> Selector {
    Selector {
        key: 2,
        default: 30,
        configs: vec![Config {
            key: 30,
            on: vec![STORY_ONLY],
            off: vec![OPEN],
            ..Config::default()
        }],
        rules: Vec::new(),
    }
}

#[test]
fn a_group_is_on_in_the_hours_the_selectors_of_the_modes_on_pick_it() {
    let words = [
        (100, OPEN),
        (200, CLOSED),
        (300, STORY_ONLY),
        (100, OPEN),
        (400, 0xD4),
    ];
    let rows = switches(&words, &[(1, door()), (2, story())]);
    let ids: Vec<u32> = rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, [100, 200, 300, 400], "one row a group");
    let day = 0x003F_FFC0u32 & !(1 << 6);
    let night = !day & 0x00FF_FFFF;
    // Base alone: the door is open by day and closed by night.
    assert_eq!(
        [rows[0].hours[1], rows[1].hours[1], rows[2].hours[1]],
        [day, night, 0]
    );
    // With the story, its selector turns the open door off at every hour.
    assert_eq!(
        [rows[0].hours[3], rows[1].hours[3], rows[2].hours[3]],
        [0, night, 0x00FF_FFFF]
    );
    assert_eq!(rows[3].hours, [0; 8], "a group no selector names stays off");
    assert!(rows.iter().all(|r| r.hours[0] == 0), "no mode, no switch");
}

/// A script-managed map of the group `group`.
fn managed(name: u32, group: u32) -> MapRow {
    MapRow {
        archive: 0,
        name,
        block: 0,
        flags: 1,
        content_flags: 0,
        held: 2,
        parents_from: 0,
        parents_len: 0,
        group,
        modes: 1,
        offset: u64::from(name),
        boxes: [0.0; 12],
    }
}

#[test]
fn the_index_says_which_groups_are_on_at_an_hour_and_keeps_their_words() {
    let maps = vec![managed(1, 100), managed(2, 200), managed(3, 300)];
    let mut index = index_with(Vec::new(), &[], maps, Vec::new());
    let words = [(100, OPEN), (200, CLOSED), (300, STORY_ONLY)];
    index.switches = switches(&words, &[(1, door()), (2, story())]);
    let index = index.finish();
    assert_eq!(index.groups()[0].word, OPEN);
    assert_eq!(index.default_on_in(12.0, 1), [100]);
    assert_eq!(index.default_on_in(23.5, 1), [200]);
    assert_eq!(index.default_on_in(12.0, 3), [300]);
    assert_eq!(index.default_on(12.0), [300]);
    // An hour past the day comes round again.
    assert_eq!(index.default_on_in(36.0, 1), [100]);
    assert_eq!(index.default_on_in(-1.0, 1), [200]);
}
