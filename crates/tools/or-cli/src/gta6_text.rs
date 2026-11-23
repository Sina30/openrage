//! `rpf scan-open` of a GTA VI text table and of audio game data: their structure and counts, and
//! with `--texts` a table's texts; `rpf scan-body --gxt2` and `--rewrite` of a text table.

use std::collections::BTreeMap;

use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Content, Rpf9Form, Rpf9ScanFind,
    Rpf9Sections, Rpf9ZlibRun,
};
use or_io::gta5::text::Gxt2;
use or_io::gta5::File as _;
use or_io::gta6::audio_data::{AudioData, AudioFields};
use or_io::gta6::text::{read, TextItem, TextTable};

/// A text table's header, map, blocks, probe distances and text counts.
pub fn print_table(virtual_section: &[u8]) -> Result<(), String> {
    let (table, layout) = read(virtual_section).map_err(|e| e.to_string())?;
    let sets = (table.entries.iter())
        .filter(|e| matches!(e.item, TextItem::Variants(_)))
        .count();
    println!(
        "  text table    {}, {} bytes, {} virtual block(s)",
        if sets > 0 {
            "with grammatical variants"
        } else {
            "plain"
        },
        layout.length,
        layout.virtual_blocks
    );
    println!(
        "  header        0x4E u16 {}; root block {}",
        layout.word_4e, layout.root
    );
    println!(
        "  map           {} groups, {} entries",
        table.groups,
        table.entries.len()
    );
    for (i, b) in layout.blocks.iter().enumerate() {
        println!(
            "  block {:<2}      type {:08x} at {:#x} size {}",
            i + 1,
            b.kind,
            b.offset,
            b.size
        );
    }
    let mut probe = BTreeMap::new();
    for e in &table.entries {
        let home = (e.key >> 7) & (table.groups - 1);
        *probe
            .entry(e.group.wrapping_sub(home) & (table.groups - 1))
            .or_insert(0) += 1;
    }
    println!("  probe         group distance -> keys {probe:?}");
    let texts: Vec<usize> = table.texts().map(|(_, t)| t.len() + 1).collect();
    let chars = (layout.blocks.iter()).filter(|b| b.kind == 0x10).count();
    println!(
        "  texts         {} ({} of them variants, in {sets} variant entries) in {} bytes, {chars} block(s)",
        texts.len(),
        texts.len() + sets - table.entries.len(),
        texts.iter().sum::<usize>(),
    );
    let keys: Vec<String> = (table.entries.iter().take(8))
        .map(|e| format!("{:08x}", e.key))
        .collect();
    println!("  keys          first 8 in slot order: {}", keys.join(" "));
    Ok(())
}

/// Every text of the text table at `offset` of `path`, a line each: key, variant form, text.
pub fn print_texts(path: &str, offset: u64, limit: u64) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    let table = table_at(&find, &bytes)?;
    for e in &table.entries {
        match &e.item {
            TextItem::Text(t) => println!("{:#010X}      {}", e.key, t.escape_debug()),
            TextItem::Variants(v) => {
                for (i, form) in v.forms.iter().enumerate() {
                    println!("{:#010X} [{i}]  {}", e.key, form.text.escape_debug());
                }
            }
        }
    }
    Ok(())
}

/// A text table's plain texts as GTA V's text table with `gxt2`, else the table written again.
pub fn converted(find: &Rpf9ScanFind, bytes: &[u8], gxt2: bool) -> Result<Vec<u8>, String> {
    let table = table_at(find, bytes)?;
    if gxt2 {
        return to_gxt2(&table);
    }
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: table.to_section().map_err(|e| e.to_string())?,
        physical: Vec::new(),
    };
    write_rpf9_resource(&sections).map_err(|e| e.to_string())
}

fn table_at(find: &Rpf9ScanFind, bytes: &[u8]) -> Result<TextTable, String> {
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        content: Rpf9Content::TextTable,
        resource: Some(resource),
        ..
    }) = find.kind
    else {
        return Err(format!("the body at {:#X} is no text table", find.offset));
    };
    let (table, _) = read(resource.sections(bytes).0).map_err(|e| e.to_string())?;
    Ok(table)
}

/// The plain texts of `table` as GTA V's text table, which holds no grammatical variants.
fn to_gxt2(table: &TextTable) -> Result<Vec<u8>, String> {
    let mut gxt2 = Gxt2::new();
    let mut variants = 0;
    for e in &table.entries {
        match &e.item {
            TextItem::Text(t) => {
                gxt2.insert(e.key, t.clone());
            }
            TextItem::Variants(_) => variants += 1,
        }
    }
    if variants > 0 {
        return Err(format!(
            "GTA V's text table holds one text per key, and {variants} key(s) of this table hold grammatical variants"
        ));
    }
    gxt2.to_bytes().map_err(|e| e.to_string())
}

/// Audio game data's tables and its items' classes, laid out or not.
pub fn print_audio(body: &[u8]) -> Result<(), String> {
    let data = AudioData::read(body).map_err(|e| e.to_string())?;
    println!(
        "  audio data    schema {} ({}), a data block of {} bytes",
        data.schema,
        data.kind.label(),
        data.data.len()
    );
    println!(
        "  groups        {} groups of {} hashes",
        data.groups.len(),
        data.group_hashes.len()
    );
    println!(
        "  names         {} names in {} bytes, encrypted and not read",
        data.names, data.name_bytes
    );
    println!(
        "  index         {} items; a second index of {} rows",
        data.index.len(),
        data.index_y.len()
    );
    println!(
        "  fields        hash {}, wave {}, group {}, reference rows {}, reference fields {}",
        data.hash_fields.len(),
        data.wave_fields.len(),
        data.group_fields.len(),
        data.references.len(),
        data.reference_fields.len()
    );
    let mut classes: BTreeMap<Option<u8>, (usize, usize)> = BTreeMap::new();
    for item in data.items() {
        let row = classes.entry(item.class).or_default();
        row.0 += 1;
        row.1 += usize::from(item.fields != AudioFields::Opaque);
    }
    for (class, (items, laid_out)) in classes {
        let class = class.map_or("none".to_string(), |c| c.to_string());
        println!("  class {class:<4}    {items} items, {laid_out} laid out");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::text::{TextEntry, Variant, Variants};

    fn table(with_variants: bool) -> TextTable {
        let mut entries = vec![
            TextEntry {
                key: 0x0000_0101,
                group: 0,
                slot: 0,
                item: TextItem::Text("une ligne".into()),
            },
            TextEntry {
                key: 0x0000_0182,
                group: 1,
                slot: 0,
                item: TextItem::Text("another line".into()),
            },
        ];
        if with_variants {
            let form = |text: &str| Variant {
                text: text.into(),
                gender: 1,
                plurality: 2,
                case: 3,
                hash_34a643c5: [[4, 5]; 9],
            };
            entries.push(TextEntry {
                key: 0x0000_0183,
                group: 1,
                slot: 1,
                item: TextItem::Variants(Variants {
                    gender: 0,
                    plurality: 1,
                    hash_27d7357f: 2,
                    hash_e0237fcf: 0,
                    forms: vec![form("one form"), form("another form")],
                }),
            });
        }
        TextTable { groups: 2, entries }
    }

    /// What a read at offset 0 makes of `file`, a loose resource.
    fn read_loose(file: Vec<u8>) -> (Rpf9ScanFind, Vec<u8>) {
        read_rpf9_body(std::io::Cursor::new(file), 0, u64::MAX).expect("it reads")
    }

    /// `table` as the loose resource a text table saves as, read at offset 0.
    fn loose(table: &TextTable) -> (Rpf9ScanFind, Vec<u8>) {
        let sections = Rpf9Sections {
            form: Rpf9Form::BlockMapped,
            virtual_section: table.to_section().expect("written"),
            physical: Vec::new(),
        };
        read_loose(write_rpf9_resource(&sections).expect("a loose resource"))
    }

    fn texts(table: &TextTable) -> Vec<(u32, String)> {
        table.texts().map(|(k, t)| (k, t.to_string())).collect()
    }

    #[test]
    fn a_text_table_converts_to_gta_v_s_and_rewrites_as_a_table_that_reads_back() {
        let (find, bytes) = loose(&table(false));
        let gxt2 = converted(&find, &bytes, true).expect("converted");
        let gxt2 = Gxt2::from_bytes(&gxt2).expect("a GXT2");
        assert_eq!(gxt2.get(0x101), Some("une ligne"));
        assert_eq!(gxt2.get(0x182), Some("another line"));
        assert_eq!(gxt2.len(), 2);

        let (find, bytes) = loose(&table(true));
        let refused = converted(&find, &bytes, true).expect_err("refused");
        assert!(refused.contains("1 key(s)"), "{refused}");
        let (find, bytes) = read_loose(converted(&find, &bytes, false).expect("rewritten"));
        let again = table_at(&find, &bytes).expect("a text table");
        assert_eq!(texts(&again), texts(&table(true)));
        assert_eq!(again.entries[..2], table(true).entries[..2]);
    }
}
