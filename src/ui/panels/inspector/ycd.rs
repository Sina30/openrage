use super::*;

pub(super) fn draw_ycd(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    use crate::ui::panels::selection_ids::ycd_selected_clip_key;
    use or_anim::clip::{format_name, header, track_name};
    use or_io::gta5::CrClipAny;

    let Some(DocPayload::Resource(or_io::Asset::Clips(dict))) =
        state.document(doc_id).map(|doc| &doc.payload)
    else {
        return;
    };

    let animations = dict
        .anim_dictionary
        .as_deref()
        .map_or(0, |a| a.animations.entries().count());
    // ⚠ Not the dictionary's two flags nor its hash buckets, which the declared grid below lists among its internals.
    let facts: Facts = vec![
        (
            "Clips".into(),
            or_anim::clip::clips(dict).count().to_string(),
        ),
        ("Animations".into(), animations.to_string()),
    ];
    draw_root_summary(ui, state, doc_id, "ycd_root", facts);

    let Some(payload) = state.document(doc_id).map(|doc| &doc.payload) else {
        return;
    };
    let DocPayload::Resource(or_io::Asset::Clips(dict)) = payload else {
        return;
    };
    let Some(key) = selection.and_then(|sel| ycd_selected_clip_key(payload, sel)) else {
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("a clip"));
        return;
    };
    let Some(clip) = or_anim::clip::clip(dict, key) else {
        return;
    };
    let head = header(clip);

    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.heading(if head.name.data.is_empty() {
        hash_text(key, None)
    } else {
        head.name.data.clone()
    });
    ui.separator();

    grid(ui, "ycd_clip", |ui| {
        row(ui, "Key", &hash_text(key, state.name_index.name_of(key)));
        row(
            ui,
            "Type",
            match clip {
                CrClipAny::CrClipAnimation(_) => "single animation",
                CrClipAny::CrClipAnimations(_) => "animation list",
                CrClipAny::Base(_) => "unknown",
            },
        );
        row(
            ui,
            "Duration",
            &format!("{:.4} s", or_anim::clip::duration(clip)),
        );
        row(ui, "Looped", &or_anim::clip::is_looped(clip).to_string());
        row(ui, "Flags", &format!("{:#04X}", head.flags));
    });

    if let CrClipAny::CrClipAnimation(single) = clip {
        grid(ui, "ycd_clip_window", |ui| {
            row(ui, "Start time", &format!("{:.4} s", single.start_time));
            row(ui, "End time", &format!("{:.4} s", single.end_time));
            row(ui, "Rate", &format!("{:.4}", single.rate));
        });
    }

    if let Some(animation) = or_anim::clip::primary(dict, clip) {
        ui.add_space(crate::ui::theme::SPACING_SNUG);
        ui.label(egui::RichText::new("Animation").strong());
        grid(ui, "ycd_anim", |ui| {
            if !animation.name.is_empty() {
                row(ui, "Name", &animation.name);
            }
            row(ui, "Frames", &animation.num_frames.to_string());
            row(
                ui,
                "Frames per chunk",
                &animation.frames_per_chunk.to_string(),
            );
            row(ui, "Duration", &format!("{:.4} s", animation.duration));
            row(ui, "Blocks", &animation.blocks.len().to_string());
            row(ui, "DOFs", &animation.dofs.len().to_string());
            row(
                ui,
                "Bones",
                &or_anim::clip::bone_ids(animation).len().to_string(),
            );
            row(ui, "Signature", &format!("{:#010X}", animation.signature));
            row(ui, "Flags", &format!("{:#06X}", animation.flags));
            if or_anim::clip::is_compact(animation) {
                row(ui, "Compact", "yes — cannot be decoded");
            }
        });

        if !animation.dofs.is_empty() {
            ui.add_space(crate::ui::theme::SPACING_SNUG);
            egui::CollapsingHeader::new(format!("Degrees of freedom ({})", animation.dofs.len()))
                .default_open(false)
                .show(ui, |ui| {
                    grid(ui, "ycd_dofs", |ui| {
                        for (i, dof) in animation.dofs.iter().take(64).enumerate() {
                            row(
                                ui,
                                &format!("{i}"),
                                &format!(
                                    "bone {:#06X}, {}, {}",
                                    dof.id,
                                    track_name(dof.track),
                                    format_name(dof.r#type)
                                ),
                            );
                        }
                    });
                    if animation.dofs.len() > 64 {
                        ui.label(
                            egui::RichText::new(format!(
                                "… and {} more",
                                animation.dofs.len() - 64
                            ))
                            .small()
                            .color(theme::TEXT_SECONDARY),
                        );
                    }
                });
        }
    }

    if let Some(tags) = head.tags.as_deref() {
        ui.add_space(crate::ui::theme::SPACING_SNUG);
        egui::CollapsingHeader::new(format!("Tags ({})", tags.tags.len()))
            .default_open(false)
            .show(ui, |ui| {
                grid(ui, "ycd_tags", |ui| {
                    row(ui, "Has block tags", &tags.has_block_tags.get().to_string());
                    for (i, tag) in tags.tags.iter().flatten().enumerate() {
                        row(
                            ui,
                            &format!("{i}"),
                            &format!(
                                "{:#010X}  phase {:.3}..{:.3}  ({} attrs)",
                                tag.base.name.key,
                                tag.start,
                                tag.end,
                                tag.base.attributes.len()
                            ),
                        );
                    }
                });
            });
    }

    if let Some(properties) = head.properties.as_deref() {
        let properties: Vec<_> = properties
            .properties
            .entries()
            .filter_map(|e| Some((e.key, e.data.as_deref()?)))
            .collect();
        ui.add_space(crate::ui::theme::SPACING_SNUG);
        egui::CollapsingHeader::new(format!("Properties ({})", properties.len()))
            .default_open(false)
            .show(ui, |ui| {
                grid(ui, "ycd_props", |ui| {
                    for (key, property) in &properties {
                        row(
                            ui,
                            &hash_text(*key, None),
                            &format!("{} attributes", property.attributes.len()),
                        );
                    }
                });
            });
    }
}
