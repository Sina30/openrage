//! The items of the audio game data kinds laid out by a table: per class, a length rule and the
//! slots where the body's field tables may put a reference row, a group slot, a hash or a bank.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::gta6::audio_data::{AudioData, AudioDataKind};

/// One line per class: schema word, class byte, byte +1 (`-` for a kind keyed by its class alone),
/// the length rule and its numbers, then each slot as `region:offset:role`.
const TABLE: &str = include_str!("audio_layouts.txt");

/// One line per class keyed as [`TABLE`]'s, then each slot whose value type every item measured
/// agrees on as `region:offset:type`: `z` zero, `a` 0xAAAAAAAA, `f` f32, `n` a u32 below 0x10000,
/// `m` f32 or -1.
const VALUES: &str = include_str!("audio_value_types.txt");

/// What the field table that lists a slot's word says it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRole {
    /// A row of the body's reference rows, or -1.
    Reference,
    /// A group-table slot, or -1.
    Group,
    /// A 32-bit hash that is no index key.
    Hash,
    /// A wave container's hash, the wave's after it.
    Bank,
}

/// Where a slot's offset counts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Head,
    /// Each element of the first counted array.
    First,
    /// The bytes between two counted arrays.
    Between,
    /// Each element of the second counted array.
    Second,
    /// The bytes after the second counted array.
    Tail,
    /// The item's stated end, counted back.
    End,
}

/// How long a class's item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Fixed(usize),
    /// A count of `width` bytes at `at`, that many `stride`-byte elements after it, then `tail`.
    Counted {
        at: usize,
        width: usize,
        stride: usize,
        tail: usize,
    },
    /// A `u8` count at `at` and its `first`-byte elements, `gap` bytes, a `u8` count and its
    /// `second`-byte elements, then `tail`.
    Two {
        at: usize,
        first: usize,
        gap: usize,
        second: usize,
        tail: usize,
    },
}

struct Layout {
    rule: Rule,
    slots: Vec<(Region, isize, SlotRole)>,
}

/// Schema word, class byte, and byte +1 where the kind's layouts are keyed by it.
type Key = (u32, u8, Option<u8>);

static LAYOUTS: LazyLock<BTreeMap<Key, Layout>> =
    LazyLock::new(|| TABLE.lines().filter_map(parse_line).collect());

/// A value type measured over the items a layout fits exactly, which no table of the file states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Zero,
    Unset,
    F32,
    Number,
    F32OrMinusOne,
}

/// A key's typed slots: each one's region, its offset there and its type.
type Slots = Vec<(Region, isize, ValueType)>;

static VALUE_SLOTS: LazyLock<BTreeMap<Key, Slots>> =
    LazyLock::new(|| VALUES.lines().filter_map(parse_values).collect());

fn parse_values(line: &str) -> Option<(Key, Slots)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let (head, rest) = words.split_at_checked(3)?;
    let slots = rest.iter().map(|word| {
        let (region, offset, letter) = place(word)?;
        let value = match letter {
            "z" => ValueType::Zero,
            "a" => ValueType::Unset,
            "f" => ValueType::F32,
            "n" => ValueType::Number,
            "m" => ValueType::F32OrMinusOne,
            _ => return None,
        };
        Some((region, offset, value))
    });
    Some((key(head)?, slots.collect::<Option<_>>()?))
}

/// A table line's schema word, class byte and byte +1.
fn key(head: &[&str]) -> Option<Key> {
    let b1 = match *head.get(2)? {
        "-" => None,
        b => Some(b.parse().ok()?),
    };
    Some((head.first()?.parse().ok()?, head.get(1)?.parse().ok()?, b1))
}

fn parse_line(line: &str) -> Option<(Key, Layout)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let (head, rest) = words.split_at_checked(4)?;
    let key = key(head)?;
    let numbers = |n: usize| -> Option<Vec<usize>> {
        rest.get(..n)?.iter().map(|w| w.parse().ok()).collect()
    };
    let (rule, used) = match head[3] {
        "fixed" => (Rule::Fixed(numbers(1)?[0]), 1),
        "counted" => {
            let v = numbers(4)?;
            let (at, width, stride, tail) = (v[0], v[1], v[2], v[3]);
            let rule = Rule::Counted {
                at,
                width,
                stride,
                tail,
            };
            (rule, 4)
        }
        "two" => {
            let v = numbers(5)?;
            let (at, first, gap, second, tail) = (v[0], v[1], v[2], v[3], v[4]);
            let rule = Rule::Two {
                at,
                first,
                gap,
                second,
                tail,
            };
            (rule, 5)
        }
        _ => return None,
    };
    let slots: Option<Vec<_>> = rest[used..].iter().copied().map(slot).collect();
    let slots = slots?;
    Some((key, Layout { rule, slots }))
}

fn slot(word: &str) -> Option<(Region, isize, SlotRole)> {
    let (region, offset, letter) = place(word)?;
    let role = match letter {
        "E" => SlotRole::Reference,
        "C" => SlotRole::Group,
        "A" => SlotRole::Hash,
        "B" => SlotRole::Bank,
        _ => return None,
    };
    Some((region, offset, role))
}

/// A slot word's region, offset and the letter after them.
fn place(word: &str) -> Option<(Region, isize, &str)> {
    let mut parts = word.split(':');
    let region = match parts.next()? {
        "head" => Region::Head,
        "first" => Region::First,
        "between" => Region::Between,
        "second" => Region::Second,
        "tail" => Region::Tail,
        "end" => Region::End,
        _ => return None,
    };
    let offset = parts.next()?.parse().ok()?;
    let letter = parts.next()?;
    parts.next().is_none().then_some((region, offset, letter))
}

/// An item laid out by its class's table row.
#[derive(Debug, Clone, PartialEq)]
pub struct TableItem {
    /// Bytes the class's rule gives the item.
    pub length: usize,
    /// Bytes after them to the next item, which hold no table field.
    pub to_next_item: usize,
    /// Each counted array's elements.
    pub counts: Vec<usize>,
    /// Every slot the item has, in offset order.
    pub fields: Vec<TableField>,
    /// A category's sub-categories: the index keys its array holds.
    pub children: Vec<u32>,
    /// Every slot a value type is measured for, its word read as that type, in offset order.
    pub values: Vec<TableValue>,
}

/// A slot of an item and its word as the value type measured for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableValue {
    pub at: usize,
    pub value: SlotValue,
}

/// A word as its measured value type; a type no table states, so a word may break it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlotValue {
    /// Measured zero: the word.
    Zero(u32),
    /// Measured 0xAAAAAAAA, GTA V's fill of an unset word: the word.
    Unset(u32),
    F32(f32),
    /// Measured below 0x10000, a largest value seen and not a bound.
    Number(u32),
    /// An f32, `None` for the word -1.
    F32OrMinusOne(Option<f32>),
}

impl ValueType {
    fn read(self, word: u32) -> SlotValue {
        match self {
            Self::Zero => SlotValue::Zero(word),
            Self::Unset => SlotValue::Unset(word),
            Self::F32 => SlotValue::F32(f32::from_bits(word)),
            Self::Number => SlotValue::Number(word),
            Self::F32OrMinusOne => {
                SlotValue::F32OrMinusOne((word != u32::MAX).then_some(f32::from_bits(word)))
            }
        }
    }
}

/// A slot of an item: its offset, its role, and its word, `None` where it is -1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableField {
    pub at: usize,
    pub role: SlotRole,
    pub value: Option<u32>,
}

/// Each region of an item: where it starts and, where a slot must fit inside it, its length.
type Places = Vec<(Region, usize, Option<usize>)>;

/// The regions of item `x`, with its stated length and its counts; `None` where the counts run
/// past the item.
fn regions(rule: Rule, x: &[u8]) -> Option<(usize, Vec<usize>, Places)> {
    let count = |at: usize, width: usize| {
        let bytes = x.get(at..at.checked_add(width)?)?;
        let n = (bytes.iter().rev()).fold(0u64, |n, b| n << 8 | u64::from(*b));
        usize::try_from(n).ok()
    };
    match rule {
        Rule::Fixed(length) => {
            let out = vec![(Region::Head, 0, Some(length)), (Region::End, length, None)];
            Some((length, Vec::new(), out))
        }
        Rule::Counted {
            at,
            width,
            stride,
            tail,
        } => {
            let n = count(at, width)?;
            let from = at + width;
            let end = from.checked_add(stride.checked_mul(n)?)?;
            let length = end.checked_add(tail)?;
            if length > x.len() {
                return None;
            }
            let mut out = vec![(Region::Head, 0, Some(at))];
            out.extend((0..n).map(|i| (Region::First, from + stride * i, None)));
            out.push((Region::End, length, None));
            Some((length, vec![n], out))
        }
        Rule::Two {
            at,
            first,
            gap,
            second,
            tail,
        } => {
            let n1 = count(at, 1)?;
            let a1 = at + 1;
            let e1 = a1.checked_add(first.checked_mul(n1)?)?;
            let n2 = count(e1.checked_add(gap)?, 1)?;
            let a2 = e1 + gap + 1;
            let e2 = a2.checked_add(second.checked_mul(n2)?)?;
            let length = e2.checked_add(tail)?;
            if length > x.len() {
                return None;
            }
            let mut out = vec![(Region::Head, 0, Some(at))];
            out.extend((0..n1).map(|i| (Region::First, a1 + first * i, Some(first))));
            out.push((Region::Between, e1, Some(gap)));
            out.extend((0..n2).map(|i| (Region::Second, a2 + second * i, Some(second))));
            out.push((Region::Tail, e2, Some(tail)));
            Some((length, vec![n1, n2], out))
        }
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The layout of item `bytes`, to the next item, at data offset `start` of `data`; `None` where
/// its class has no table row or a table field in its bytes lies off the row's slots.
pub(super) fn table_item(data: &AudioData, start: usize, bytes: &[u8]) -> Option<TableItem> {
    let (&class, rest) = bytes.split_first()?;
    let b1 = match data.kind {
        AudioDataKind::GameObjects => Some(*rest.first()?),
        _ => None,
    };
    if rest.is_empty() {
        return None;
    }
    let key = (data.schema, class, b1);
    let layout = LAYOUTS.get(&key)?;
    let (length, counts, places) = regions(layout.rule, bytes)?;
    if length > bytes.len() {
        return None;
    }

    let want = placed(&places, &layout.slots);
    // A field table lists body offsets, 8 past data offsets; a later table's role wins.
    let mut found = BTreeMap::new();
    let tables = [
        (&data.hash_fields, SlotRole::Hash),
        (&data.wave_fields, SlotRole::Bank),
        (&data.group_fields, SlotRole::Group),
        (&data.reference_fields, SlotRole::Reference),
    ];
    let first = start + 8;
    for (fields, role) in tables {
        let from = fields.partition_point(|f| (*f as usize) < first);
        let to = fields.partition_point(|f| (*f as usize) < first + bytes.len());
        for f in &fields[from..to] {
            found.insert(*f as usize - first, role);
        }
    }
    if (found.iter()).any(|(at, role)| *at >= length || want.get(at) != Some(role)) {
        return None;
    }
    let mut fields = Vec::with_capacity(want.len());
    for (&at, &role) in &want {
        let word = u32_at(bytes, at).filter(|_| at + 4 <= length)?;
        let listed = found.contains_key(&at);
        if !listed && (role != SlotRole::Reference || word != u32::MAX) {
            return None;
        }
        let value = (word != u32::MAX).then_some(word);
        fields.push(TableField { at, role, value });
    }
    let children = match data.kind {
        AudioDataKind::Categories => (places.iter())
            .filter(|(region, ..)| *region == Region::First)
            .filter_map(|(_, at, _)| u32_at(bytes, *at))
            .collect(),
        _ => Vec::new(),
    };
    let typed = VALUE_SLOTS.get(&key).map(|slots| placed(&places, slots));
    let values = (typed.into_iter().flatten())
        .filter_map(|(at, kind)| {
            let word = u32_at(bytes, at).filter(|_| at + 4 <= length)?;
            Some(TableValue {
                at,
                value: kind.read(word),
            })
        })
        .collect();
    Some(TableItem {
        length,
        to_next_item: bytes.len() - length,
        counts,
        fields,
        children,
        values,
    })
}

/// Where each of `slots` falls in an item whose regions are `places`, by item offset.
fn placed<T: Copy>(places: &Places, slots: &[(Region, isize, T)]) -> BTreeMap<usize, T> {
    let mut at = BTreeMap::new();
    for &(region, from, room) in places {
        for &(r, offset, what) in slots {
            if r != region || room.is_some_and(|room| offset + 4 > room as isize) {
                continue;
            }
            if let Some(to) = from.checked_add_signed(offset) {
                at.insert(to, what);
            }
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_of_the_table_reads_as_one_class() {
        assert_eq!(LAYOUTS.len(), TABLE.lines().count());
        assert_eq!(VALUE_SLOTS.len(), VALUES.lines().count());
        for (key, slots) in VALUE_SLOTS.iter() {
            let layout = LAYOUTS
                .get(key)
                .expect("a value line keys a laid-out class");
            let roles: Vec<(Region, isize)> = layout.slots.iter().map(|s| (s.0, s.1)).collect();
            assert!(
                slots.iter().all(|s| !roles.contains(&(s.0, s.1))),
                "{key:?}: a value slot sits on a role slot"
            );
        }
    }

    /// GameObjects class 48 with byte +1 zero: 100 bytes, its group slots at +48 and +76 and its
    /// reference slots -1; its value line states an f32 at +8, an f32 or -1 at +20 and zero at +40.
    #[test]
    fn a_slot_of_a_measured_value_type_reads_as_that_type() {
        let mut item = vec![0; 100];
        (item[0], item[1]) = (48, 0);
        for at in [24, 48, 60, 72, 76] {
            put(&mut item, at, u32::MAX);
        }
        put(&mut item, 8, 1.5f32.to_bits());
        let groups: [&[usize]; 4] = [&[], &[], &[48, 76], &[]];
        let data = body(AudioDataKind::GameObjects, 581, &item, groups);
        let got = laid(&data).expect("laid out");
        let value = |at: usize| got.values.iter().find(|v| v.at == at).map(|v| v.value);
        assert_eq!(value(8), Some(SlotValue::F32(1.5)));
        assert_eq!(value(20), Some(SlotValue::F32OrMinusOne(Some(0.0))));
        assert_eq!(value(40), Some(SlotValue::Zero(0)));
        assert_eq!(value(24), None);
        let stated = VALUE_SLOTS.get(&(581, 48, Some(0))).map(Vec::len);
        assert_eq!(Some(got.values.len()), stated);
        assert!(got.values.windows(2).all(|w| w[0].at < w[1].at));

        put(&mut item, 20, u32::MAX);
        put(&mut item, 40, 7);
        let data = body(AudioDataKind::GameObjects, 581, &item, groups);
        let got = laid(&data).expect("a word that breaks its type refuses nothing");
        let value = |at: usize| got.values.iter().find(|v| v.at == at).map(|v| v.value);
        assert_eq!(value(20), Some(SlotValue::F32OrMinusOne(None)));
        assert_eq!(value(40), Some(SlotValue::Zero(7)));
    }

    /// A body of `kind` whose data block is the stamp and `item`, with the fields of each table
    /// at the item offsets given.
    fn body(kind: AudioDataKind, schema: u32, item: &[u8], tables: [&[usize]; 4]) -> AudioData {
        let mut data = crate::gta6::audio_data::STAMP.to_le_bytes().to_vec();
        data.extend_from_slice(item);
        let listed = |at: &[usize]| at.iter().map(|a| (a + 4 + 8) as u32).collect::<Vec<u32>>();
        AudioData {
            schema,
            kind,
            data,
            groups: Vec::new(),
            group_hashes: Vec::new(),
            names: 0,
            name_bytes: 0,
            index: vec![(0x1234_5678, 4)],
            index_y: Vec::new(),
            hash_fields: listed(tables[0]),
            wave_fields: listed(tables[1]),
            group_fields: listed(tables[2]),
            references: Vec::new(),
            reference_fields: listed(tables[3]),
        }
    }

    fn put(item: &mut [u8], at: usize, word: u32) {
        item[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }

    fn laid(data: &AudioData) -> Option<TableItem> {
        table_item(data, 4, &data.data[4..])
    }

    /// The index keys of a category's three sub-categories.
    const CHILDREN: [u32; 3] = [0xA1A1_0001, 0xB2B2_0002, 0xC3C3_0003];

    /// A category of three sub-categories and `extra` bytes after them: its reference slots at
    /// +14, +20, +30 and +48, a `u8` count at +77 and the children's index keys after it.
    fn category(extra: usize) -> Vec<u8> {
        let mut item = vec![0x11; 78 + 12 + extra];
        item[0] = 0;
        put(&mut item, 14, 7);
        put(&mut item, 20, 9);
        put(&mut item, 30, u32::MAX);
        put(&mut item, 48, 2);
        item[77] = 3;
        for (i, key) in CHILDREN.iter().enumerate() {
            put(&mut item, 78 + 4 * i, *key);
        }
        item
    }

    /// A Categories body of `item`, its reference fields at `rows`.
    fn categories(item: &[u8], rows: &[usize]) -> AudioData {
        body(AudioDataKind::Categories, 41, item, [&[], &[], &[], rows])
    }

    #[test]
    fn a_category_lists_its_slots_and_its_sub_categories() {
        let rows: &[usize] = &[14, 20, 48];
        let item = category(0);
        let data = categories(&item, rows);
        let field = |at, value| TableField {
            at,
            role: SlotRole::Reference,
            value,
        };
        let want = TableItem {
            length: 90,
            to_next_item: 0,
            counts: vec![3],
            fields: vec![
                field(14, Some(7)),
                field(20, Some(9)),
                field(30, None),
                field(48, Some(2)),
            ],
            children: CHILDREN.to_vec(),
            values: Vec::new(),
        };
        assert_eq!(laid(&data), Some(want.clone()));

        let data = categories(&category(6), rows);
        let after = TableItem {
            to_next_item: 6,
            ..want
        };
        assert_eq!(laid(&data), Some(after));
    }

    #[test]
    fn an_item_whose_fields_leave_their_slots_has_no_layout() {
        let rows: &[usize] = &[14, 20, 48];
        let item = category(6);
        let off_slot: &[usize] = &[14, 16, 20, 48];
        let past_the_length: &[usize] = &[14, 20, 48, 91];
        let cases: [[&[usize]; 4]; 3] = [
            [&[], &[], &[], off_slot],
            [&[], &[], &[48], &[14, 20]],
            [&[], &[], &[], past_the_length],
        ];
        for tables in cases {
            let data = body(AudioDataKind::Categories, 41, &item, tables);
            assert_eq!(laid(&data), None, "{tables:?}");
        }
        // An unlisted reference slot holds -1, and the children's keys need the bytes they count.
        let mut unset = item.clone();
        put(&mut unset, 30, 5);
        assert_eq!(laid(&categories(&unset, rows)), None);
        assert_eq!(laid(&categories(&item[..80], rows)), None);
        assert!(laid(&categories(&item, rows)).is_some());
    }

    /// GameObjects class 30 with byte +1 zero: a `u8` count at +14 of 4-byte elements, 2 bytes,
    /// a `u8` count of 4-byte elements, then 1 byte; a reference slot at +6, a group slot at +10.
    #[test]
    fn a_game_object_of_two_arrays_is_keyed_by_its_class_and_byte_1() {
        let mut item = vec![0x22; 31];
        (item[0], item[1], item[14], item[25]) = (30, 0, 2, 1);
        put(&mut item, 6, 4);
        put(&mut item, 10, u32::MAX);
        for (at, row) in [(15, 1), (19, 3), (26, 5)] {
            put(&mut item, at, row);
        }
        let tables: [&[usize]; 4] = [&[], &[], &[10], &[6, 15, 19, 26]];
        let data = body(AudioDataKind::GameObjects, 581, &item, tables);
        let got = laid(&data).expect("laid out");
        assert_eq!((got.length, got.to_next_item), (31, 0));
        assert_eq!(got.counts, [2, 1]);
        let fields = got.fields.iter().map(|f| (f.at, f.role, f.value));
        let row = SlotRole::Reference;
        assert_eq!(
            fields.collect::<Vec<_>>(),
            [
                (6, row, Some(4)),
                (10, SlotRole::Group, None),
                (15, row, Some(1)),
                (19, row, Some(3)),
                (26, row, Some(5)),
            ]
        );
        assert!(got.children.is_empty());
        item[1] = 0x7B;
        let data = body(AudioDataKind::GameObjects, 581, &item, tables);
        assert_eq!(laid(&data), None);
    }
}
