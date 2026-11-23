//! How a tag's or property's attributes, and a motion's databases, read in the inspector.

use or_io::gta6::anim::{column_parameter, Attribute, AttributeValue, Column, Record, Typed};

use super::*;

/// The leading bytes of a data attribute shown; the rest are counted.
const DATA_SHOWN: usize = 16;

/// `name = value`, a bit set's bits named by the record's one hash array of as many names.
pub(super) fn attribute(record: &Record, attribute: &Attribute) -> String {
    let names = match &attribute.value {
        AttributeValue::BitSet { bits, .. } => record.bit_names(*bits),
        _ => None,
    };
    let shown = match names {
        Some(names) => {
            let set: Vec<String> = (attribute.value.set_bits().into_iter())
                .filter_map(|i| names.get(i))
                .map(|h| named(*h))
                .collect();
            format!("[{}]", set.join(", "))
        }
        None => value_text(&attribute.value),
    };
    format!("{} = {shown}", named(attribute.name))
}

fn value_text(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Float(v) | AttributeValue::FloatWide(v) => v.to_string(),
        AttributeValue::Int(v) => v.to_string(),
        AttributeValue::Bool(v) => v.to_string(),
        AttributeValue::Hash(h) | AttributeValue::Word(h) => named(*h),
        AttributeValue::OtherWord(w) | AttributeValue::WordWide(w) => format!("{w:#010X}"),
        AttributeValue::String(s) => format!("{s:?}"),
        AttributeValue::Vector3(v) => format!("{v:?}"),
        AttributeValue::Vector4(v) | AttributeValue::Quaternion(v) => format!("{v:?}"),
        AttributeValue::Guid(g) => g.iter().map(|b| format!("{b:02X}")).collect(),
        AttributeValue::Floats(v) => format!("{v:?}"),
        AttributeValue::Words(v) | AttributeValue::Hashes(v) => {
            let shown: Vec<String> = v.iter().map(|h| named(*h)).collect();
            format!("[{}]", shown.join(", "))
        }
        AttributeValue::BitSet { bits, .. } => tr!(
            "bits {set} of {bits}",
            set = format!("{:?}", value.set_bits()),
            bits = bits
        ),
        AttributeValue::Data(bytes) => {
            let head: String = (bytes.iter().take(DATA_SHOWN))
                .map(|b| format!("{b:02X}"))
                .collect();
            tr_n!(
                "{count} byte, from {head}",
                "{count} bytes, from {head}",
                bytes.len(),
                head = head
            )
        }
        AttributeValue::Object(attributes) => {
            let shown: Vec<String> = (attributes.iter())
                .map(|a| format!("{} = {}", named(a.name), value_text(&a.value)))
                .collect();
            format!("{{{}}}", shown.join(", "))
        }
        AttributeValue::Empty => tr!("16 zero bytes").to_string(),
    }
}

/// A motion database: its columns as `props` names them, a type 5's tetrahedra and every row.
pub(super) fn draw_database(ui: &mut egui::Ui, group: usize, typed: &Typed, props: &[Record]) {
    let (columns, tables, simplices): (&[Column], Vec<&Vec<[u16; 16]>>, usize) = match typed {
        Typed::Table(database) => (database.columns.as_slice(), vec![&database.rows], 0),
        Typed::Rows(database) => (
            database.keys.as_slice(),
            database.tables.iter().collect(),
            0,
        ),
        Typed::Simplices(database) => (
            database.columns.as_slice(),
            database.tables.iter().collect(),
            database.simplices.len(),
        ),
        _ => return,
    };
    let rows: usize = tables.iter().map(|t| t.len()).sum();
    // Row `r` counted through every table in turn.
    let row_at = |mut r: usize| {
        for t in &tables {
            match t.get(r) {
                Some(row) => return Some(row),
                None => r -= t.len(),
            }
        }
        None
    };
    if !matches!(typed, Typed::Rows(_)) {
        ui.label(tr!(
            "Database of group {group}, type {kind}: {columns} columns, {tables} tables of {rows} rows in all, {simplices} simplices; a row is 16 16-bit floats in two chains of steps, whose meaning is not known",
            group = group,
            kind = typed.type_word(),
            columns = columns.len(),
            tables = tables.len(),
            rows = rows,
            simplices = simplices,
        ));
    }
    // A column's parameter is resolved once while its motion is shown, not each frame.
    let key = egui::Id::new(("gta6_anim_columns", group, columns, props.as_ptr()));
    let shown = ui.memory(|m| m.data.get_temp::<String>(key));
    let shown = shown.unwrap_or_else(|| {
        let labels: Vec<String> = (columns.iter())
            .map(|&(hash, words)| {
                let raw = || format!("{} {words:?}", named(hash));
                column_parameter(props, (hash, words)).map_or_else(|_| raw(), |p| p.label(named))
            })
            .collect();
        let shown = labels.join(", ");
        ui.memory_mut(|m| m.data.insert_temp(key, shown.clone()));
        shown
    });
    ui.monospace(shown);
    if let Typed::Table(database) = typed {
        let tetrahedra = &database.tetrahedra;
        if !tetrahedra.is_empty() {
            ui.label(tr!("Tetrahedra: each vertex's weight list, counted from its group's first, and its place in the space of the three columns"));
            scrolled(ui, ("gta6_anim_tetrahedra", group), tetrahedra.len(), |t| {
                let t = &tetrahedra[t];
                format!("{:?} {:?}", t.lists, t.vertices)
            });
        }
    }
    scrolled(ui, ("gta6_anim_rows", group), rows, |r| {
        format!(
            "{r}: {}",
            row_at(r).map_or_else(String::new, |h| halves_text(h))
        )
    });
}

/// Each of `halves` as the float it holds.
pub(super) fn halves_text(halves: &[u16]) -> String {
    let shown: Vec<String> = (halves.iter())
        .map(|h| or_io::half::half_to_f32(*h).to_string())
        .collect();
    shown.join(" ")
}

/// `count` lines in a scrolled list, `line` making only those in view.
pub(super) fn scrolled(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    count: usize,
    line: impl Fn(usize) -> String,
) {
    let row_height = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_height(row_height * ROWS_IN_VIEW)
        .auto_shrink([false, true])
        .show_rows(ui, row_height, count, |ui, range| {
            for i in range {
                ui.monospace(line(i));
            }
        });
}
