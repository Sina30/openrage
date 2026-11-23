use super::*;

/// The tables a test body states after its index, each as its words.
#[derive(Default, Clone)]
struct Tables {
    groups: Vec<Vec<u32>>,
    names: u32,
    ciphertext: Vec<u8>,
    hash: Vec<u32>,
    wave: Vec<u32>,
    group: Vec<u32>,
    references: Vec<(u32, u32)>,
    reference: Vec<u32>,
    y: Vec<[u32; 2]>,
}

/// A body of `schema` holding `items` (key, bytes) back to back after the stamp, and `t`.
fn body(schema: u32, items: &[(u32, Vec<u8>)], t: &Tables) -> Vec<u8> {
    let mut data = STAMP.to_le_bytes().to_vec();
    let mut index = Vec::new();
    for (key, bytes) in items {
        index.push((*key, data.len() as u32));
        data.extend(bytes);
    }
    index.sort_by_key(|(k, _)| index_order(*k));
    let mut v = Vec::new();
    let put = |w: u32, v: &mut Vec<u8>| v.extend(w.to_le_bytes());
    put(schema, &mut v);
    put(data.len() as u32, &mut v);
    v.extend(&data);
    let hashes: Vec<u32> = t.groups.concat();
    put((4 + 8 * t.groups.len() + 4 * hashes.len()) as u32, &mut v);
    put(t.groups.len() as u32, &mut v);
    let mut first = 0;
    for g in &t.groups {
        put(first, &mut v);
        put(g.len() as u32, &mut v);
        first += g.len() as u32;
    }
    hashes.iter().for_each(|h| put(*h, &mut v));
    put(4 + t.ciphertext.len() as u32, &mut v);
    put(t.names, &mut v);
    v.extend(&t.ciphertext);
    put(index.len() as u32, &mut v);
    for (k, o) in &index {
        put(*k, &mut v);
        put(*o, &mut v);
    }
    let kind = AudioDataKind::of_schema(schema).expect("a schema");
    if kind.has_y() {
        put(index.len() as u32, &mut v);
        for ((_, o), y) in index.iter().zip(&t.y) {
            put(y[0], &mut v);
            put(y[1], &mut v);
            put(*o, &mut v);
        }
    }
    let table = |fields: &[u32], v: &mut Vec<u8>| {
        v.extend((fields.len() as u32).to_le_bytes());
        fields.iter().for_each(|f| v.extend(f.to_le_bytes()));
    };
    table(&t.hash, &mut v);
    table(&t.wave, &mut v);
    if kind.tables() >= 3 {
        table(&t.group, &mut v);
    }
    if kind.tables() == 5 {
        put(t.references.len() as u32, &mut v);
        for (k, m) in &t.references {
            put(*k, &mut v);
            put(*m, &mut v);
        }
        table(&t.reference, &mut v);
    }
    v
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Three sounds: a class-13 wave, a class-12 sound with a wave field and a reference, and a class
/// whose layout is not known with a group slot.
fn sounds() -> (Vec<u8>, Tables) {
    let wave13 = [vec![13, 7, 9], words(&[0xB0B0_0001, 0xE000_0123])].concat();
    let mut sound12 = vec![12, 0];
    sound12.extend(words(&[0xB0B0_0002, 0x2000_0456, 0, 0]));
    let opaque = [vec![70, 1, 2, 3, 4], words(&[0xFFFF_FFFF, 1])].concat();
    // Items at data offsets 4, 15 and 33; a field's body offset is 8 more.
    let items = [
        (0xC641_2B01, wave13),
        (0x4665_4704, sound12),
        (0x0000_0105, opaque),
    ];
    let t = Tables {
        groups: vec![vec![0x1111], vec![0x2222, 0x3333]],
        names: 2,
        ciphertext: vec![0x9A; 12],
        hash: vec![8 + 33 + 5 + 4],
        wave: vec![8 + 4 + 3, 8 + 15 + 2],
        group: vec![8 + 33 + 5],
        references: vec![(0x0000_0105, 0x2), (0xABCD_0000, 0x3)],
        reference: vec![8 + 15 + 10, 8 + 15 + 14],
        y: Vec::new(),
    };
    let mut v = body(166, &items, &t);
    // The reference fields name rows 1 and 0.
    let row = |v: &mut Vec<u8>, at: usize, w: u32| v[at..at + 4].copy_from_slice(&w.to_le_bytes());
    row(&mut v, 8 + 15 + 10, 1);
    row(&mut v, 8 + 15 + 14, 0);
    (v, t)
}

#[test]
fn a_sounds_body_reads_its_items_their_waves_and_references() {
    let (v, _) = sounds();
    assert!(stamped(&v));
    let data = AudioData::read(&v).expect("it reads");
    assert_eq!(data.kind, AudioDataKind::Sounds);
    assert_eq!(data.names, 2);
    assert_eq!(data.name_bytes, 12);
    assert_eq!(data.groups, [(0, 1), (1, 2)]);
    let items = data.items();
    let by_key = |k: u32| items.iter().find(|i| i.key == k).expect("an item");
    let wave = by_key(0xC641_2B01);
    assert_eq!((wave.offset, wave.span, wave.class), (4, 11, Some(13)));
    assert_eq!(
        wave.fields,
        AudioFields::Wave {
            bank: 0xB0B0_0001,
            wave: 0xE000_0123
        }
    );
    assert_eq!(wave.waves[0].stream_id(), 0x123);
    let sound = by_key(0x4665_4704);
    assert_eq!(sound.class, Some(12));
    assert_eq!(sound.fields, AudioFields::Opaque);
    assert_eq!(
        sound.waves,
        [WaveReference {
            at: 2,
            bank: 0xB0B0_0002,
            wave: 0x2000_0456
        }]
    );
    assert_eq!(
        sound.references,
        [
            AudioReference {
                at: 10,
                key: 0xABCD_0000,
                mask: 3
            },
            AudioReference {
                at: 14,
                key: 0x0000_0105,
                mask: 2
            },
        ]
    );
    assert_eq!(
        AudioDataKind::of_mask(3),
        [AudioDataKind::GameObjects, AudioDataKind::Sounds]
    );
    let opaque = by_key(0x0000_0105);
    assert_eq!((opaque.span, opaque.class), (13, Some(70)));
    assert!(opaque.waves.is_empty() && opaque.references.is_empty());
}

#[test]
fn curves_and_a_second_index_read_as_laid_out() {
    let mut base = vec![4u8, 0xAA, 0xAA, 0xAA, 0xAA, 0, 0, 0];
    base.extend(words(&[0.5f32.to_bits(), 2.0f32.to_bits(), 2]));
    base.extend(words(&[
        0.0f32.to_bits(),
        1.0f32.to_bits(),
        1.0f32.to_bits(),
        0.25f32.to_bits(),
    ]));
    let mut constant = vec![1u8, 1, 0, 0, 0, 0, 0, 0];
    constant.extend(words(&[0, 1.0f32.to_bits(), 3.0f32.to_bits()]));
    let t = Tables::default();
    let v = body(25, &[(0x10, base), (0x20, constant)], &t);
    let data = AudioData::read(&v).expect("it reads");
    let items = data.items();
    assert_eq!(
        items[0].fields,
        AudioFields::Curve(Curve {
            flags: 0xAAAA_AAAA,
            min_input: 0.5,
            max_input: 2.0,
            shape: CurveShape::PiecewiseLinear(vec![[0.0, 1.0], [1.0, 0.25]]),
        })
    );
    assert!(
        matches!(&items[1].fields, AudioFields::Curve(c) if c.shape == CurveShape::Constant(3.0))
    );

    let t = Tables {
        y: vec![[1, 2], [3, 4]],
        ..Tables::default()
    };
    let v = body(581, &[(0x30, vec![9, 0, 0, 0]), (0x40, vec![9, 1])], &t);
    let data = AudioData::read(&v).expect("it reads");
    assert_eq!(data.index_y.len(), 2);
    let mut twice = t.clone();
    twice.y[1] = [1, 2];
    assert!(AudioData::read(&body(
        581,
        &[(0x30, vec![9, 0, 0, 0]), (0x40, vec![9, 1])],
        &twice
    ))
    .is_err());
}

#[cfg(feature = "rel")]
#[test]
fn an_audio_configuration_item_reads_as_gta_v_lays_it_out() {
    let float = [
        vec![2u8, 0x10, 0, 0],
        words(&[0xAAAA_AAAA, 1.5f32.to_bits()]),
    ]
    .concat();
    let v = body(26, &[(0x77, float)], &Tables::default());
    let items = AudioData::read(&v).expect("it reads").items();
    let AudioFields::Config(item) = &items[0].fields else {
        panic!("a configuration item: {:?}", items[0].fields);
    };
    assert_eq!(item.value, crate::rel::config::ConfigValue::Float(1.5));
    assert_eq!(item.name_table_offset, 0x10);
}

#[test]
fn a_body_that_breaks_a_measured_rule_is_refused() {
    let (v, t) = sounds();
    let index_at = v.len()
        - 4 * (1 + t.reference.len())
        - (4 + 8 * t.references.len())
        - 4 * (1 + t.group.len())
        - 4 * (1 + t.wave.len())
        - 4 * (1 + t.hash.len())
        - 8 * 3;
    type Patch = Box<dyn Fn(&mut Vec<u8>)>;
    let cases: Vec<(Patch, &str)> = vec![
        (Box::new(|v| v[0] = 99), "not GTA VI audio game data"),
        (Box::new(|v| v[8] = 0), "stamp"),
        (Box::new(|v| v.push(0)), "follow the last table"),
        (
            Box::new(move |v| {
                let (a, b) = (index_at, index_at + 8);
                let first: Vec<u8> = v[a..b].to_vec();
                v.copy_within(b..b + 8, a);
                v[b..b + 8].copy_from_slice(&first);
            }),
            "not sorted",
        ),
        (Box::new(|v| v[8 + 15 + 10] = 7), "names no row"),
        (Box::new(|v| v[8 + 33 + 5] = 5), "names no group"),
        (Box::new(|v| v[8 + 4 + 3] = 2), "distinct wave containers"),
    ];
    for (patch, want) in cases {
        let mut bytes = v.clone();
        patch(&mut bytes);
        let err = AudioData::read(&bytes).expect_err(want).to_string();
        assert!(err.contains(want), "{want}: {err}");
    }
    for len in 0..v.len() {
        assert!(AudioData::read(&v[..len]).is_err(), "{len} bytes read");
    }
}
