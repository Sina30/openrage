//! The rows of the Wardrobe's list: a slot's heading, which picks what is under it and opens a menu of what can be done to it, and a garment's row, which is one click target.

use std::collections::HashMap;
use std::path::PathBuf;

use egui_phosphor::regular as icon;
use or_wardrobe::search;

use super::list::{Asked, Click, Frame, Heading, Line, View, ROW_HEIGHT};
use super::words::cut_in_the_middle;
use super::{menus, preview};
use crate::state::front_door::counted;
use crate::state::workbench::{in_pack, Severity};
use crate::ui::theme;

/// A row shows how many textures it has and whether it has cloth only where the panel is wide enough to hold that beside the name.
const WIDE_ROW: f32 = 240.0;

/// The side of the picture of a row's first texture, in points.
const ROW_PICTURE: f32 = 16.0;

/// The room of the mark that says a garment has a problem, kept whether it has one or not so that the places below it line up.
const MARK: f32 = 14.0;

pub(super) fn rows(
    ui: &mut egui::Ui,
    frame: &Frame,
    tiles: &mut HashMap<u64, egui::TextureHandle>,
    view: &mut View,
    asked: &mut Asked,
) {
    let (read, cached) = (frame.read, frame.cached);
    if cached.lines.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(theme::SPACING_RELAXED);
            if read.wardrobe.garments.is_empty() {
                ui.label(egui::RichText::new(tr!("No garments yet")).color(theme::TEXT_SECONDARY));
                return;
            }
            // ⚠ A filter that hides everything says what is on and lets it go, so that an empty list is never a dead end.
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!(
                        "No garment matches what is on: {filters}",
                        filters = frame.filters
                    ))
                    .color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
            if ui
                .button(tr!("Clear filters"))
                .on_hover_text(tr!(
                    "Let go of the text, the ped and the chip, so every garment is listed again"
                ))
                .clicked()
            {
                asked.clear = true;
            }
        });
        return;
    }
    // ⚠ No minimum height: below it the list would run on under the block along the bottom of the column.
    let mut area = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .min_scrolled_height(0.0);
    if let Some(id) = view.scroll_to.take() {
        let line = cached.lines.iter().position(
            |line| matches!(line, Line::Garment(i) if read.wardrobe.garments[*i].id == id),
        );
        // ⚠ Only a garment at the edge of what is drawn or past it: one in the middle of the screen stays where it is.
        if let Some(line) = line.filter(|l| *l <= view.shown.start || l + 1 >= view.shown.end) {
            let pitch = ROW_HEIGHT + ui.spacing().item_spacing.y;
            area = area.vertical_scroll_offset(line.saturating_sub(2) as f32 * pitch);
        }
    }
    let mut seen = 0..0;
    area.show_rows(ui, ROW_HEIGHT, cached.lines.len(), |ui, range| {
        seen = range.clone();
        for (at, line) in cached.lines[range.clone()].iter().enumerate() {
            match line {
                Line::Group(heading) => {
                    let under = &cached.lines[range.start + at + 1..];
                    group_heading(ui, frame, heading, under, asked);
                }
                Line::Garment(i) => garment_row(ui, frame, *i, tiles, asked),
            }
        }
    });
    view.shown = seen;
}

/// A slot's heading: a click picks every garment under it, a right click does too unless they are picked already, and the button at its end opens the menu a right click opens.
fn group_heading(
    ui: &mut egui::Ui,
    frame: &Frame,
    heading: &Heading,
    under: &[Line],
    asked: &mut Asked,
) {
    let ids: Vec<String> = (under.iter())
        .map_while(|line| match line {
            Line::Garment(i) => Some(frame.read.wardrobe.garments[*i].id.clone()),
            Line::Group(..) => None,
        })
        .collect();
    // ⚠ The button at the end of a heading picks what it acts on, as a right click does, so that the highlight, the Garment panel and the menu name the same garments.
    let mut opened = false;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, 2.0, theme::BG_L3);
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let (button, _) = egui::containers::menu::MenuButton::from_button(
            egui::Button::new(icon::DOTS_THREE).frame(false),
        )
        .ui(&mut row, |ui| {
            menus::selection(ui, ids.clone(), frame, heading.seat, asked);
        });
        button.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                tr!("Actions for {words}", words = heading.words),
            )
        });
        opened = button.clicked();
        button.on_hover_text(tr!(
            "What can be done to the garments under this heading, all at once"
        ));
        row.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&heading.words)
                        .strong()
                        .color(theme::TEXT_SECONDARY),
                )
                .truncate(),
            );
        });
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, heading.words.as_str())
    });
    let all_picked = ids.iter().all(|id| frame.picked.contains(id));
    if response.clicked() || ((response.secondary_clicked() || opened) && !all_picked) {
        asked.heading = Some(ids.clone());
    }
    response
        .on_hover_text(&heading.tip)
        .context_menu(|ui| menus::selection(ui, ids.clone(), frame, heading.seat, asked));
}

/// A garment's row: the whole row is the click target, a right click picks it too, and the menu opens on it.
fn garment_row(
    ui: &mut egui::Ui,
    frame: &Frame,
    index: usize,
    tiles: &mut HashMap<u64, egui::TextureHandle>,
    asked: &mut Asked,
) {
    let (read, cached) = (frame.read, frame.cached);
    let g = &read.wardrobe.garments[index];
    let unread = g.ped.is_none() || g.slot.is_none();
    let place = read.allocation.of(index).map(|p| format!("{:03}", p.place));
    let on = frame.picked.contains(&g.id);
    let problem = cached.problems.get(&g.id);
    let name = &cached.labels[index];
    let mut detail = counted(g.variations.len(), "texture");
    if g.cloth.is_some() {
        detail.push_str(tr!(" · cloth"));
    }
    if g.is_reserved() {
        detail.push_str(tr!(" · held"));
    }
    let files: Vec<PathBuf> = (g.variations.iter())
        .filter_map(|v| v.texture.as_ref())
        .filter_map(|t| in_pack(frame.folder, t))
        .collect();

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        asked.thumbs.extend(files.first().cloned());
        let fill = match (on, response.hovered()) {
            (true, _) => theme::SELECTION_BG.get(),
            (false, true) => theme::BG_L3.get(),
            (false, false) => egui::Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(rect, 2.0, fill);
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if rect.width() >= WIDE_ROW {
            row.label(
                egui::RichText::new(&detail)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        row.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // A shape and a colour: a state that rests on a colour alone is lost to somebody.
            let mark = match problem.map(|p| p.worst) {
                Some(Severity::Error) => egui::RichText::new(icon::X_CIRCLE).color(theme::ERROR),
                Some(Severity::Warning) => egui::RichText::new(icon::WARNING).color(theme::WARNING),
                None => egui::RichText::new(""),
            };
            ui.add_sized([MARK, ROW_HEIGHT], egui::Label::new(mark));
            if unread {
                ui.label(egui::RichText::new("?").color(theme::WARNING));
            }
            ui.label(
                egui::RichText::new(place.as_deref().unwrap_or("···"))
                    .monospace()
                    .color(theme::TEXT_SECONDARY),
            );
            if g.position.is_some() {
                ui.label(icon::PUSH_PIN);
            }
            row_picture(ui, frame, files.first(), tiles);
            // ⚠ Cut in the middle: the end of a name is what tells one row from another.
            let room = ui.available_width();
            let font = egui::TextStyle::Body.resolve(ui.style());
            let shown = cut_in_the_middle(name, |text| {
                let galley = ui.painter().layout_no_wrap(
                    text.to_string(),
                    font.clone(),
                    egui::Color32::WHITE,
                );
                galley.size().x <= room
            });
            ui.add(egui::Label::new(shown).wrap_mode(egui::TextWrapMode::Extend));
        });
    }
    let said = match (&place, problem) {
        (Some(place), Some(p)) => {
            tr!(
                "{name}, place {place}, {detail}, {count}",
                name = name,
                place = place,
                detail = detail,
                count = counted(p.count, "problem")
            )
        }
        (Some(place), None) => tr!(
            "{name}, place {place}, {detail}",
            name = name,
            place = place,
            detail = detail
        ),
        (None, _) => tr!(
            "{name}, no place yet, {detail}",
            name = name,
            detail = detail
        ),
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &said)
    });
    if response.hovered() {
        asked.thumbs.extend(files.iter().skip(1).cloned());
    }
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| hover(ui, frame, index, &said, &files, tiles));
    if response.clicked() {
        let held = ui.input(|i| i.modifiers);
        let click = match (held.shift, held.command || held.ctrl) {
            (true, _) => Click::Span,
            (false, true) => Click::Toggle,
            (false, false) => Click::Only,
        };
        asked.choose = Some((click, g.id.clone()));
    } else if response.secondary_clicked() && !on {
        // A right click on a garment that is picked leaves the pick whole, so the menu acts on all of it.
        asked.choose = Some((Click::Only, g.id.clone()));
    }
    response.context_menu(|ui| match on && frame.picked.len() > 1 {
        true => menus::selection(ui, frame.picked.to_vec(), frame, None, asked),
        false => menus::row(ui, frame, index, place.is_some(), asked),
    });
}

/// The picture of a row's first texture in its place before the name, a faint square while it is read.
fn row_picture(
    ui: &mut egui::Ui,
    frame: &Frame,
    file: Option<&PathBuf>,
    tiles: &mut HashMap<u64, egui::TextureHandle>,
) {
    let (square, _) = ui.allocate_exact_size(egui::Vec2::splat(ROW_PICTURE), egui::Sense::hover());
    let Some(file) = file else { return };
    match frame.dressed.thumb(file) {
        Some(thumb) => {
            let [w, h] = thumb.size;
            let fit = egui::vec2(w as f32, h as f32) * (ROW_PICTURE / w.max(h).max(1) as f32);
            ui.painter().image(
                preview::tile_texture(ui.ctx(), tiles, thumb),
                egui::Rect::from_center_size(square.center(), fit),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            ui.painter().rect_filled(square, 2.0, theme::BG_L1);
        }
    }
}

/// What resting on a row shows: its name, the first thing wrong with it, the name the game gives its model and each of its textures.
fn hover(
    ui: &mut egui::Ui,
    frame: &Frame,
    index: usize,
    said: &str,
    files: &[PathBuf],
    tiles: &mut HashMap<u64, egui::TextureHandle>,
) {
    let read = frame.read;
    ui.label(egui::RichText::new(said).strong());
    let g = &read.wardrobe.garments[index];
    if let Some(problem) = frame.cached.problems.get(&g.id) {
        let colour = match problem.worst {
            Severity::Error => theme::ERROR,
            Severity::Warning => theme::WARNING,
        };
        ui.add(egui::Label::new(egui::RichText::new(&problem.first).small().color(colour)).wrap());
    }
    if let Some(game) = search::game_name(&read.wardrobe, &read.allocation, index) {
        ui.label(
            egui::RichText::new(game)
                .small()
                .monospace()
                .color(theme::TEXT_SECONDARY),
        );
    }
    let filed: Vec<String> = (g
        .group
        .iter()
        .map(|group| tr!("Group: {group}", group = group)))
    .chain((!g.tags.is_empty()).then(|| tr!("Tags: {tags}", tags = g.tags.join(", "))))
    .collect();
    if !filed.is_empty() {
        ui.label(
            egui::RichText::new(filed.join(" · "))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
    if files.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        for (i, file) in files.iter().take(8).enumerate() {
            let letter = char::from(b'a' + u8::try_from(i).unwrap_or(25).min(25));
            let leaf = file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            match frame.dressed.thumb(file) {
                Some(thumb) => {
                    let [w, h] = thumb.size;
                    let fit = egui::vec2(w as f32, h as f32) * (64.0 / w.max(h).max(1) as f32);
                    let said = tr!(
                        "Texture {letter}, {leaf}, {width} by {height}",
                        letter = letter,
                        leaf = leaf,
                        width = thumb.full[0],
                        height = thumb.full[1]
                    );
                    ui.add(
                        egui::Image::new((preview::tile_texture(ui.ctx(), tiles, thumb), fit))
                            .alt_text(said),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new(tr!("Texture {letter}: reading…", letter = letter))
                            .small()
                            .color(theme::TEXT_SECONDARY),
                    );
                }
            }
        }
        if files.len() > 8 {
            ui.label(
                egui::RichText::new(tr!("and {count} more", count = files.len() - 8))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
}
