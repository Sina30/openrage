//! The header of the Wardrobe panel: two rows that keep their places, what adds, mends and takes back, and which garments show.
//!
//! ⚠ **A row never wraps.** A control that wraps moves when another one's label changes, so the first row is the actions (every menu is named by a word, never by an icon alone) and the second is the filter with the choice of ped beside it.
//!
//! A pack with no garment has the first row only: no filter of nobody.

use egui_phosphor::regular as icon;
use or_wardrobe::Ped;

use super::{action_button, WardrobeUi};
use crate::state::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::theme;

/// What a clothing content is called on screen: its pack, which is the name people gave it, and not the id the project file keeps.
pub(crate) fn pack_name(state: &AppState, module: &str) -> String {
    (state.workbench.packs().iter())
        .find(|p| p.module == module)
        .and_then(|p| p.read.as_ref().ok())
        .map_or_else(|| module.to_string(), |read| read.wardrobe.pack.clone())
}

/// What a combo's arrow takes of its width, which its text does not have.
const COMBO_ARROW: f32 = 22.0;

/// What the state of the build, said beside its button, takes of the line.
const BUILD_STATE: f32 = 62.0;

/// The choice of ped, drawn at one width so that the filter beside it keeps its room.
const PED_CHOICE: f32 = 108.0;

/// Undo and Redo: icons at one width, whatever they would take back, so that nothing beside them moves.
const STEP_BUTTON: f32 = 30.0;

/// The room the filter keeps at its right end for the button that clears it.
const CLEAR_ROOM: f32 = 22.0;

/// Whether the pack has no garment and is this project's to fill: the Wardrobe is then one drop area and nothing else.
pub(super) fn bare(state: &AppState) -> bool {
    (state.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
        .is_some_and(|read| read.wardrobe.garments.is_empty())
        && !state.wardrobe_is_linked()
}

/// The commands of the Wardrobe, in the bar under the tabs while the screen is in front: what adds clothes, what fixes them, the versions, and the two steps of undo.
pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    if state.wardrobe_module_id().is_none() {
        return;
    }
    let empty = (state.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
        .is_some_and(|read| read.wardrobe.garments.is_empty());
    // ⚠ An empty pack's own drop area carries the one primary button, so this one is not a second.
    add_menu(ui, state, !empty);
    fix_menu(ui, state);
    super::recover::versions_menu(ui, state);
    undo_button(ui, state);
    redo_button(ui, state);
}

pub(super) fn draw(ui: &mut egui::Ui, state: &mut AppState, shown: &mut WardrobeUi, module: &str) {
    pack_chooser(ui, state, module);
    // ⚠ An empty pack is one drop area and nothing else: no filter, no ped to choose, no saved queries of nobody.
    if bare(state) {
        return;
    }
    ui.horizontal(|ui| {
        let room = (ui.available_width() - PED_CHOICE - ui.spacing().item_spacing.x).max(60.0);
        // The buttons at the right end of the field: the terms it understands, and while it holds something, the one that clears it.
        let buttons = match shown.filter.is_empty() {
            true => super::query_help::ROOM,
            false => super::query_help::ROOM + CLEAR_ROOM,
        };
        let filter = ui
            .add(
                egui::TextEdit::singleline(&mut shown.filter)
                    .hint_text(crate::i18n::tr(super::query_bar::HINT))
                    .margin(egui::Margin {
                        left: 4,
                        right: buttons as i8,
                        top: 2,
                        bottom: 2,
                    })
                    .desired_width(room),
            )
            .on_hover_text(crate::i18n::tr(super::query_bar::HELP));
        filter.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Filter garments"))
        });
        if std::mem::take(&mut shown.focus_filter) {
            filter.request_focus();
        }
        clear_button(ui, shown, filter.rect);
        if let Some(term) = super::query_help::button(ui, filter.rect) {
            super::query_help::put(ui.ctx(), &filter, &mut shown.filter, term);
            shown.focus_filter = true;
        }
        ped_choice(ui, shown);
    });
    super::query_bar::draw(ui, state, shown, module);
}

/// The button at the right end of the filter that empties it, drawn only while it holds something.
fn clear_button(ui: &mut egui::Ui, shown: &mut WardrobeUi, field: egui::Rect) {
    if shown.filter.is_empty() {
        return;
    }
    let side = field.height() - 4.0;
    let at = egui::Rect::from_min_size(
        egui::pos2(
            field.right() - super::query_help::ROOM - side - 3.0,
            field.top() + 2.0,
        ),
        egui::vec2(side, side),
    );
    let clear = ui.place(at, egui::Button::new(icon::X).small().frame(false));
    clear.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Clear the filter"))
    });
    if clear
        .on_hover_text(tr!("Clear the filter: every garment is listed again"))
        .clicked()
    {
        shown.filter.clear();
    }
}

/// Which ped's garments the list shows, as one choice and not three that look like captions.
fn ped_choice(ui: &mut egui::Ui, shown: &mut WardrobeUi) {
    let words = |ped: Option<Ped>| match ped {
        None => tr!("Both peds"),
        Some(Ped::Male) => tr!("Male"),
        Some(Ped::Female) => tr!("Female"),
    };
    let combo = egui::ComboBox::from_id_salt("wardrobe_ped")
        .width(PED_CHOICE)
        .selected_text(words(shown.ped))
        .show_ui(ui, |ui| {
            for ped in [None, Some(Ped::Male), Some(Ped::Female)] {
                ui.selectable_value(&mut shown.ped, ped, words(ped));
            }
        });
    combo.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr!("Peds shown"))
    });
    combo.response.on_hover_text(tr!(
        "Which ped's garments the list shows: both, the male ped's or the female ped's"
    ));
}

/// The choice of clothing pack, on a row of its own and only where the project holds more than one.
fn pack_chooser(ui: &mut egui::Ui, state: &mut AppState, module: &str) {
    // ⚠ The name people gave the pack, and while the scan has not read it yet, not its id, which nobody typed.
    let others: Vec<(String, String)> = state
        .clothing_modules()
        .iter()
        .map(|m| {
            let name = match (state.workbench.packs().iter()).find(|p| p.module == m.id) {
                None => tr!("Reading…").to_string(),
                Some(pack) if pack.read.is_err() => tr!("Could not be read").to_string(),
                Some(_) => pack_name(state, &m.id),
            };
            (m.id.clone(), name)
        })
        .collect();
    if others.len() < 2 {
        return;
    }
    let mut chosen = module.to_string();
    let named = others
        .iter()
        .find(|(id, _)| id == module)
        .map_or(module, |(_, name)| name.as_str());
    let combo = egui::ComboBox::from_id_salt("wardrobe_content")
        .selected_text(tr!("Pack: {named}", named = named))
        .show_ui(ui, |ui| {
            for (id, name) in &others {
                ui.selectable_value(&mut chosen, id.clone(), name);
            }
        });
    combo.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr!("Clothing pack shown"))
    });
    if chosen != module {
        state.workbench.wardrobe.module = Some(chosen);
        state.workbench.wardrobe.selected = None;
    }
}

/// The words of an edit that runs and the button that stops it, above the build; nothing is drawn while no edit runs.
pub(super) fn progress(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(what) = state.workbench.wardrobe.progress_words() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.spinner();
        ui.add(egui::Label::new(egui::RichText::new(what).color(theme::TEXT_SECONDARY)).truncate());
        if state.workbench.wardrobe.stoppable()
            && ui
                .small_button(tr!("Stop"))
                .on_hover_text(tr!("Stop: nothing is changed and the pack stays as it was"))
                .clicked()
        {
            state.workbench.wardrobe.stop();
        }
    });
}

/// Add clothes…: a menu of the two ways in, files and a whole folder (the file dialog of a platform picks one or the other), in the primary colour while the pack has garments.
fn add_menu(ui: &mut egui::Ui, state: &mut AppState, filled: bool) {
    let (Some(files), Some(folder)) = (
        commands::of(state, Action::AddClothes),
        commands::of(state, Action::AddClothesFolder),
    ) else {
        return;
    };
    let text = egui::RichText::new(files.label.as_str());
    let button = match filled {
        true => egui::Button::new(text.color(theme::ON_ACCENT.get())).fill(theme::ACCENT.get()),
        false => egui::Button::new(text),
    };
    let menu = ui.add_enabled_ui(files.enabled.is_ok(), |ui| {
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            crate::ui::popup_of_the_shell(ui.ctx());
            ui.set_min_width(120.0);
            for (words, command) in [(tr!("Files…"), &files), (tr!("Folder…"), &folder)] {
                let item = ui.add_enabled(command.enabled.is_ok(), egui::Button::new(words));
                if commands::hints(item, command).clicked() {
                    ui.close_kind(egui::UiKind::Menu);
                    commands::run(state, ui.ctx(), command);
                }
            }
        })
    });
    let (button, _) = menu.inner;
    commands::hints(button, &files);
}

/// A weak line of the Fix menu: the scope of the commands under it, or what the command above it does.
fn weak(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

/// The fixes of the pack in one menu, so that the header keeps the rows it had, in groups that say what each acts on: the whole pack, the garments picked or the whole pack, and the garment selected. The Project menu and the palette offer them too.
fn fix_menu(ui: &mut egui::Ui, state: &mut AppState) {
    let menu = ui.menu_button(tr!("Fix"), |ui| {
        ui.set_min_width(300.0);
        let groups: [(&str, &[Action]); 3] = [
            (tr!("The whole pack"), &[Action::FixAllSafe]),
            (
                tr!("The garments picked, or all"),
                &[
                    Action::OptimiseTextures,
                    Action::MakeMaps,
                    Action::BudgetTextures,
                    Action::CompleteMaterials,
                ],
            ),
            (tr!("The garment selected"), &[Action::RepairModel]),
        ];
        // ⚠ What each fix does, in a line under it, where a line is its only explanation: a menu of verbs alone leaves a person to guess what is changed, and how much.
        ui.set_max_width(340.0);
        let mut said_why = None;
        for (at, (scope, actions)) in groups.into_iter().enumerate() {
            if at > 0 {
                ui.separator();
            }
            weak(ui, scope);
            for &action in actions {
                let Some(command) = commands::of(state, action) else {
                    continue;
                };
                crate::ui::panels::menu::draw_command(ui, state, &command);
                // A fix that cannot run says why, once for a reason several share.
                match command.enabled {
                    Ok(()) => weak(ui, fix_caption(action)),
                    Err(why) if said_why != Some(why) => {
                        said_why = Some(why);
                        weak(ui, why);
                    }
                    Err(_) => {}
                }
                if action == Action::FixAllSafe {
                    levels_checkbox(ui, state);
                }
            }
        }
    });
    menu.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Fix")));
    menu.response.on_hover_text(
        tr!("Mend the pack: fix everything that is safe to, make the textures lighter or give them maps, and repair a model"),
    );
}

/// What a fix of the menu does to the pack, in a plain sentence for under its name.
fn fix_caption(action: Action) -> &'static str {
    match action {
        Action::FixAllSafe => {
            tr!("Mends what is safe to mend in the models, the textures, the order of the garments and the skins they fit. Nothing is made smaller on purpose and nothing is taken out. One Undo")
        }
        Action::OptimiseTextures => {
            tr!("Makes the textures use less memory, as lighter copies: the pictures they came from are left as they are")
        }
        Action::MakeMaps => tr!("Adds a normal map and a specular map to each texture, estimated from its brightness"),
        Action::BudgetTextures => tr!("Fits the textures in the memory you pick, losing the least picture"),
        Action::CompleteMaterials => {
            tr!("Fills in the shader values a material lacks, in each model. One Undo takes it back")
        }
        Action::RepairModel => tr!("Fixes a model that states its triangle and vertex counts wrongly, without changing its shape"),
        _ => "",
    }
}

/// Whether Fix all safe also gives the models that lack them a medium and a low level, in its one step of Undo.
fn levels_checkbox(ui: &mut egui::Ui, state: &mut AppState) {
    // Indented: it is an option of Fix all safe, which stands above it.
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        // ⚠ A menu draws a tick box with no frame, so an unticked one reads as greyed out.
        super::kit::framed(ui);
        ui.checkbox(
            &mut state.workbench.wardrobe.mesh.fix_levels,
            tr!("Also add missing levels of detail"),
        )
        .on_hover_text(
            tr!("Off: Fix all safe leaves every model as it is. On: it also gives each model that has a high level and no medium or low one those levels, made from the high level at the shares the mesh doctor is set to, in the same one step of Undo; a level a model has is kept"),
        );
    });
}

/// The button that takes the last change back: an icon of one width, named for the change it would take back and saying what it would take back.
fn undo_button(ui: &mut egui::Ui, state: &mut AppState) {
    let name = state.workbench.wardrobe.undo_words();
    let Some(mut command) = commands::of(state, Action::UndoClothes) else {
        return;
    };
    if let Some(label) = state.workbench.wardrobe.undo_label() {
        command.hover = Some(tr!("Takes back: {label}", label = label));
    }
    let button =
        egui::Button::new(icon::ARROW_COUNTER_CLOCKWISE).min_size(egui::vec2(STEP_BUTTON, 0.0));
    let response = ui.add_enabled(command.enabled.is_ok(), button);
    let response = crate::ui::accessible_name::named(response, &name);
    if commands::hints(response, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
    }
}

/// The button that makes the change taken back last again, saying what it would make again.
fn redo_button(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(mut command) = commands::of(state, Action::RedoClothes) else {
        return;
    };
    if let Some(label) = state.redo_label() {
        command.hover = Some(tr!("Makes again: {label}", label = label));
    }
    let button = egui::Button::new(icon::ARROW_CLOCKWISE).min_size(egui::vec2(STEP_BUTTON, 0.0));
    let response = ui.add_enabled(command.enabled.is_ok(), button);
    let response = crate::ui::accessible_name::named(response, tr!("Redo"));
    if commands::hints(response, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
    }
}

/// What the banner over linked clothing says, and the way to the project that edits it.
pub(super) fn linked_banner(ui: &mut egui::Ui, state: &mut AppState, module: &str) {
    let (said, owner) = match state.workbench.link(module) {
        Some(link) => (link.banner(), link.owner.is_some()),
        None => (
            tr!("Linked from another project, which alone edits it.").to_string(),
            false,
        ),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));
        if owner && ui.button(tr!("Open project")).clicked() {
            state.open_linked_source(module);
        }
    });
    ui.add_space(theme::SPACING_TIGHT);
}

/// The builds that ship this clothing, the one the window's Build button builds and the state of that build, on a row of their own under the list. The button itself is the menu bar's, the one Build control of the window.
///
/// With no build that ships it, the way to make one.
pub(super) fn build_chooser(ui: &mut egui::Ui, state: &mut AppState, module: &str) {
    let outputs: Vec<(String, String)> = (state.project_carried.container.outputs.iter())
        .filter(|o| o.take.iter().any(|t| t == module))
        .map(|o| (o.id.clone(), o.name.clone()))
        .collect();
    // ⚠ All the room the state leaves, and not a fixed cap: builds of one project share the start of their names, and a chooser cut after the shared part cannot say which one the Build button writes.
    // ⚠ The state keeps what its words need when they are longer than the room English gives them, which another language's are.
    let state_room = state.active_output_id().map_or(BUILD_STATE, |output| {
        let (words, _) = super::strip::build_state(state, &output);
        let font = egui::TextStyle::Small.resolve(ui.style());
        let galley = ui
            .painter()
            .layout_no_wrap(words, font, egui::Color32::WHITE);
        (galley.size().x + 2.0 * ui.spacing().item_spacing.x).max(BUILD_STATE)
    });
    let room = (ui.available_width() - state_room).max(60.0);
    ui.horizontal(|ui| {
        if outputs.is_empty() {
            ui.label(
                egui::RichText::new(tr!("No build ships this clothing"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            action_button(
                ui,
                state,
                Action::AddBuild,
                Some(tr!("Add a build").into()),
                false,
            );
            return;
        }
        let active = state.active_output_id();
        let mut chosen = active.clone();
        let name = outputs
            .iter()
            .find(|(id, _)| Some(id) == active.as_ref())
            .map_or(tr!("Build"), |(_, name)| name.as_str())
            .to_string();
        // ⚠ The name alone, since the Build button of the menu bar names the verb; and cut in the middle when it does not fit, so what tells the builds apart (`…_dlc` from `…_oiv`) stays in sight.
        let font = egui::TextStyle::Button.resolve(ui.style());
        let shown = super::words::cut_in_the_middle(&name, |text| {
            let galley =
                ui.painter()
                    .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE);
            galley.size().x <= room - COMBO_ARROW
        });
        let combo = ui
            .scope(|ui| {
                ui.set_max_width(room);
                egui::ComboBox::from_id_salt("wardrobe_build")
                    .width(room)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (id, name) in &outputs {
                            ui.selectable_value(&mut chosen, Some(id.clone()), name);
                        }
                    })
            })
            .inner;
        combo.response.widget_info(|| egui::WidgetInfo {
            current_text_value: Some(name.clone()),
            ..egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                true,
                tr!("Build the Build button makes"),
            )
        });
        if chosen != active {
            state.workbench.selected_output = chosen;
        }
        if let Some(output) = state.active_output_id() {
            let (words, colour) = super::strip::build_state(state, &output);
            ui.add(egui::Label::new(egui::RichText::new(words).small().color(colour)).truncate())
                .on_hover_text(super::strip::last_build(state, &output));
        }
    });
}
