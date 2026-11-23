//! A GTA VI audio item laid out by its class's table: a row per slot a field table or a measured
//! value type places, by offset, and the bytes between the slots as they are.

use std::ops::Range;

use egui_extras::{Column, TableBuilder};
use or_io::gta6::audio_links::{SlotRole, SlotValue, TableField, TableItem, TableValue};

use super::super::{grid, row};
use crate::ui::theme;

/// The item's length and counts, a note, then its slots and bytes, `height` high at most.
pub(super) fn draw(
    ui: &mut egui::Ui,
    table: &TableItem,
    item: &[u8],
    name: impl Fn(u32) -> String,
    height: f32,
) {
    grid(ui, "gta6_audio_item", |ui| {
        row(ui, tr!("Length"), &table.length.to_string());
        row(
            ui,
            tr!("Bytes to the next item"),
            &table.to_next_item.to_string(),
        );
        if !table.counts.is_empty() {
            let counts: Vec<String> = table.counts.iter().map(usize::to_string).collect();
            row(ui, tr!("Counted elements"), &counts.join(", "));
        }
    });
    ui.label(
        egui::RichText::new(tr!("Each slot a field table or a measured value type places in the item, at its offset, and the bytes between them as they are. A value type is what every item measured holds there, which no table of the file states, so a value may break it; the types are shown only on an item that ends where its layout does."))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    // A class's index item holds a pair per item of its class, so only the rows in view are
    // written out and drawn.
    let lines = lines(table, item.len());
    let line_height = ui.spacing().interact_size.y;
    TableBuilder::new(ui)
        .id_salt("gta6_audio_slots")
        .striped(true)
        .max_scroll_height(height)
        .auto_shrink([false, true])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(56.0))
        .column(Column::auto().at_least(96.0))
        .column(Column::remainder().clip(true))
        .header(line_height, |mut header| {
            for heading in [tr!("Offset"), tr!("Type"), tr!("Value")] {
                header.col(|ui| {
                    ui.strong(heading);
                });
            }
        })
        .body(|body| {
            body.rows(line_height, lines.len(), |mut line| {
                let Some(at) = lines.get(line.index()) else {
                    return;
                };
                for cell in text(at, item, &name) {
                    line.col(|ui| {
                        ui.monospace(cell.as_str());
                    });
                }
            });
        });
}

/// A row: a slot of the table, or up to four bytes at an offset that no slot spans.
enum Line<'t> {
    Field(&'t TableField),
    Value(&'t TableValue),
    Bytes(Range<usize>),
}

/// The rows of `table` over an item of `len` bytes: each slot and the bytes no slot spans, in
/// offset order.
fn lines(table: &TableItem, len: usize) -> Vec<Line<'_>> {
    let fields = (table.fields.iter()).map(|f| (f.at, 4, Line::Field(f)));
    let values = (table.values.iter()).map(|v| (v.at, v.width, Line::Value(v)));
    let mut slots: Vec<(usize, usize, Line)> = fields.chain(values).collect();
    // A slot may overlap the one before it; the bytes between are shown once.
    slots.sort_by_key(|s| s.0);
    let mut out = Vec::new();
    let mut next = 0;
    for (at, width, line) in slots {
        raw(&mut out, next..at, len);
        next = next.max(at + width);
        out.push(line);
    }
    raw(&mut out, next..table.length, len);
    out
}

/// The rows of the bytes in `range`, up to four to a row; none where the item ends before it.
fn raw(out: &mut Vec<Line>, range: Range<usize>, len: usize) {
    if range.end > len {
        return;
    }
    out.extend((range.clone().step_by(4)).map(|at| Line::Bytes(at..range.end.min(at + 4))));
}

/// Each slot of `table` as its offset, its type and its value, and the bytes of `item` no slot
/// spans, in offset order; `name` shows a hash.
#[cfg(test)]
fn rows(table: &TableItem, item: &[u8], name: impl Fn(u32) -> String) -> Vec<[String; 3]> {
    (lines(table, item.len()).iter())
        .map(|l| text(l, item, &name))
        .collect()
}

/// A row's offset, type and value.
fn text(line: &Line, item: &[u8], name: impl Fn(u32) -> String) -> [String; 3] {
    let minus_one = || "-1".to_string();
    let (at, kind, value) = match line {
        Line::Bytes(range) => {
            let bytes = item.get(range.clone()).unwrap_or_default();
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            return [offset(range.start), tr!("bytes").to_string(), hex.join(" ")];
        }
        Line::Field(f) => {
            let kind = match f.role {
                SlotRole::Reference => tr!("reference row"),
                SlotRole::Group => tr!("group slot"),
                SlotRole::Hash => tr!("hash"),
                SlotRole::Bank => tr!("wave container"),
            };
            let value = match (f.role, f.value) {
                (_, None) => minus_one(),
                (SlotRole::Hash | SlotRole::Bank, Some(word)) => name(word),
                (_, Some(n)) => n.to_string(),
            };
            (f.at, kind, value)
        }
        Line::Value(v) => {
            let (kind, value) = match &v.value {
                SlotValue::Zero(word) => (tr!("zero"), measured(*word, 0)),
                SlotValue::Unset(word) => (tr!("unset"), measured(*word, 0xAAAA_AAAA)),
                SlotValue::F32(f) => (tr!("float"), f.to_string()),
                SlotValue::Number(n) => (tr!("number"), n.to_string()),
                SlotValue::F32OrMinusOne(f) => (
                    tr!("float or -1"),
                    f.map_or_else(minus_one, |f| f.to_string()),
                ),
                SlotValue::NumberOrMinusOne(n) => (
                    tr!("number or -1"),
                    n.map_or_else(minus_one, |n| n.to_string()),
                ),
                SlotValue::Key(word @ (0 | u32::MAX)) => {
                    (tr!("index key"), format!("{word:#010X}"))
                }
                SlotValue::Key(word) => (tr!("index key"), name(*word)),
                SlotValue::OwnKey(word) => (tr!("its own key"), name(*word)),
                SlotValue::Flags(word) => (tr!("2-bit flags"), flags(*word)),
                SlotValue::Hash(word) => (tr!("hash"), name(*word)),
                SlotValue::ItemOffset { offset, of_its_key } => {
                    let shown = format!("{offset:#X}");
                    let value = if *of_its_key {
                        shown
                    } else {
                        tr!(
                            "{offset}, where the item of the key before it does not start",
                            offset = shown
                        )
                    };
                    (tr!("item offset"), value)
                }
                SlotValue::Text(text) => (tr!("text"), text.clone()),
                SlotValue::ByteZero(b) => (tr!("byte, zero"), b.to_string()),
                SlotValue::ByteConstant(b) => (tr!("byte, constant"), b.to_string()),
                SlotValue::Bool(b) => (tr!("byte, 0 or 1"), b.to_string()),
                SlotValue::Enum(b) => (tr!("byte, few values"), b.to_string()),
            };
            (v.at, kind, value)
        }
    };
    [offset(at), kind.to_string(), value]
}

fn offset(at: usize) -> String {
    format!("+{at:#X}")
}

/// A word's sixteen 2-bit fields, the highest first, in groups of four.
fn flags(word: u32) -> String {
    let field = |i: u32| char::from(b'0' + (word >> (2 * i) & 3) as u8);
    let groups: Vec<String> = (0..4u32)
        .rev()
        .map(|g| (0..4).rev().map(|i| field(4 * g + i)).collect())
        .collect();
    groups.join(" ")
}

/// A word measured as `held` in every item: the word, and what they hold where it differs.
fn measured(word: u32, held: u32) -> String {
    let shown = format!("{word:#010X}");
    if word == held {
        return shown;
    }
    tr!(
        "{word}, where every item measured holds {held}",
        word = shown,
        held = format!("{held:#010X}")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_slot_and_the_bytes_between_are_rows_in_offset_order() {
        let field = |at, role, value| TableField { at, role, value };
        let value = |at, width, value| TableValue { at, width, value };
        let table = TableItem {
            length: 48,
            to_next_item: 0,
            counts: vec![1],
            fields: vec![
                field(6, SlotRole::Reference, Some(4)),
                field(10, SlotRole::Group, None),
                field(20, SlotRole::Hash, Some(0xABCD)),
            ],
            children: Vec::new(),
            values: vec![
                value(2, 4, SlotValue::F32(1.5)),
                value(14, 4, SlotValue::Zero(7)),
                value(24, 4, SlotValue::F32OrMinusOne(None)),
                value(28, 1, SlotValue::ByteConstant(3)),
                value(32, 4, SlotValue::Key(0)),
                value(
                    36,
                    4,
                    SlotValue::ItemOffset {
                        offset: 0x40,
                        of_its_key: false,
                    },
                ),
                value(40, 4, SlotValue::Flags(0b10_01)),
                value(44, 4, SlotValue::Text("ab".into())),
            ],
        };
        let item: Vec<u8> = (0..48).collect();
        let got = rows(&table, &item, |w| format!("key {w:#X}"));
        let want = [
            ["+0x0", "bytes", "00 01"],
            ["+0x2", "float", "1.5"],
            ["+0x6", "reference row", "4"],
            ["+0xA", "group slot", "-1"],
            [
                "+0xE",
                "zero",
                "0x00000007, where every item measured holds 0x00000000",
            ],
            ["+0x12", "bytes", "12 13"],
            ["+0x14", "hash", "key 0xABCD"],
            ["+0x18", "float or -1", "-1"],
            ["+0x1C", "byte, constant", "3"],
            ["+0x1D", "bytes", "1D 1E 1F"],
            ["+0x20", "index key", "0x00000000"],
            [
                "+0x24",
                "item offset",
                "0x40, where the item of the key before it does not start",
            ],
            ["+0x28", "2-bit flags", "0000 0000 0000 0021"],
            ["+0x2C", "text", "ab"],
        ];
        let want: Vec<[String; 3]> = (want.iter()).map(|r| r.map(str::to_string)).collect();
        assert_eq!(got, want);
    }
}
