use super::*;
use crate::gta6::audio_data::STAMP;
use crate::gta6::hash::name_hash32;

/// A body of `kind` whose items (key, bytes) lie back to back after the stamp, with the field
/// tables given as data offsets.
fn data(
    kind: AudioDataKind,
    items: &[(u32, Vec<u8>)],
    wave: &[u32],
    group: &[u32],
    reference: &[u32],
) -> AudioData {
    let mut block = STAMP.to_le_bytes().to_vec();
    let mut index = Vec::new();
    for (key, bytes) in items {
        index.push((*key, block.len() as u32));
        block.extend(bytes);
    }
    let body = |fields: &[u32]| fields.iter().map(|f| f + 8).collect();
    AudioData {
        schema: 0,
        kind,
        data: block,
        groups: vec![(0, 1)],
        group_hashes: vec![0x0BAD_CAFE],
        names: 0,
        name_bytes: 0,
        index,
        index_y: Vec::new(),
        hash_fields: Vec::new(),
        wave_fields: body(wave),
        group_fields: body(group),
        references: vec![(0x0101_0101, 2), (0x0202_0202, 0x20)],
        reference_fields: body(reference),
    }
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[test]
fn a_sounds_header_states_its_mask_bytes_then_sub_masks_then_sized_fields() {
    // Presence bits 0 and 4: mask byte 0 is zero and byte 4 sets field bit 38, one byte.
    let item = [12, 0x11, 0x00, 0x40, 0x04, 0xEE];
    assert_eq!(
        sounds_header(&item),
        Some(SoundsHeader::Read {
            presence: 0x11,
            mask: 1 << 38,
            end: 5
        })
    );
    // Mask bits 0 and 1, then sub-mask bits 0 and 1 (4 bytes each) and bit 5 (1 byte), then bit 9.
    let item = [
        6, 0x03, 0x03, 0x02, 0x03, 0x20, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
    ];
    assert_eq!(
        sounds_header(&item),
        Some(SoundsHeader::Read {
            presence: 0x03,
            mask: 0x0203,
            end: 6 + 4 + 4 + 1 + 4
        })
    );
    assert_eq!(sounds_header(&item[..18]), None, "a field past the item");
    assert_eq!(
        sounds_header(&[69, NO_HEADER, 0xAA, 0xAA]),
        Some(SoundsHeader::Absent)
    );
    // Mask bit 45 (byte 5, bit 5) was never measured.
    let unmeasured = Some(SoundsHeader::Unmeasured);
    assert_eq!(sounds_header(&[53, 0x20, 0x20, 0, 0, 0, 0]), unmeasured);
    // Sub-mask bits of mask bits 4 to 7 were never measured.
    assert_eq!(sounds_header(&[53, 0x01, 0x10, 0x01, 0, 0]), unmeasured);
}

#[test]
fn sounds_items_of_classes_12_8_and_6_lay_out_and_break_rules_refuse() {
    let (bank, wave, duration) = (0xB0B0_0001, 0xE000_0B02, 0x0000_1F40);
    // Class 12 at 4: a 5-byte header, bank, wave, two zero bytes, one row.
    let mut simple = vec![12, 0x11, 0x00, 0x40, 0x04];
    simple.extend(words(&[bank, wave]));
    simple.extend([0, 0, 1]);
    simple.extend(words(&[1]));
    // Class 8 at 24: a 4-byte header, duration, child class, one row.
    let mut streaming = vec![8, 0x10, 0x40, 0x07];
    streaming.extend(words(&[duration]));
    streaming.extend([12, 1]);
    streaming.extend(words(&[0]));
    // Class 6 at 38: no field mask, a row, group slot 0, no second row.
    let mut class6 = vec![6, 0x00];
    class6.extend(words(&[1, 0, u32::MAX]));
    let items = [
        (0x0101_0101, simple),
        (0x0303_0303, streaming),
        (0x0404_0404, class6),
    ];
    let body = data(AudioDataKind::Sounds, &items, &[9], &[44], &[20, 34, 40]);
    let laid: Vec<_> = (body.items().iter()).map(|i| layout(&body, i)).collect();
    assert_eq!(
        laid,
        [
            Some(ItemLayout::SimpleSound {
                mask: 1 << 38,
                bank,
                wave,
                rows: vec![1]
            }),
            Some(ItemLayout::StreamingSound {
                mask: 1 << 38,
                duration,
                child_class: 12,
                rows: vec![0]
            }),
            Some(ItemLayout::Class6 {
                mask: 0,
                first_row: Some(1),
                group_slot: Some(0),
                second_row: None
            }),
        ]
    );

    let mut broken = body.clone();
    broken.data[4 + 13] = 7;
    assert_eq!(layout(&broken, &broken.items()[0]), None, "a zero byte set");
    let refused = |wave: &[u32], group: &[u32], reference: &[u32], item: usize| {
        let broken = data(AudioDataKind::Sounds, &items, wave, group, reference);
        layout(&broken, &broken.items()[item]).is_none()
    };
    assert!(
        refused(&[9], &[44], &[20, 40], 1),
        "a child word is no reference field"
    );
    assert!(
        refused(&[9], &[], &[20, 34, 40], 2),
        "the slot is no group field"
    );
    assert!(
        refused(&[13], &[44], &[20, 34, 40], 0),
        "the bank is not at the header's end"
    );
}

#[test]
fn speech_items_take_the_first_form_their_length_and_bytes_fit() {
    let mut bank = vec![2];
    bank.extend(words(&[0xB0B0_0002]));
    assert_eq!(
        speech(&bank, true),
        Some(SpeechForm::Bank {
            bank: 0xB0B0_0002,
            rest: None
        })
    );
    let mut long = bank.clone();
    long.extend([1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(
        speech(&long, true),
        Some(SpeechForm::Bank {
            bank: 0xB0B0_0002,
            rest: Some([1, 2, 3, 4, 5, 6, 7, 8])
        })
    );
    bank[0] = 3;
    assert_eq!(
        speech(&bank, true),
        None,
        "a bank item whose byte 0 is not 2"
    );

    assert_eq!(
        speech(&[3, 0x11, 0, 0x22, 0, 0x33, 0], false),
        Some(SpeechForm::Shorts(vec![0x11, 0x22, 0x33]))
    );
    let mut triples = vec![1];
    triples.extend(words(&[0xA1, 0xA2, 0xA3]));
    assert_eq!(
        speech(&triples, false),
        Some(SpeechForm::Triples(vec![[0xA1, 0xA2, 0xA3]]))
    );
    let mut rows = words(&[1, 0xC1]);
    rows.extend([0xC2, 0, 0xC3, 0]);
    rows.extend(words(&[0xC4]));
    assert_eq!(
        speech(&rows, false),
        Some(SpeechForm::Rows {
            rows: vec![(0xC1, 0xC2, 0xC3)],
            last: 0xC4
        })
    );
    let mut headed = words(&[5, 0xD2]);
    headed.push(1);
    headed.extend(words(&[0xD3, 0xD4, 0xD5]));
    assert_eq!(
        speech(&headed, false),
        Some(SpeechForm::HeadedTriples {
            head: [5, 0xD2],
            rows: vec![[0xD3, 0xD4, 0xD5]]
        })
    );
    assert_eq!(speech(&[9, 1, 2, 3], false), None, "no form fits");
}

#[test]
fn an_audio_configuration_body_names_its_items_from_the_pool_or_refuses_it() {
    let pool = b"Alpha_Mix\0beta_scene\0".to_vec();
    let item = |at: u8| vec![1, at, 0, 0, 0xAA, 0xAA, 0xAA, 0xAA];
    let config = |items: &[(u32, Vec<u8>)]| data(AudioDataKind::AudioConfig, items, &[], &[], &[]);
    let (alpha, beta) = (name_hash32(b"alpha_mix"), name_hash32(b"beta_scene"));
    let body = config(&[(beta, item(10)), (alpha, item(0))]);
    let names = config_names(&body, &pool).expect("every name hashes to its key");
    assert_eq!(names.get(&alpha).map(String::as_str), Some("Alpha_Mix"));
    assert_eq!(names.get(&beta).map(String::as_str), Some("beta_scene"));

    let mid = config(&[(beta, item(11))]);
    assert!(
        config_names(&mid, &pool).is_err(),
        "an offset inside a name"
    );
    let wrong = config(&[(alpha ^ 1, item(0))]);
    assert!(
        config_names(&wrong, &pool).is_err(),
        "a name of another key"
    );
    let twice = config(&[(alpha, item(0)), (alpha, item(0))]);
    assert!(
        config_names(&twice, &pool).is_err(),
        "one entry named twice"
    );
    let sounds = data(AudioDataKind::Sounds, &[(alpha, item(0))], &[], &[], &[]);
    assert!(
        config_names(&sounds, &pool).is_err(),
        "not an audio configuration body"
    );
}

#[test]
fn a_bank_stands_for_the_container_holding_every_found_wave_with_the_fewest_streams() {
    let wanted: BTreeSet<u32> = [1, 2, 3, 4, 5, 6, 9].into();
    let mut index = WaveIndex::default();
    // Stream words carry a chunk count in their top three bits.
    index.add(0x1000, &[0x2000_0001, 0x4000_0002, 3], &wanted);
    index.add(0x2000, &[1, 2], &wanted);
    index.add(0x3000, &[3, 1, 0x6000_0002], &wanted);
    index.add(0x4000, &[4, 5], &wanted);
    index.add(0x5000, &[4, 6], &wanted);
    let bank = |s: &[u32]| index.bank(&s.iter().copied().collect());
    assert_eq!(
        bank(&[1, 2]),
        BankLink::Container(0x2000),
        "the fewest streams"
    );
    assert_eq!(
        bank(&[3, 9]),
        BankLink::Container(0x1000),
        "copies, the first"
    );
    assert_eq!(bank(&[9]), BankLink::Absent);
    assert_eq!(bank(&[4]), BankLink::Tied(vec![0x4000, 0x5000]));
    assert_eq!(bank(&[5, 6]), BankLink::Split);
    assert!(index.holds(2) && !index.holds(9));
}
