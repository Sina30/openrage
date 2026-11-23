//! What a GTA VI headerless resource's header states and the table its word at 0x50 points at,
//! below its metadata document's facts.

use std::sync::Arc;

use or_io::meta::binary::rbm::gta6::{header_facts, HeaderFacts};
use or_io::meta::binary::rbm::gta6_ids::{id_table, uuid_text, IdTable};
use or_io::meta::binary::ContainerLayout;

use super::*;

/// The header's facts and the id table as read, or why it is not, kept per document: each read
/// scans the whole section.
type Read = Arc<Option<(HeaderFacts, Result<Option<IdTable>, String>)>>;

pub(super) fn draw(ui: &mut egui::Ui, doc_id: DocumentId, meta: &crate::state::MetaDocument) {
    let Some(ContainerLayout::Rbm(rbm)) = meta.layout.as_ref().map(|l| &l.container) else {
        return;
    };
    // A headerless resource has no RSC7 header of its own, and its document holds the virtual
    // section as it was read.
    if rbm.seal.is_some() {
        return;
    }
    let key = egui::Id::new(("gta6_meta_ids", doc_id));
    let read = match ui.data(|d| d.get_temp::<Read>(key)) {
        Some(read) => read,
        None => {
            let read: Read = Arc::new(
                header_facts(&meta.raw_bytes)
                    .ok()
                    .map(|facts| (facts, id_table(&meta.raw_bytes).map_err(|e| e.to_string()))),
            );
            ui.data_mut(|d| d.insert_temp(key, read.clone()));
            read
        }
    };
    let Some((facts, ids)) = &*read else {
        return;
    };
    ui.add_space(theme::SPACING_DEFAULT);
    ui.heading(tr!("Headerless resource"));
    ui.separator();
    grid(ui, "gta6_meta_header", |ui| {
        row(ui, tr!("Root block"), &rbm.header.root_block_id.to_string());
        let blocks = tr!(
            "{virtual_blocks} virtual and {physical_blocks} physical blocks",
            virtual_blocks = facts.virtual_blocks,
            physical_blocks = facts.physical_blocks
        );
        row(ui, tr!("Block map"), &blocks);
        let strings = match facts.strings {
            Some((entries, bytes)) => tr!(
                "{entries} entries, {bytes} blob bytes, not read: no key for them is known",
                entries = entries,
                bytes = bytes
            ),
            None => tr!("None").to_string(),
        };
        row(ui, tr!("String table"), &strings);
        let table = match ids {
            Ok(Some(table)) => tr!(
                "{count} at {at}, carried as read",
                count = table.entries.len(),
                at = format!("{:#X}", table.at)
            ),
            Ok(None) => tr!("None").to_string(),
            Err(why) => why.clone(),
        };
        row(ui, tr!("Ids at 0x50"), &table);
    });
    let Ok(Some(table)) = ids else {
        return;
    };
    let uuids = table.reads_as_uuids();
    ui.label(
        egui::RichText::new(match uuids {
            true => tr!("Every entry has a version 1 or 4 UUID's version and variant, so each reads as a UUID. What object an entry names is not known."),
            false => tr!("Not every entry has a UUID's version and variant, so each is shown as its bytes. What object an entry names is not known."),
        })
        .small()
        .color(theme::TEXT_SECONDARY),
    );
    let n = table.entries.len();
    egui::CollapsingHeader::new(format!("{} ({n})", tr!("Entries")))
        .id_salt(("gta6_meta_id_entries", doc_id))
        .default_open(true)
        .show(ui, |ui| {
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
            egui::ScrollArea::vertical()
                .id_salt(("gta6_meta_id_rows", doc_id))
                .max_height(320.0)
                .show_rows(ui, row_height, n, |ui, range| {
                    for i in range {
                        let e = &table.entries[i];
                        let text = match uuids {
                            true => uuid_text(e),
                            false => e.iter().map(|b| format!("{b:02X}")).collect(),
                        };
                        ui.monospace(format!("{i:>5}  {text}"));
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIRTUAL: u64 = 0x5000_0000;

    /// A version 4 UUID with the RFC variant, in GUID byte order, made distinct by `n`.
    fn uuid(n: u8) -> [u8; 16] {
        let mut e = [n; 16];
        e[7] = 0x4A;
        e[8] = 0x9B;
        e
    }

    /// The virtual section of a headerless resource whose root holds one u32, its block map at
    /// 0xF0, and at 0x100 a table of two ids that the word at 0x50 points at.
    fn section() -> Vec<u8> {
        let mut v = vec![0u8; 0x100];
        let mut put = |at: usize, bytes: &[u8]| v[at..at + bytes.len()].copy_from_slice(bytes);
        put(0x08, &(VIRTUAL + 0xF0).to_le_bytes());
        put(0xF8, &[1, 0]);
        put(0x10, b"0DRP");
        put(0x14, &0x7Au16.to_le_bytes());
        put(0x20, &1u32.to_le_bytes());
        put(0x28, &(VIRTUAL + 0x60).to_le_bytes());
        put(0x30, &(VIRTUAL + 0x80).to_le_bytes());
        put(0x38, &(VIRTUAL + 0xC0).to_le_bytes());
        put(0x48, &[1, 0, 0, 0, 1, 0]);
        put(0x50, &(VIRTUAL + 0x100).to_le_bytes());
        // The class: its name, signature, member table, size and one member, a u32.
        put(0x60, &0x1717_0001u32.to_le_bytes());
        put(0x64, &0x5151_5151u32.to_le_bytes());
        put(0x69, &[3]);
        put(0x70, &(VIRTUAL + 0xA0).to_le_bytes());
        put(0x78, &4u32.to_le_bytes());
        put(0x7E, &1u16.to_le_bytes());
        put(0xA0, &0x1717_0010u32.to_le_bytes());
        put(0xA8, &[0x15]);
        // The one data block: the root's four bytes at 0xE0.
        put(0xC0, &0x1717_0001u32.to_le_bytes());
        put(0xC4, &4u32.to_le_bytes());
        put(0xC8, &(VIRTUAL + 0xE0).to_le_bytes());
        put(0xE0, &77u32.to_le_bytes());
        v.extend(2u32.to_le_bytes());
        v.extend(uuid(1));
        v.extend(uuid(2));
        v.resize(0x130, 0);
        v
    }

    /// The text of every label a frame draws.
    fn drawn(meta: &crate::state::MetaDocument) -> Vec<String> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| draw(ui, 7, meta));
        let update = output.platform_output.accesskit_update;
        (update.into_iter().flat_map(|u| u.nodes))
            .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_string))
            .collect()
    }

    #[test]
    fn a_headerless_resource_shows_its_header_and_its_ids_as_uuids() {
        let meta =
            crate::state::MetaDocument::from_gta_vi_headerless(&section(), &[]).expect("it opens");
        let texts = drawn(&meta);
        let shown = |want: &str| texts.iter().any(|t| t.contains(want));
        assert!(shown("1 virtual and 0 physical blocks"), "{texts:?}");
        assert!(shown("2 at 0x100, carried as read"), "{texts:?}");
        assert!(shown("so each reads as a UUID"), "{texts:?}");
        let table = id_table(&section()).expect("it reads").expect("a table");
        assert!(table.reads_as_uuids());
        assert!(shown(&uuid_text(&table.entries[1])), "{texts:?}");
    }

    #[test]
    fn a_document_that_is_no_headerless_resource_shows_nothing_of_one() {
        let mut meta =
            crate::state::MetaDocument::from_gta_vi_headerless(&section(), &[]).expect("it opens");
        meta.layout = None;
        assert!(!drawn(&meta).iter().any(|t| t.contains("carried as read")));
    }
}
