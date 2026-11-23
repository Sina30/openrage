//! The quick look of a garment: its texture and its facts, docked under the list, without opening it.
//!
//! ⚠ Inside the Wardrobe's panel and never a window over the screen, as the Assets table's is. The picture is read off the UI
//! thread by the same `Glance` the Assets table uses, and Space, Escape and the close button take it down.

use std::path::PathBuf;

use super::preview;
use super::words::{ped_words, slot_words};
use super::WardrobeUi;
use crate::state::front_door::counted;
use crate::state::workbench::{in_pack, Glimpse, Place, Severity, Thumb, Want};
use crate::state::AppState;
use crate::ui::theme;

/// The tallest picture, in points.
const PICTURE: f32 = 140.0;

/// What the quick look says of the garment selected, taken out of the pack before the picture is asked for.
struct About {
    name: String,
    game: Option<String>,
    seat: String,
    textures: usize,
    worn: usize,
    file: Option<PathBuf>,
    group: Option<String>,
    tags: Vec<String>,
    problems: Vec<(Severity, String)>,
}

fn about(state: &AppState) -> Option<About> {
    let (pack, at) = state.selected_garment()?;
    let read = pack.read.as_ref().ok()?;
    let g = &read.wardrobe.garments[at];
    let worn = state.dressed_variation(&g.id);
    let seat = match (g.ped, g.slot, read.allocation.of(at)) {
        (Some(ped), Some(slot), Some(place)) => tr!(
            "Place {place} · {slot} · {ped}",
            place = format!("{:03}", place.position),
            slot = slot_words(slot),
            ped = ped_words(Some(ped))
        ),
        _ => tr!("No place yet: a ped or a slot is unsaid").to_string(),
    };
    let problems = (state.workbench.findings().iter())
        .filter(|f| {
            matches!(&f.place, Place::Garment { content, id: Some(id), .. }
                if *content == pack.module && *id == g.id)
        })
        .map(|f| (f.severity, f.text.clone()))
        .collect();
    Some(About {
        name: or_wardrobe::rules::name_of(g),
        game: or_wardrobe::search::game_name(&read.wardrobe, &read.allocation, at),
        seat,
        textures: g.variations.len(),
        worn,
        file: (g.variations.get(worn))
            .and_then(|v| v.texture.as_ref())
            .and_then(|t| in_pack(&pack.folder, t)),
        group: g.group.clone(),
        tags: g.tags.clone(),
        problems,
    })
}

pub(super) fn draw(ui: &mut egui::Ui, state: &mut AppState, shown: &mut WardrobeUi) {
    let Some(about) = about(state) else {
        state.workbench.wardrobe.quick_look = false;
        return;
    };
    let crypto = state.keys.provider();
    let want = about.file.clone().and_then(Want::of_file);
    let glance = &mut state.workbench.wardrobe.glance;
    let picture: Option<Result<std::sync::Arc<Thumb>, String>> = match glance.look(want, &crypto) {
        Glimpse::Nothing => None,
        Glimpse::Reading => Some(Err(String::new())),
        Glimpse::Picture(thumb) => Some(Ok(std::sync::Arc::clone(thumb))),
        Glimpse::Cannot(why) => Some(Err(why.to_string())),
    };
    if glance.is_reading() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    let mut open = true;
    ui.horizontal(|ui| {
        // ⚠ The button first, from the right, and the name cut to what is left: a long name must not push the button out of the panel.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui.small_button(egui_phosphor::regular::X);
            close.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Close quick look"))
            });
            if close
                .on_hover_text(tr!("Close the quick look (Space)"))
                .clicked()
            {
                open = false;
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let title =
                    egui::RichText::new(tr!("Quick look: {name}", name = about.name)).strong();
                ui.add(egui::Label::new(title).truncate());
            });
        });
    });
    egui::ScrollArea::vertical()
        .id_salt("wardrobe_quick_look_body")
        .max_height(PICTURE + 24.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                picture_of(ui, shown, &about, picture.as_ref());
                ui.vertical(|ui| facts(ui, &about));
            });
        });
    if !open || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        state.workbench.wardrobe.quick_look = false;
    }
}

fn picture_of(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    about: &About,
    picture: Option<&Result<std::sync::Arc<Thumb>, String>>,
) {
    match picture {
        None => {
            ui.label(
                egui::RichText::new(tr!("This garment has no texture to draw"))
                    .color(theme::TEXT_SECONDARY),
            );
        }
        Some(Err(why)) if why.is_empty() => {
            ui.spinner();
        }
        Some(Err(why)) => {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("The texture cannot be drawn: {why}", why = why))
                        .color(theme::WARNING),
                )
                .wrap(),
            );
        }
        Some(Ok(thumb)) => {
            let [w, h] = thumb.size;
            let fit = egui::vec2(w as f32, h as f32) * (PICTURE / h.max(w).max(1) as f32);
            // A textures source is named by the file a build cooks it to.
            let file =
                about
                    .file
                    .as_ref()
                    .and_then(|f| f.file_name())
                    .map_or_else(String::new, |n| {
                        let n = n.to_string_lossy();
                        n.strip_suffix(".textures.src")
                            .map_or_else(|| n.to_string(), |stem| format!("{stem}.ytd"))
                    });
            let said = tr!(
                "Picture of {file}, {width} by {height}",
                file = file,
                width = thumb.full[0],
                height = thumb.full[1]
            );
            let texture = preview::tile_texture(ui.ctx(), &mut shown.tiles, thumb);
            ui.add(egui::Image::new((texture, fit)).alt_text(said));
        }
    }
}

fn facts(ui: &mut egui::Ui, about: &About) {
    let quiet = |text: String| {
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY)
    };
    if let Some(game) = &about.game {
        ui.label(quiet(game.clone()).monospace());
    }
    ui.label(quiet(about.seat.clone()));
    let letter = char::from(b'a' + u8::try_from(about.worn).unwrap_or(25).min(25));
    ui.label(quiet(tr!(
        "{textures} · texture {letter} shown",
        letter = letter,
        textures = counted(about.textures, "texture")
    )));
    if let Some(group) = &about.group {
        ui.label(quiet(tr!("Group: {group}", group = group)));
    }
    if !about.tags.is_empty() {
        ui.label(quiet(tr!("Tags: {tags}", tags = about.tags.join(", "))));
    }
    for (severity, text) in &about.problems {
        let colour = match severity {
            Severity::Error => theme::ERROR,
            Severity::Warning => theme::WARNING,
        };
        ui.add(egui::Label::new(egui::RichText::new(text).color(colour)).wrap());
    }
}
