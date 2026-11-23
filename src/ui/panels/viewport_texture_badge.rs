//! The viewport's texture-resolution badge: which dictionaries answered, which
//! are pinned, and what can be attached.

use crate::render::RenderBridge;
use crate::state::{AppState, DocumentId, DocumentKind, TextureResolutionReport};
use crate::ui::theme;

#[derive(Debug, Clone, Copy)]
pub(super) enum TextureBadgeAction {
    Pin(DocumentId),
    Unpin(DocumentId),
    ClearPins,
    AttachFromDisk,
    Detach(DocumentId),
}

pub(super) fn apply_texture_badge_action(
    state: &mut AppState,
    bridge: &mut RenderBridge,
    action: TextureBadgeAction,
) {
    // ⚠ Only the pins name documents, the ones the viewport's textures are asked for. An
    // attachment is session-wide, so it must not wait for there to be one.
    match action {
        TextureBadgeAction::AttachFromDisk => {
            let Some(path) = crate::platform::file_dialogs::pick_texture_dictionary() else {
                return;
            };
            match state.attach_texture_dictionary_from_disk(&path) {
                Ok(_) => state.show_toast(format!("Attached {}", path.display())),
                Err(message) => state.show_error_toast(message),
            }
        }
        TextureBadgeAction::Detach(dictionary) => state.detach_texture_dictionary(dictionary),
        TextureBadgeAction::Pin(dictionary) => state.pin_texture_dictionary_in_view(dictionary),
        TextureBadgeAction::Unpin(dictionary) => state.unpin_texture_dictionary_in_view(dictionary),
        TextureBadgeAction::ClearPins => state.clear_texture_pins_in_view(),
    }
    bridge.invalidate_texture_resolution();
}

pub(super) fn draw_texture_badge(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    state: &AppState,
) -> Option<TextureBadgeAction> {
    let report = &state.texture_report;
    if report.is_empty() || report.document != state.active_document {
        return None;
    }

    let id = ui.id().with("viewport_texture_badge");
    let mut expanded = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));

    let clean = report.is_clean();
    let accent = if clean {
        theme::TEXT_SECONDARY
    } else {
        theme::WARNING
    };

    let backdrop = ui.painter().add(egui::Shape::Noop);
    let origin = rect.min + egui::vec2(8.0, 8.0);
    let max_rect = egui::Rect::from_min_max(
        origin,
        egui::pos2(
            (origin.x + if expanded { 320.0 } else { 118.0 }).min(rect.right() - 8.0),
            rect.bottom() - 8.0,
        ),
    );
    if max_rect.width() < 60.0 {
        return None;
    }

    let mut action = None;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(max_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 2.0;

    let header = format!("\u{25A4} {}", report.summary());
    let chevron = if expanded { "\u{25B4}" } else { "\u{25BE}" };
    let toggle = child.add(
        egui::Label::new(
            egui::RichText::new(format!("{header}  {chevron}"))
                .monospace()
                .size(11.0)
                .color(accent),
        )
        .sense(egui::Sense::click()),
    );
    if toggle.clicked() {
        expanded = !expanded;
        ui.data_mut(|d| d.insert_temp(id, expanded));
    }
    let _ = toggle.on_hover_text(
        "Where the viewport's material textures came from.\nClick for the per-texture breakdown.",
    );

    match report.primary_source() {
        Some(origin) if !expanded => {
            child.label(
                egui::RichText::new(elide_middle(&origin.label, 18))
                    .monospace()
                    .size(10.0)
                    .color(theme::TEXT_SECONDARY),
            );
        }
        None if !expanded => {
            child.label(
                egui::RichText::new("no source")
                    .monospace()
                    .size(10.0)
                    .color(theme::WARNING),
            );
        }
        _ => {}
    }

    if expanded {
        action = draw_texture_badge_body(&mut child, state, report);
    }

    let used = child.min_rect().expand(6.0);
    ui.painter().set(
        backdrop,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(used, theme::RADIUS_MEDIUM, theme::overlay_bg()),
            egui::Shape::rect_stroke(
                used,
                6.0,
                egui::Stroke::new(1.0_f32, if clean { theme::BORDER } else { accent }),
                egui::StrokeKind::Inside,
            ),
        ]),
    );
    action
}

fn draw_texture_badge_body(
    ui: &mut egui::Ui,
    state: &AppState,
    report: &TextureResolutionReport,
) -> Option<TextureBadgeAction> {
    let mut action;
    let pins = state.texture_pins_in_view();

    ui.add_space(theme::SPACING_HAIR);
    let source = match report.primary_source() {
        Some(origin) => format!(
            "source  {} \u{2192} {}",
            if pins.is_empty() { "Auto" } else { "Pinned" },
            origin.describe()
        ),
        None => "source  nothing open supplies these".to_string(),
    };
    ui.label(
        egui::RichText::new(source)
            .monospace()
            .size(10.0)
            .color(theme::TEXT_PRIMARY),
    );

    egui::ScrollArea::vertical()
        .max_height(150.0)
        .id_salt("texture-badge-rows")
        .show(ui, |ui| {
            for outcome in &report.outcomes {
                if outcome.found.is_none() {
                    ui.label(
                        egui::RichText::new(format!("\u{2716} {}   not found", outcome.name))
                            .monospace()
                            .size(10.0)
                            .color(theme::ERROR),
                    );
                } else if outcome.is_ambiguous() {
                    let others: Vec<&str> =
                        outcome.also_in.iter().map(|o| o.label.as_str()).collect();
                    ui.label(
                        egui::RichText::new(format!(
                            "\u{26A0} {}   also in {}",
                            outcome.name,
                            others.join(", ")
                        ))
                        .monospace()
                        .size(10.0)
                        .color(theme::WARNING),
                    )
                    .on_hover_text(format!(
                        "Resolved from {}.\nPin a dictionary below to decide this explicitly.",
                        outcome
                            .found
                            .as_ref()
                            .map(|o| o.describe())
                            .unwrap_or_default()
                    ));
                }
            }
        });

    ui.add_space(theme::SPACING_TIGHT);
    ui.separator();
    action = draw_attached_dictionaries(ui, state);

    ui.add_space(theme::SPACING_TIGHT);
    ui.separator();
    ui.label(
        egui::RichText::new("Pin a dictionary")
            .monospace()
            .size(10.0)
            .color(theme::TEXT_SECONDARY),
    );

    let mut offered = 0usize;
    for doc in &state.documents {
        if doc.payload.kind() != DocumentKind::Ytd {
            continue;
        }
        offered += 1;
        let pinned = is_pinned_in_view(state, doc.id);
        let label = doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| doc.name.clone());
        let mut checked = pinned;
        if ui
            .checkbox(
                &mut checked,
                egui::RichText::new(elide_middle(&label, 34))
                    .monospace()
                    .size(10.0),
            )
            .changed()
        {
            action = Some(if checked {
                TextureBadgeAction::Pin(doc.id)
            } else {
                TextureBadgeAction::Unpin(doc.id)
            });
        }
    }
    if offered == 0 {
        ui.label(
            egui::RichText::new("no texture dictionary open")
                .monospace()
                .size(10.0)
                .color(theme::TEXT_SECONDARY),
        );
    }
    if !pins.is_empty() && ui.small_button("Back to Auto").clicked() {
        action = Some(TextureBadgeAction::ClearPins);
    }
    action
}

/// The dictionaries the user attached by hand, each removable on its own.
///
/// ⚠ A different list from the search paths in Settings, against a different
/// noun: that one orders *folders* to look in for any name, this one names
/// one dictionary to consult. Collapsing them would lose the ordering the
/// folder list exists for.
fn draw_attached_dictionaries(ui: &mut egui::Ui, state: &AppState) -> Option<TextureBadgeAction> {
    let mut action = None;

    ui.label(
        egui::RichText::new("Attached dictionaries")
            .monospace()
            .size(10.0)
            .color(theme::TEXT_SECONDARY),
    );

    for attached in state.attached_texture_dictionaries() {
        let label = state
            .document(attached.document)
            .map(|doc| {
                doc.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| doc.name.clone())
            })
            .unwrap_or_else(|| format!("#{}", attached.document));
        ui.horizontal(|ui| {
            if ui
                .small_button("\u{2716}")
                .on_hover_text(if attached.opened_by_attach {
                    "Detach and close it again"
                } else {
                    "Detach it. The document stays open."
                })
                .clicked()
            {
                action = Some(TextureBadgeAction::Detach(attached.document));
            }
            ui.label(
                egui::RichText::new(elide_middle(&label, 30))
                    .monospace()
                    .size(10.0)
                    .color(theme::TEXT_PRIMARY),
            );
        });
    }

    if state.attached_texture_dictionaries().is_empty() {
        ui.label(
            egui::RichText::new("none attached")
                .monospace()
                .size(10.0)
                .color(theme::TEXT_SECONDARY),
        );
    }

    if ui
        .small_button("Attach .ytd\u{2026}")
        .on_hover_text(
            "Consult one dictionary for every document's textures. Right-click a \
             .ytd inside an open archive to attach that one instead.",
        )
        .clicked()
    {
        action = Some(TextureBadgeAction::AttachFromDisk);
    }

    action
}

fn elide_middle(s: &str, max: usize) -> String {
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1) / 2;
    let prefix: String = s.chars().take(keep).collect();
    let suffix: String = s.chars().skip(len.saturating_sub(keep)).collect();
    format!("{prefix}\u{2026}{suffix}")
}

/// Whether `dictionary` is pinned for every document the viewport's textures are asked for, which is what a box ticked for it says; `texture_pins_in_view` says it is pinned for any.
pub(crate) fn is_pinned_in_view(state: &AppState, dictionary: DocumentId) -> bool {
    let documents = state.textured_documents();
    let pinned = |doc: &DocumentId| {
        state
            .pinned_texture_dictionaries(*doc)
            .contains(&dictionary)
    };
    !documents.is_empty() && documents.iter().all(pinned)
}
