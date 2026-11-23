//! The menus of the list: one for a garment on its own, and one for garments picked together, which a heading's is as well.

use or_wardrobe::edit::Toward;
use or_wardrobe::rules::name_of;
use or_wardrobe::{search, Garment, Ped, Slot};

use super::list::{Asked, Frame, GroupAsk, PlaceAsk};
use super::words::{ped_words, slot_words};
use crate::state::front_door::counted;
use crate::state::workbench::in_pack;
use crate::ui::theme;

/// The menu of the garment at `index`: what can be copied of it, where its file is, and what can be done to it.
pub(super) fn row(ui: &mut egui::Ui, frame: &Frame, index: usize, placed: bool, asked: &mut Asked) {
    crate::ui::popup_of_the_shell(ui.ctx());
    let (read, linked) = (frame.read, frame.linked);
    let g = &read.wardrobe.garments[index];
    let game = search::game_name(&read.wardrobe, &read.allocation, index);
    let file = (g.model.as_ref()).and_then(|m| in_pack(frame.folder, m));
    let close = |ui: &mut egui::Ui| ui.close_kind(egui::UiKind::Menu);

    if ui.button(tr!("Copy name")).clicked() {
        asked.copy = Some((tr!("the name"), name_of(g)));
        close(ui);
    }
    if ui
        .add_enabled(game.is_some(), egui::Button::new(tr!("Copy game name")))
        .on_disabled_hover_text(tr!(
            "A garment with no model and no place has no name in the game"
        ))
        .clicked()
    {
        asked.copy = game.map(|game| (tr!("the game name"), game));
        close(ui);
    }
    if ui
        .add_enabled(file.is_some(), egui::Button::new(tr!("Copy path")))
        .on_disabled_hover_text(tr!("This garment has no model file inside the pack"))
        .clicked()
    {
        asked.copy = file
            .as_ref()
            .map(|file| (tr!("the path"), file.display().to_string()));
        close(ui);
    }
    if ui
        .add_enabled(
            file.is_some(),
            egui::Button::new(tr!("Reveal in the file manager")),
        )
        .on_disabled_hover_text(tr!("This garment has no model file inside the pack"))
        .clicked()
    {
        asked.reveal = file;
        close(ui);
    }
    if ui
        .add_enabled(g.model.is_some(), egui::Button::new(tr!("Open model in place")))
        .on_hover_text(tr!("Looks at the model where it is, in the Model workspace, and changes nothing in the project; Enter does the same. Add clothes… imports instead: it decompiles a game file into a source of the project"))
        .on_disabled_hover_text(tr!("This place is held with no model, so there is nothing to open"))
        .clicked()
    {
        asked.open_model = Some(g.id.clone());
        close(ui);
    }
    ui.separator();

    if ui
        .add_enabled(
            !linked && g.position.is_some(),
            egui::Button::new(tr!("Unpin: take the lowest free place")),
        )
        .clicked()
    {
        asked.unpin = Some(g.id.clone());
        close(ui);
    }
    for (toward, words) in [
        (Toward::Earlier, tr!("Move earlier in its slot")),
        (Toward::Later, tr!("Move later in its slot")),
    ] {
        if ui
            .add_enabled(!linked && placed, egui::Button::new(words))
            .clicked()
        {
            asked.moved = Some((g.id.clone(), toward));
            close(ui);
        }
    }
    if let Some(ped) = g.ped {
        let words = tr!(
            "Duplicate to the {ped} ped",
            ped = ped_words(Some(ped.other())).to_lowercase()
        );
        if ui.add_enabled(!linked, egui::Button::new(words)).clicked() {
            asked.duplicate = Some((g.id.clone(), ped.other()));
            close(ui);
        }
    }
    if ui
        .add_enabled(
            !linked,
            egui::Button::new(tr!("Remove from the pack (its files stay)")),
        )
        .clicked()
    {
        asked.remove = Some(g.id.clone());
        close(ui);
    }
}

/// What a slot's heading offers for its places: the gaps in its numbering filled with empty places, and its empty places moved after its garments.
fn places(ui: &mut egui::Ui, (ped, slot): (Ped, Slot), frame: &Frame, asked: &mut Asked) {
    let empty = or_wardrobe::dummies::count(&frame.read.wardrobe, ped, slot);
    for (ask, words, hover, none) in [
        (
            PlaceAsk::Fill,
            tr!("Fill the gaps in the numbering with empty places"),
            tr!("Every number below the last garment of a collection that nothing holds becomes an empty place, a drawable with no model that holds the number"),
            None,
        ),
        (
            PlaceAsk::Sort,
            tr!("Move the empty places to the end"),
            tr!("The garments with a model take the lowest places and the empty places follow. Each is pinned where it went; a pack that freezes its drawables asks for a reason at the next build"),
            Some(tr!("This slot has no empty places")),
        ),
    ] {
        let absent = none.filter(|_| empty == 0);
        let response = ui
            .add_enabled(!frame.linked && absent.is_none(), egui::Button::new(words))
            .on_hover_text(hover)
            .on_disabled_hover_text(match (frame.linked, absent) {
                (true, _) => tr!("Linked clothing is edited in its own project"),
                (false, Some(why)) => why,
                (false, None) => hover,
            });
        if response.clicked() {
            asked.places = Some((ped, slot, ask));
            ui.close_kind(egui::UiKind::Menu);
        }
    }
}

/// The ped or the slot every garment of a pick has, none where they differ or any has none.
fn shared<T: PartialEq + Copy>(mut each: impl Iterator<Item = Option<T>>) -> Option<T> {
    let first = each.next()??;
    each.all(|one| one == Some(first)).then_some(first)
}

/// Why a menu item that would change nothing is greyed: the ped or the slot is in force already.
fn already(n: usize, what: &str) -> String {
    match n {
        1 => tr!("This garment is already {what}", what = what),
        n => tr!("All {n} are already {what}", n = n, what = what),
    }
}

/// The menu of garments picked together, which a slot's heading offers as well, with the places of that slot: what can be done to every one of them at once, each a single step of Undo.
///
/// ⚠ The first line says what the menu acts on, and an item that would change nothing is greyed with its reason. Removal is last, after a separator, and in the colour of an error: it is the one item here that takes garments out.
pub(super) fn selection(
    ui: &mut egui::Ui,
    ids: Vec<String>,
    frame: &Frame,
    seat: Option<(Ped, Slot)>,
    asked: &mut Asked,
) {
    crate::ui::popup_of_the_shell(ui.ctx());
    let linked = frame.linked;
    let all = counted(ids.len(), "garment");
    let of: Vec<&Garment> = (ids.iter())
        .filter_map(|id| frame.read.wardrobe.garment(id))
        .collect();
    let scope = [
        shared(of.iter().map(|g| g.ped)).map(|ped| ped_words(Some(ped)).to_string()),
        shared(of.iter().map(|g| g.slot)).map(|slot| slot_words(slot).to_string()),
        Some(all.clone()),
    ];
    ui.label(
        egui::RichText::new(scope.into_iter().flatten().collect::<Vec<_>>().join(" · "))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    let mut ask = |ask: GroupAsk, ui: &mut egui::Ui| {
        asked.group = Some((ids.clone(), ask));
        ui.close_kind(egui::UiKind::Menu);
    };
    let say_why = |response: egui::Response, held: Option<String>| match (linked, held) {
        (true, _) => {
            response.on_disabled_hover_text(tr!("Linked clothing is edited in its own project"))
        }
        (false, Some(why)) => response.on_disabled_hover_text(why),
        (false, None) => response,
    };
    for ped in Ped::ALL {
        let words = tr!(
            "Set the ped to {ped} for these {all}",
            all = all,
            ped = ped_words(Some(ped))
        );
        let held = (of.iter().all(|g| g.ped == Some(ped)))
            .then(|| already(ids.len(), ped_words(Some(ped))));
        let item = ui.add_enabled(!linked && held.is_none(), egui::Button::new(words));
        if say_why(item, held).clicked() {
            ask(GroupAsk::Ped(ped), ui);
        }
    }
    ui.add_enabled_ui(!linked, |ui| {
        let menu = ui.menu_button(tr!("Set the slot for these {all}", all = all), |ui| {
            crate::ui::popup_of_the_shell(ui.ctx());
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for slot in Slot::all() {
                        let words = format!("{} ({})", slot_words(slot), slot.name());
                        let held = (of.iter().all(|g| g.slot == Some(slot)))
                            .then(|| already(ids.len(), slot_words(slot)));
                        let item = ui.add_enabled(held.is_none(), egui::Button::new(words));
                        if say_why(item, held).clicked() {
                            ask(GroupAsk::Slot(slot), ui);
                        }
                    }
                });
        });
        say_why(menu.response, None);
    });
    if ui
        .button(tr!("Export the files of these {all}…", all = all))
        .clicked()
    {
        ask(GroupAsk::Export, ui);
    }
    if let Some(seat) = seat {
        ui.separator();
        places(ui, seat, frame, asked);
    }
    ui.separator();
    let remove = egui::Button::new(
        egui::RichText::new(tr!(
            "Remove these {all} from the pack (their files stay)",
            all = all
        ))
        .color(theme::ERROR),
    );
    if say_why(ui.add_enabled(!linked, remove), None).clicked() {
        asked.group = Some((ids, GroupAsk::Remove));
        ui.close_kind(egui::UiKind::Menu);
    }
}
