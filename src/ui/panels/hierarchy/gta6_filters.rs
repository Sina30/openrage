//! A scanned GTA VI dictionary's frame filters and animation records in the outline, a row each
//! under its name as GTA V's frame filter dictionary lists its filters, and the status line of the
//! one picked.

use or_io::gta6::headerless::{Entry, Resource};

use super::*;
use crate::ui::panels::selection_ids::dict_entry_unpack_part;

/// Each frame filter and animation record of `resource`, by code, in the dictionary's order.
fn listed(resource: &Resource) -> Vec<(u32, &Entry)> {
    let Resource::Dictionary(dict) = resource else {
        return Vec::new();
    };
    (dict.entries.iter())
        .filter_map(|(code, e)| match e {
            Ok(entry @ (Entry::FrameFilter { .. } | Entry::AnimationRecord { .. })) => {
                Some((*code, entry))
            }
            _ => None,
        })
        .collect()
}

/// Whether `resource` is a dictionary holding a frame filter or an animation record.
pub(crate) fn holds_filters(resource: &Resource) -> bool {
    !listed(resource).is_empty()
}

/// An entry's name, and how many tracks or bones it weights or sets.
fn named(entry: &Entry) -> (&str, String) {
    match entry {
        Entry::FrameFilter { name, tracks } => (
            name.as_str(),
            tr_n!("{count} track", "{count} tracks", tracks.len()),
        ),
        Entry::AnimationRecord { name, bones } => (
            name.as_str(),
            tr_n!("{count} bone", "{count} bones", bones.len()),
        ),
        _ => ("", String::new()),
    }
}

/// A row for each frame filter and animation record of `resource`; a click selects it.
pub(super) fn draw_gta6_filters(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &Resource,
) {
    let mut clicked = None;
    for (code, entry) in listed(resource) {
        let (name, detail) = named(entry);
        let sel = dict_entry_id(code);
        let selected = state.selection.is_selected(doc_id, sel);
        let response = leaf_row(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui.selectable_label(selected, name);
                ui.add(detail_label(detail));
                response
            })
            .inner
        });
        if response.clicked() {
            clicked = Some(sel);
        }
    }
    if let Some(sel) = clicked {
        state.selection.select(doc_id, sel);
    }
}

/// The status line of the frame filter or animation record `selection` picks, else what the
/// dictionary holds of them; `None` for a dictionary of neither, or of drawables with none picked.
pub(crate) fn summary(resource: &Resource, selection: u64) -> Option<String> {
    let listed = listed(resource);
    if listed.is_empty() {
        return None;
    }
    let picked = (is_dict_entry_selection(selection))
        .then(|| dict_entry_unpack_part(selection).0)
        .and_then(|code| listed.iter().position(|(c, _)| *c == code));
    let Some(at) = picked else {
        if crate::state::gta6_resource::dictionary_drawables(resource)
            .next()
            .is_some()
        {
            return None;
        }
        let filters = (listed.iter())
            .filter(|(_, e)| matches!(e, Entry::FrameFilter { .. }))
            .count();
        return Some(tr!(
            "{filters} frame filters, {records} animation records",
            filters = filters,
            records = listed.len() - filters
        ));
    };
    let (name, detail) = named(listed[at].1);
    let said = match listed[at].1 {
        Entry::FrameFilter { .. } => tr!(
            "Frame filter: {name} ({at} of {total}), {detail}",
            name = name,
            at = at + 1,
            total = listed.len(),
            detail = detail
        ),
        _ => tr!(
            "Animation record: {name} ({at} of {total}), {detail}",
            name = name,
            at = at + 1,
            total = listed.len(),
            detail = detail
        ),
    };
    Some(said)
}

#[cfg(test)]
mod tests {
    use or_io::gta6::headerless::{BoneRecord, Dictionary, FilterTrack};

    use super::*;

    #[test]
    fn a_filter_and_a_record_are_listed_and_the_one_picked_is_summarised() {
        let _serial = crate::ui::theme::serial();
        let track = FilterTrack {
            track: 0x100,
            weight: 1.0,
            group: 0x200,
            word_2c: 0,
        };
        let bone = BoneRecord {
            bone: 0x300,
            kind: 1,
            flags: 2,
            values: [0.0; 27],
        };
        let filter = Entry::FrameFilter {
            name: "pack:/f.iff".into(),
            tracks: vec![track, track],
        };
        let record = Entry::AnimationRecord {
            name: "r".into(),
            bones: vec![bone],
        };
        let resource = Resource::Dictionary(Dictionary {
            entries: vec![(5, Ok(filter)), (9, Ok(record))],
        });
        assert!(holds_filters(&resource));
        let root = crate::ui::panels::selection_ids::DOC_ROOT_SEL;
        let whole = summary(&resource, root).expect("a dictionary of filters");
        assert!(
            whole.contains("1 frame filters, 1 animation records"),
            "{whole}"
        );
        let filter = summary(&resource, dict_entry_id(5)).expect("the filter");
        assert!(
            filter.contains("pack:/f.iff (1 of 2), 2 tracks"),
            "{filter}"
        );
        let record = summary(&resource, dict_entry_id(9)).expect("the record");
        assert!(record.contains("r (2 of 2), 1 bone"), "{record}");

        let none = Resource::Dictionary(Dictionary { entries: vec![] });
        assert!(!holds_filters(&none));
        assert_eq!(summary(&none, root), None);
    }
}
