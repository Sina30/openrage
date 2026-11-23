use egui::TextStyle;

use crate::state::name_index::{NameIndex, NameIndexCategory};
use crate::state::{AppState, DocPayload};
use crate::ui::theme;

use or_anim::clip::{self, track_name};
use or_io::codec::anim::ChannelWalk;
use or_io::gta5::CrClipDictionary;

struct ClipRow {
    key: u32,
    name: String,
    duration: f32,
    looped: bool,
    frames: u16,
    bones: usize,
    tags: usize,
    properties: usize,
    tracks: String,
    encodings: String,
    compact: bool,
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(doc_id) = state.active_document else {
        empty_state(ui);
        return;
    };
    let Some(DocPayload::Resource(or_io::Asset::Clips(dict))) =
        state.document(doc_id).map(|d| &d.payload)
    else {
        empty_state(ui);
        return;
    };

    let query_id = egui::Id::new(("ycd_search", doc_id));
    let mut query: String = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(query_id))
        .unwrap_or_default();

    // The document's own string table is the best source there is for its own
    // keys, so it goes into the index before the index is asked anything.
    let embedded: Vec<String> = clip::clips(dict)
        .map(|(_, c)| clip::header(c).name.data.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let animations = dict
        .anim_dictionary
        .as_deref()
        .map_or(0, |a| a.animations.entries().count());
    let buckets = dict.clips.hash.len();
    let dict = dict.clone();
    for name in embedded {
        state.name_index.add(NameIndexCategory::ClipSet, name);
    }

    let rows = build_rows(&dict, &state.name_index);
    let total = rows.len();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{total} clips")).strong());
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(
            egui::RichText::new(format!("{animations} animations · {buckets} hash buckets"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Filter by name, key (hex) or encoding…")
                .desired_width(240.0),
        );
        if resp.changed() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(query_id, query.clone()));
        }
        if !query.is_empty()
            && ui
                .button(egui_phosphor::regular::X)
                .on_hover_text("Clear filter")
                .clicked()
        {
            query.clear();
            ui.ctx()
                .data_mut(|d| d.insert_temp(query_id, query.clone()));
        }
    });
    ui.separator();

    let needle = query.trim().to_ascii_lowercase();
    let visible: Vec<&ClipRow> = if needle.is_empty() {
        rows.iter().collect()
    } else {
        let key_needle = needle.strip_prefix("0x").unwrap_or(&needle);
        rows.iter()
            .filter(|row| {
                row.name.to_ascii_lowercase().contains(&needle)
                    || format!("{:08x}", row.key).contains(key_needle)
                    || row.encodings.to_ascii_lowercase().contains(&needle)
                    || row.tracks.to_ascii_lowercase().contains(&needle)
            })
            .collect()
    };

    if visible.is_empty() {
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(
            egui::RichText::new(if total == 0 {
                "This clip dictionary is empty."
            } else {
                "No clips match the filter."
            })
            .color(theme::TEXT_SECONDARY),
        );
        return;
    }

    if visible.len() != total {
        ui.label(
            egui::RichText::new(format!("Showing {} of {total}", visible.len()))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }

    let row_height = ui.text_style_height(&TextStyle::Body) + 8.0;
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, visible.len(), |ui, range| {
            egui::Grid::new("ycd_rows")
                .num_columns(8)
                .striped(true)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    if range.start == 0 {
                        header(ui);
                    }
                    for row in &visible[range] {
                        ui.label(
                            egui::RichText::new(format!("0x{:08X}", row.key))
                                .monospace()
                                .color(theme::legible(theme::KIND_YCD, theme::AA)),
                        );
                        let mut name = egui::RichText::new(&row.name).monospace();
                        if row.compact {
                            name = name.color(theme::TEXT_SECONDARY);
                        }
                        ui.label(name).on_hover_text(if row.compact {
                            "Compact (compressed) animation blocks: readable and writable, \
                             but not decodable without crAnimCache expansion."
                        } else {
                            row.name.as_str()
                        });
                        ui.label(format!("{:.3}s", row.duration));
                        ui.label(if row.looped { "yes" } else { "—" });
                        ui.label(row.frames.to_string());
                        ui.label(row.bones.to_string());
                        ui.label(format!("{} / {}", row.tags, row.properties));
                        ui.label(
                            egui::RichText::new(if row.encodings.is_empty() {
                                "—".to_string()
                            } else {
                                row.encodings.clone()
                            })
                            .small()
                            .color(theme::TEXT_SECONDARY),
                        )
                        .on_hover_text(format!("tracks: {}", row.tracks));
                        ui.end_row();
                    }
                });
        });
}

fn header(ui: &mut egui::Ui) {
    for label in [
        "Key",
        "Name",
        "Duration",
        "Loop",
        "Frames",
        "Bones",
        "Tags / Props",
        "Channel encodings",
    ] {
        ui.label(
            egui::RichText::new(label)
                .strong()
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
    ui.end_row();
}

fn build_rows(dict: &CrClipDictionary, names: &NameIndex) -> Vec<ClipRow> {
    clip::clips(dict)
        .map(|(key, c)| {
            let animation = clip::primary(dict, c);
            let header = clip::header(c);
            let mut tracks: Vec<&'static str> = Vec::new();
            let mut encodings: Vec<&'static str> = Vec::new();

            if let Some(animation) = animation {
                for dof in &animation.dofs {
                    let name = track_name(dof.track);
                    if !tracks.contains(&name) {
                        tracks.push(name);
                    }
                }
                for block in animation.blocks.iter().flatten() {
                    match ChannelWalk::new(&**block).and_then(|walk| walk.channels()) {
                        Ok(channels) => {
                            for channel in channels {
                                let name = channel.format.engine_name();
                                if !encodings.contains(&name) {
                                    encodings.push(name);
                                }
                            }
                        }
                        Err(_) => {
                            if !encodings.contains(&"(undecodable)") {
                                encodings.push("(undecodable)");
                            }
                        }
                    }
                }
            }

            ClipRow {
                key,
                // The file's own name first; then the index, which is the
                // only thing that can name a clip whose string table the
                // build stripped; then an honest `hash_<hex>`.
                name: if header.name.data.is_empty() {
                    names.display(key)
                } else {
                    header.name.data.clone()
                },
                duration: clip::duration(c),
                looped: clip::is_looped(c),
                frames: animation.map(|a| a.num_frames).unwrap_or(0),
                bones: animation.map(|a| clip::bone_ids(a).len()).unwrap_or(0),
                tags: header.tags.as_deref().map(|t| t.tags.len()).unwrap_or(0),
                properties: header
                    .properties
                    .as_deref()
                    .map(|p| p.properties.entries().count())
                    .unwrap_or(0),
                tracks: tracks.join(", "),
                encodings: encodings.join(", "),
                compact: animation.is_some_and(clip::is_compact),
            }
        })
        .collect()
}

fn empty_state(ui: &mut egui::Ui) {
    theme::empty_message(ui, "No clip dictionary open");
}
