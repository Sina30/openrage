//! The Garment panel: the garment selected, what is wrong with it and what it states; with several selected, what can be set on all of them; with none, the pack's own settings.

mod place;
mod tags;
mod textures;
mod trouble;

use or_wardrobe::edit::Edit;
use or_wardrobe::{shop, Garment, Ped};

use super::behaviour;
use super::list::GroupAsk;
use super::mesh;
use super::recover::{self, Rollback};
use super::words::ped_words;
use super::WardrobeUi;
use crate::platform::file_dialogs;
use crate::state::front_door::counted;
use crate::state::workbench::{Fix, PackRead, Wanting};
use crate::state::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::theme;

/// What a row of the textures asked for, with the texture's place and the file the row was drawn with.
enum TextureAsk {
    /// Pick the file that takes the texture's place.
    Replace(usize),
    /// Give it the letter at the place.
    Move(usize, Option<String>, usize),
    Remove(usize, Option<String>),
    /// Pick the folder its file is written into.
    Export(usize),
}

/// What a control asked for, carried out once the panel is drawn.
#[derive(Default)]
struct Asked {
    /// What the mesh doctor's controls asked for.
    mesh: mesh::Asked,
    /// Commands that act on the garment selected, run once the panel is drawn.
    run: Vec<Action>,
    /// The garment and what its texture row asked for.
    texture: Option<(String, TextureAsk)>,
    /// The garment and what the buttons of the texture places asked for.
    places: Option<(String, super::texture_places::Ask)>,
    /// The garment, the texture, the file its row was drawn with and the tint chosen; none takes the choice back.
    tint: Option<(String, usize, Option<String>, Option<String>)>,
    edit: Option<(String, Edit)>,
    durty: Option<bool>,
    rename: Option<String>,
    unwaive: Option<usize>,
    fix: Option<Fix>,
    link_cloth: bool,
    unlink_cloth: bool,
    /// The garment, the texture and the id chosen for it; none takes the choice back.
    shop_id: Option<(String, usize, Option<String>)>,
    /// The garment or the texture to put back as an earlier version held it.
    rollback: Option<Rollback>,
    /// What the tags and the group asked for, over every garment picked.
    tags: tags::Asked,
    /// The ped or the slot chosen for every garment picked.
    all: Option<GroupAsk>,
    /// What the behaviour controls asked for.
    behaviour: behaviour::Asked,
    /// What the watched-folder controls asked for.
    watched: super::watched::Asked,
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_clothing() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    ui.ctx()
        .request_repaint_after(crate::state::workbench::RESCAN_AFTER);
    let mut shown = state.panels.take::<WardrobeUi>();
    let mut asked = Asked::default();
    let linked = state.wardrobe_is_linked();
    let picked = state.picked_garments();
    // F2 or a toast asked for a name; it is answered by the panel that has the field: the garment's, or the pack's while none is selected.
    shown.focus_name |= std::mem::take(&mut state.workbench.wardrobe.rename_asked);
    if shown.versions_open {
        state.load_versions();
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if linked {
                ui.label(
                    egui::RichText::new(tr!(
                        "Linked clothing is read here and edited in the project that holds it"
                    ))
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
            }
            ui.add_enabled_ui(!linked, |ui| {
                let selected = state.selected_garment();
                match selected {
                    Some((pack, at)) => match &pack.read {
                        Ok(read) => garment(ui, state, &mut shown, read, at, &picked, &mut asked),
                        Err(why) => {
                            ui.label(egui::RichText::new(why).color(theme::ERROR));
                        }
                    },
                    None => match state.wardrobe_pack().map(|p| &p.read) {
                        Some(Ok(read)) => pack_settings(ui, state, &mut shown, read, &mut asked),
                        Some(Err(why)) => {
                            ui.label(egui::RichText::new(why).color(theme::ERROR));
                        }
                        None => {
                            ui.label(
                                egui::RichText::new(tr!("Reading the pack…"))
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                    },
                }
            });
        });
    state.panels.put(shown);
    if let Some((id, edit)) = asked.edit {
        state.edit_garment(id, edit);
    }
    if let Some((id, edit)) = asked.behaviour.edit.take() {
        state.edit_garment(id, edit);
    }
    for change in std::mem::take(&mut asked.behaviour.sets) {
        state.edit_alternates(change);
    }
    match asked.all {
        Some(GroupAsk::Ped(ped)) => state.set_ped_of(picked.clone(), Some(ped)),
        Some(GroupAsk::Slot(slot)) => state.set_slot_of(picked.clone(), Some(slot)),
        _ => {}
    }
    for tag in asked.tags.add {
        state.tag_garments(picked.clone(), tag);
    }
    if let Some(tag) = asked.tags.remove {
        state.untag_garments(picked.clone(), tag);
    }
    if let Some(group) = asked.tags.group {
        state.group_garments(picked.clone(), group);
    }
    if let Some(on) = asked.durty {
        state.set_durty_hashes(on);
    }
    if let Some(name) = asked.rename {
        state.rename_pack(name);
    }
    if let Some(index) = asked.unwaive {
        state.remove_waiver(index);
    }
    if let Some(fix) = asked.fix {
        state.ask_about_fix(&fix);
    }
    if asked.link_cloth {
        commands::run_action(state, ui.ctx(), Action::LinkCloth);
    }
    if asked.unlink_cloth {
        commands::run_action(state, ui.ctx(), Action::UnlinkCloth);
    }
    if let Some((id, texture, shop_id)) = asked.shop_id {
        state.set_shop_id(id, texture, shop_id);
    }
    for action in asked.run {
        commands::run_action(state, ui.ctx(), action);
    }
    for action in asked.mesh.run {
        commands::run_action(state, ui.ctx(), action);
    }
    if let Some((medium, low, replace)) = asked.mesh.set {
        let desk = &mut state.workbench.wardrobe.mesh;
        (desk.medium, desk.low, desk.replace) = (medium, low, replace);
    }
    if let (Some(shown), Some(before)) = (
        asked.mesh.show_before,
        state.workbench.wardrobe.mesh.before.as_mut(),
    ) {
        before.shown = shown;
    }
    if let Some((id, ask)) = asked.texture {
        match ask {
            TextureAsk::Replace(i) => {
                if let Some(module) = state.wardrobe_module_id() {
                    state.ask_for_a_file(Wanting::Texture {
                        module,
                        garment: id,
                        variation: i,
                    });
                }
            }
            TextureAsk::Move(i, file, to) => state.move_variation(id, i, file, to),
            TextureAsk::Remove(i, file) => state.remove_variation(id, i, file),
            TextureAsk::Export(i) => {
                let picked = file_dialogs::pick_folder(tr!("Export Texture Into Folder"));
                if let Some(folder) = picked {
                    state.export_variation(id, i, folder);
                }
            }
        }
    }
    match asked.places {
        Some((id, super::texture_places::Ask::Hold)) => state.hold_texture_place(id),
        Some((id, super::texture_places::Ask::Last)) => state.sort_texture_places(id),
        None => {}
    }
    if let Some((id, texture, slot)) = asked.watched.bind {
        if let Some(module) = state.wardrobe_module_id() {
            if let Some(folder) = file_dialogs::pick_folder(tr!("Folder The Editor Saves Into")) {
                state.bind_watch_folder(&module, id, texture, slot, folder);
            }
        }
    }
    if let Some(binding) = asked.watched.unbind {
        if let Some(module) = state.wardrobe_module_id() {
            state.unbind_watch_folder(&module, &binding);
        }
    }
    if let Some((id, texture, file, tint)) = asked.tint {
        state.tint_variation(id, texture, file, tint);
    }
    match asked.rollback {
        Some(Rollback::Garment(id, n)) => state.roll_back_garment(id, n),
        Some(Rollback::Texture(id, texture, n)) => state.roll_back_variation(id, texture, n),
        None => {}
    }
}

pub(super) fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::SPACING_DEFAULT);
    ui.label(egui::RichText::new(text).color(theme::TEXT_SECONDARY));
}

fn garment(
    ui: &mut egui::Ui,
    state: &AppState,
    shown: &mut WardrobeUi,
    read: &PackRead,
    at: usize,
    picked: &[String],
    asked: &mut Asked,
) {
    let g = &read.wardrobe.garments[at];
    let id = g.id.clone();
    if picked.len() > 1 {
        several(ui, shown, read, picked, g, asked);
        return;
    }
    name(ui, shown, g, asked);
    trouble::problems(ui, state, shown, &id, asked);

    place::places(ui, read, at, g, asked);
    ui.horizontal_wrapped(|ui| {
        for action in [Action::MoveGarmentEarlier, Action::MoveGarmentLater] {
            if command_button(ui, state, action) {
                asked.run.push(action);
            }
        }
    });

    tags::draw(ui, shown, &[g], &mut asked.tags);

    heading(ui, tr!("Files"));
    match &g.model {
        Some(model) => {
            ui.label(egui::RichText::new(model).small().monospace());
            if !g.alternates.is_empty() {
                ui.label(
                    egui::RichText::new(counted(g.alternates.len(), tr!("alternate model")))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
        }
        None => {
            ui.label(
                egui::RichText::new(tr!("No model: the place is held for a drawable with none"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    }
    ui.horizontal_wrapped(|ui| {
        for action in [
            Action::ReplaceModel,
            Action::UseModelSource,
            Action::RepairModel,
        ] {
            if command_button(ui, state, action) {
                asked.run.push(action);
            }
        }
    });
    mesh::draw(ui, state, g, &mut asked.mesh);
    cloth(ui, state, shown, g, asked);
    behaviour::draw(ui, shown, &read.wardrobe, g, &mut asked.behaviour);

    heading(ui, &tr!("Textures ({count})", count = g.variations.len()));
    for action in [Action::AddTexture, Action::OptimiseTextures] {
        if command_button(ui, state, action) {
            asked.run.push(action);
        }
    }
    super::watched::draw(ui, state, g, &mut asked.watched);
    for i in 0..g.variations.len() {
        textures::row(ui, shown, g, i, asked);
    }
    super::texture_places::draw(ui, g, &mut asked.places);

    shop_ids(ui, shown, read, at, asked);

    ui.add_space(theme::SPACING_DEFAULT);
    recover::garment_versions(ui, state, shown, &id, &mut asked.rollback);

    // ⚠ The garment's one destructive action is apart from the others, on a row of its own, so that nothing is pressed for it by mistake.
    ui.add_space(theme::SPACING_DEFAULT);
    ui.horizontal_wrapped(|ui| {
        for action in [
            Action::DuplicateToOtherPed,
            Action::ExportGarment,
            Action::PackageGarment,
            Action::CreditGarment,
        ] {
            if command_button(ui, state, action) {
                asked.run.push(action);
            }
        }
    });
    ui.separator();
    let remove =
        egui::Button::new(egui::RichText::new(tr!("Remove from the pack")).color(theme::ERROR));
    if ui
        .add(remove)
        .on_hover_text(tr!(
            "The garment leaves wardrobe.json; its files are not touched"
        ))
        .clicked()
    {
        asked.edit = Some((id.clone(), Edit::Remove));
    }
}

/// The garment's name, which is the title of the panel and the field that changes it, with its stable id in the tip.
fn name(ui: &mut egui::Ui, shown: &mut WardrobeUi, g: &Garment, asked: &mut Asked) {
    let derived = g
        .label
        .clone()
        .or_else(|| {
            (g.model.as_deref())
                .and_then(|m| m.rsplit('/').next())
                .map(String::from)
        })
        .unwrap_or_else(|| tr!("A place held").into());
    let id = &g.id;
    // The name people read; the game never sees it. Empty, the garment is called by the file of its model.
    let mut label = match &shown.label {
        Some((who, typed)) if who == id => typed.clone(),
        _ => g.label.clone().unwrap_or_default(),
    };
    let field = ui.add(
        egui::TextEdit::singleline(&mut label)
            .font(egui::FontId::proportional(15.0))
            .hint_text(derived.as_str())
            .desired_width(ui.available_width()),
    );
    field.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Garment name"))
    });
    let field = field.on_hover_text(tr!(
        "{derived}\nStable id {id}. Nothing that points at this garment is broken by a rename, a renumbering or an edit",
        derived = derived,
        id = id.chars().take(8).collect::<String>()
    ));
    if std::mem::take(&mut shown.focus_name) {
        field.request_focus();
    }
    if field.changed() {
        shown.label = Some((id.clone(), label.clone()));
    }
    if field.lost_focus() {
        shown.label = None;
        if Some(label.as_str()) != g.label.as_deref() {
            asked.edit = Some((id.clone(), Edit::Label(Some(label.clone()))));
        }
    }
}

/// Several garments selected: what can be set on all of them, and the one thing that cannot be edited until a single garment is selected.
fn several(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    read: &PackRead,
    picked: &[String],
    g: &Garment,
    asked: &mut Asked,
) {
    let all = counted(picked.len(), "garment");
    ui.label(
        egui::RichText::new(tr!("{all} selected", all = all))
            .strong()
            .size(15.0),
    );
    ui.add(
        egui::Label::new(
            egui::RichText::new(tr!("What is set below is set on all {count}. Select one garment to edit its name, position, files and textures.", count = picked.len()))
            .small()
            .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
    let of: Vec<&Garment> = (picked.iter())
        .filter_map(|id| read.wardrobe.garment(id))
        .collect();
    let (peds, slots) = (
        of.iter().all(|other| other.ped == g.ped),
        of.iter().all(|other| other.slot == g.slot),
    );
    heading(ui, tr!("Place"));
    place::field(ui, tr!("Ped"), |ui| {
        if let Some(Some(ped)) = place::ped_choice(ui, "garments-ped", g.ped, Some(!peds)) {
            asked.all = Some(GroupAsk::Ped(ped));
        }
    });
    place::field(ui, tr!("Slot"), |ui| {
        if let Some(Some(slot)) = place::slot_choice(ui, "garments-slot", g.slot, Some(!slots)) {
            asked.all = Some(GroupAsk::Slot(slot));
        }
    });
    tags::draw(ui, shown, &of, &mut asked.tags);
}

/// A command as a button, greyed with the reason when it cannot run; true once it is clicked.
///
/// ⚠ The panel is drawn from a shared borrow of the state, so the click is carried out after, by the command's own name.
pub(super) fn command_button(ui: &mut egui::Ui, state: &AppState, action: Action) -> bool {
    let Some(command) = commands::of(state, action) else {
        return false;
    };
    let response = ui.add_enabled(command.enabled.is_ok(), egui::Button::new(&command.label));
    commands::hints(response, &command).clicked()
}

/// The garment's cloth: the file linked, or the lack of one the metadata asked for, with Link and Unlink.
///
/// ⚠ The flag the game reads (`ownsCloth`) is nothing the panel offers: it follows the link.
fn cloth(
    ui: &mut egui::Ui,
    state: &AppState,
    shown: &mut WardrobeUi,
    g: &Garment,
    asked: &mut Asked,
) {
    heading(ui, tr!("Cloth"));
    match (&g.cloth, g.missing_cloth) {
        (Some(file), _) => {
            ui.label(
                egui::RichText::new(tr!("Linked: {file}", file = file))
                    .small()
                    .monospace(),
            );
            ui.label(
                egui::RichText::new(tr!(
                    "The build ships it under the model's name and states ownsCloth because of it"
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            );
        }
        (None, true) => {
            ui.label(
                egui::RichText::new(tr!(
                    "Missing: its metadata states ownsCloth and no cloth file is linked"
                ))
                .small()
                .color(theme::ERROR),
            );
        }
        (None, false) => {
            ui.label(
                egui::RichText::new(tr!("No cloth: the game simulates none for it"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    }
    ui.horizontal(|ui| {
        asked.link_cloth |= command_button(ui, state, Action::LinkCloth);
        asked.unlink_cloth |= command_button(ui, state, Action::UnlinkCloth);
    });
    if let Some(pack) = state.wardrobe_pack() {
        super::cloth_wind::draw(ui, &mut shown.wind, &pack.folder, g, &mut asked.edit);
    }
}

/// The shop id each texture is written with, where it came from, and the one a person may give it in its place.
fn shop_ids(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    read: &PackRead,
    at: usize,
    asked: &mut Asked,
) {
    let g = &read.wardrobe.garments[at];
    let Some(placed) = read.allocation.of(at) else {
        return;
    };
    if g.is_reserved() || g.variations.is_empty() {
        return;
    }
    heading(ui, tr!("Shop"));
    for (i, variation) in g.variations.iter().enumerate() {
        let letter = textures::letter(i);
        let written = shop::id_of(&read.wardrobe, g, placed, i);
        let came_from = match (&variation.shop_id, &variation.shop) {
            (Some(_), _) => tr!("chosen here").to_string(),
            (None, Some(entry)) => tr!(
                "read from {from}, entry {entry}",
                from = entry.from,
                entry = entry.entry + 1
            ),
            (None, None) => tr!("named by the build").to_string(),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(letter.to_string()).monospace());
            ui.label(egui::RichText::new(&written).monospace());
        });
        let cost = variation.shop.as_ref().and_then(|e| e.cost.as_deref());
        let mut said = came_from;
        if let Some(cost) = cost {
            said.push_str(&tr!(", cost {cost}", cost = cost));
        }
        ui.label(
            egui::RichText::new(said)
                .small()
                .color(theme::TEXT_SECONDARY),
        );

        // The id somebody chose, which wins over the one an import read; empty takes the choice back.
        let mut typed = match &shown.shop_id {
            Some((who, texture, text)) if *who == g.id && *texture == i => text.clone(),
            _ => variation.shop_id.clone().unwrap_or_default(),
        };
        ui.horizontal_wrapped(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut typed)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(tr!("Use another id"))
                    .desired_width(ui.available_width()),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit,
                    true,
                    tr!("Shop id of texture {letter}", letter = letter),
                )
            });
            if field.changed() {
                shown.shop_id = Some((g.id.clone(), i, typed.clone()));
            }
            if field.lost_focus() {
                shown.shop_id = None;
                let chosen = Some(typed.trim().to_string()).filter(|t| !t.is_empty());
                if chosen != variation.shop_id {
                    asked.shop_id = Some((g.id.clone(), i, chosen));
                }
            }
            if variation.shop_id.is_some() {
                let back = if variation.shop.is_some() {
                    tr!("Use the imported id")
                } else {
                    tr!("Use the build's name")
                };
                let button = ui.small_button(back);
                button.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        tr!("{back} for texture {letter}", back = back, letter = letter),
                    )
                });
                if button.clicked() {
                    shown.shop_id = None;
                    asked.shop_id = Some((g.id.clone(), i, None));
                }
            }
        });
        super::entry_fields::draw(ui, &mut shown.entry, g, i, &mut asked.edit);
    }
}

/// The pack itself: its name, how its shop names are written, and the waivers it records.
fn pack_settings(
    ui: &mut egui::Ui,
    state: &AppState,
    shown: &mut WardrobeUi,
    read: &PackRead,
    asked: &mut Asked,
) {
    let w = &read.wardrobe;
    ui.label(egui::RichText::new(tr!("Pack")).strong().size(15.0));
    // The name the collections and the streamed folder are made of; the engine refuses one a collection cannot carry.
    let mut name = match &shown.pack {
        Some((who, typed)) if *who == w.pack => typed.clone(),
        _ => w.pack.clone(),
    };
    ui.horizontal(|ui| {
        ui.label(tr!("Name"));
        let field = ui.add(egui::TextEdit::singleline(&mut name).font(egui::TextStyle::Monospace));
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Pack name"))
        });
        if std::mem::take(&mut shown.focus_name) {
            field.request_focus();
        }
        if field.changed() {
            shown.pack = Some((w.pack.clone(), name.clone()));
        }
        if field.lost_focus() {
            shown.pack = None;
            if name != w.pack {
                asked.rename = Some(name.clone());
            }
        }
    });
    // ⚠ No count of nothing: a pack with no garment says nothing of what it plans.
    let bare = w.garments.is_empty();
    if !bare {
        ui.label(
            egui::RichText::new(tr!(
                "{count}, {count_2} and {count_3} in the metadata it plans",
                count = counted(w.garments.len(), "garment"),
                count_2 = counted(read.drawables as usize, "drawable"),
                count_3 = counted(read.textures as usize, "texture")
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
    }
    for ped in Ped::ALL {
        ui.label(
            egui::RichText::new(tr!(
                "{ped}: collection {w}",
                ped = ped_words(Some(ped)),
                w = w.collection(ped)
            ))
            .small()
            .monospace(),
        );
    }

    // ⚠ A package adds a garment to the pack, so it is offered here as well as on a garment: none needs to be selected.
    if command_button(ui, state, Action::AddPackage) {
        asked.run.push(Action::AddPackage);
    }

    // ⚠ What a first garment does not need is folded away until it is asked for: the alternate variations, the shop's names and the waivers.
    match bare {
        true => {
            egui::CollapsingHeader::new(tr!("Pack settings"))
                .default_open(false)
                .show(ui, |ui| pack_more(ui, shown, w, asked));
        }
        false => pack_more(ui, shown, w, asked),
    }
}

/// The pack's alternate variations, how its shop names are written, and the waivers it records.
fn pack_more(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    w: &or_wardrobe::Wardrobe,
    asked: &mut Asked,
) {
    behaviour::sets(ui, &mut shown.behaviour, w, &mut asked.behaviour);

    heading(ui, tr!("Shop"));
    let mut durty = w.shop.durty_hashes;
    let check = ui.checkbox(
        &mut durty,
        tr!("Name shop entries as Durty Cloth Tool does"),
    );
    check.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            w.shop.durty_hashes,
            tr!("Name shop entries as Durty Cloth Tool does"),
        )
    });
    if check.changed() {
        asked.durty = Some(durty);
    }
    ui.label(
        egui::RichText::new(
            tr!("A script, a shop table or a package keyed on Durty's names (CLO_DCT_TOP_0_0_M) keeps finding \
             the entries after a move to OpenRage. Off, an entry is named for its collection, slot, position \
             and texture. Two packs that both use Durty's names share them, as Durty's do."),
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );

    // What an import read of a shop's own metadata and keeps, so that it is written back.
    if !w.shop.unplaced.is_empty() {
        ui.label(
            egui::RichText::new(tr_n!(
                "{count} shop entry read from a shop's own metadata point at no garment of the pack: kept as stated and written back",
                "{count} shop entries read from a shop's own metadata point at no garment of the pack: kept as stated and written back",
                w.shop.unplaced.len()
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
    }
    for carried in &w.shop.carried {
        ui.label(
            egui::RichText::new(tr!(
                "{file} ({kind}) is carried unchanged",
                file = carried.file,
                kind = carried.kind
            ))
            .small()
            .monospace(),
        );
    }

    heading(ui, &tr!("Waivers ({count})", count = w.waivers.len()));
    if w.waivers.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(tr!(
                    "None: Waive… in Problems records a reason a finding stands"
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    }
    for (i, waiver) in w.waivers.iter().enumerate() {
        let who = waiver
            .garment
            .as_deref()
            .and_then(|id| w.garment(id))
            .map_or(tr!("every garment").to_string(), |g| {
                or_wardrobe::rules::name_of(g)
            });
        ui.horizontal_wrapped(|ui| {
            // ⚠ A person reads the rule's sentence; the code is the tooltip's. A waiver of a rule this build has no row for keeps what it says.
            match or_wardrobe::rule_of(&waiver.code) {
                Some(rule) => ui
                    .label(rule.sentence())
                    .on_hover_text(tr!("Rule {code}", code = waiver.code)),
                None => ui.label(egui::RichText::new(&waiver.code).monospace()),
            };
            ui.label(tr!(
                "for {who}: {reason}",
                who = who,
                reason = waiver.reason
            ));
            if ui.small_button(tr!("Remove")).clicked() {
                asked.unwaive = Some(i);
            }
        });
    }
}
