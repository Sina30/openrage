//! The rules of a clothing content: for each rule whether it is on, how serious it is and where its limit stands, and which edition the pack is built for.
//!
//! The window edits a copy and Apply makes it the pack's rules as one step of Undo, so a drag of a limit is not a hundred writes.
//! A rule that no reason could mend is listed and cannot be changed, since a pack that names a file that is not there is not a
//! pack a project may decide to let through.
//!
//! ⚠ **Two headed sections.** What a build does (the edition, the freeze, the empty places, replacing the game's files) is not a check, and a check has a bold title, a line that says what it is about and a choice of how serious it is; the whole of a rule's sentence is the line under its title, wrapped, and Reset stands only where a check is not at its default.

use or_wardrobe::settings::{Level, Replace, Settings, Target};
use or_wardrobe::{Ped, Rule, Severity, RULES};

use super::builds_editor::named_choice;
use super::rule_titles::title_or_sentence;
use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Rules of the clothing");

/// The room a rule's second line and its limit leave under its checkbox, so that they sit under its title.
const INDENT: f32 = 24.0;

/// The room of the Reset button of a check, kept whether it is drawn or not so that the choices of how serious stand in one column under their header.
const RESET: f32 = 44.0;

pub fn draw(ctx: &egui::Context, state: &mut AppState, module: String, mut draft: Settings) {
    let current = state.rules_of_pack(&module).unwrap_or_default();
    let frozen = state.frozen_places(&module);
    let pack = crate::ui::panels::wardrobe::pack_name(state, &module);
    state.poll_game_collections();
    if state.workbench.collections.reading() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    let mut answer: Option<bool> = None;
    let mut export = false;
    question_within(ctx, crate::i18n::tr(TITLE), 640.0, |ui| {
        ui.label(
            egui::RichText::new(tr!(
                "Rules for the pack {pack}. A rule you do not change is not written to the pack.",
                pack = pack
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_SNUG);
        // ⚠ One scroll for both sections, under a height that leaves the strip of workspaces in sight: the buttons under it never scroll away.
        egui::ScrollArea::vertical()
            .max_height((ctx.content_rect().height() - 250.0).clamp(160.0, 520.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                section(ui, tr!("What a build does"));
                built_for(ui, &mut draft);
                places(ui, &mut draft, frozen);
                replacing(ui, state, &mut draft);
                ui.add_space(theme::SPACING_DEFAULT);
                section(ui, tr!("Checks"));
                how_serious(ui);
                for rule in RULES.iter().filter(|r| r.waivable) {
                    one(ui, &mut draft, rule);
                    ui.add_space(theme::SPACING_TIGHT);
                }
                // ⚠ A rule no reason mends is listed and cannot change, so it is out of the way until asked for.
                egui::CollapsingHeader::new(tr!("Always on: no reason mends these"))
                    .default_open(false)
                    .show(ui, |ui| {
                        for rule in RULES.iter().filter(|r| !r.waivable) {
                            // ⚠ The code is for the tooltip and the export; a person reads the sentence.
                            ui.label(
                                egui::RichText::new(rule.sentence())
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                            )
                            .on_hover_text(tr!("Rule {code}", code = rule.code));
                        }
                    });
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(draft != current, egui::Button::new(tr!("Apply")))
                .on_disabled_hover_text(tr!("Nothing is changed yet"))
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
            if ui
                .button(tr!("Export the list…"))
                .on_hover_text(tr!(
                    "Write every rule, with its severity and limit, to a file"
                ))
                .clicked()
            {
                export = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !draft.settings.is_empty(),
                        egui::Button::new(tr!("Reset all checks")),
                    )
                    .on_hover_text(tr!(
                        "Put every check back to the severity and the limit it has by default"
                    ))
                    .on_disabled_hover_text(tr!("Every check is at its default"))
                    .clicked()
                {
                    draft.settings.clear();
                }
            });
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(false);
    }
    if export {
        state.export_rules();
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.apply_rules(module, draft);
        }
        Some(false) => state.workbench.asking = None,
        None => state.workbench.asking = Some(Question::Rules { module, draft }),
    }
}

/// A heading for a group of settings: it names what the rows under it are, so that a switch of what a build does is not mistaken for a check.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().size(15.0));
    ui.separator();
}

/// The words over the column of choices: what the three levels of a check are for.
fn how_serious(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(tr!("Check"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // ⚠ Over the choices and not over the Reset buttons beside them.
            ui.add_space(RESET);
            ui.label(
                egui::RichText::new(tr!("How serious"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .on_hover_text(tr!(
                "A note is listed and stops nothing, a warning is listed, an error stops a build"
            ));
        });
    });
}

/// The edition the pack is built for, which the limits that depend on it follow.
fn built_for(ui: &mut egui::Ui, draft: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label(tr!("Built for"));
        for (target, label) in [
            (None, tr!("Both editions")),
            (Some(Target::Legacy), tr!("Legacy")),
            (Some(Target::Enhanced), tr!("Enhanced")),
        ] {
            ui.selectable_value(&mut draft.target, target, label);
        }
    });
}

/// What a build does with the places of the garments: freezing them, and the empty places that hold one.
fn places(ui: &mut egui::Ui, draft: &mut Settings, frozen: Option<usize>) {
    ui.add_space(theme::SPACING_TIGHT);
    let mut freeze = draft.freezes();
    let said = tr!("Freeze the drawables a build ships");
    let response = ui.checkbox(&mut freeze, said);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, freeze, said));
    if freeze != draft.freezes() {
        draft.freeze = (!freeze).then_some(false);
    }
    let state = match (freeze, frozen) {
        (false, _) => tr!("Off: the pack renumbers freely, and a lock it has is let go at the next build.").to_string(),
        (true, Some(n)) => tr!("{n} drawables are frozen since the last build. A garment that would move stops the build until it is pinned back or a reason says why.", n = n),
        (true, None) => tr!("Nothing is frozen until a build goes through; after that a garment that would move stops the next build.").to_string(),
    };
    ui.label(
        egui::RichText::new(state)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    for (on, said, hover) in [
        (
            &mut draft.dummies.replace_on_add,
            tr!("A garment added takes the first empty place"),
            tr!("An empty place holds a number and ships no model. A garment added to its slot takes the first one's place, and the empty place goes; a texture added to a garment takes the first empty texture it holds the same way"),
        ),
        (
            &mut draft.dummies.leave_on_delete,
            tr!("A garment taken out leaves an empty place"),
            tr!("The garments after it keep their places, which saved outfits point at"),
        ),
    ] {
        let response = ui.checkbox(on, said).on_hover_text(hover);
        let now = *on;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, now, said)
        });
    }
}

/// How one ped's files meet the game's: a collection of their own, one of the game's, or the ped's base drawables.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Adds,
    Dlc,
    Base,
}

impl Mode {
    fn of(replace: Option<&Replace>) -> Self {
        match replace {
            None => Self::Adds,
            Some(Replace { dlc: Some(_) }) => Self::Dlc,
            Some(_) => Self::Base,
        }
    }
}

/// Replace the game's files instead of adding a collection, for each ped; the collections of the game are the install's DLC list.
fn replacing(ui: &mut egui::Ui, state: &AppState, draft: &mut Settings) {
    egui::CollapsingHeader::new(tr!("Replace the game's files"))
        .default_open(!draft.replace.is_empty())
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr!("A replacement is for a FiveM resource. It writes the files under the game's names and none of the metadata, so each garment is pinned at the drawable it replaces. {unverified}.", unverified = or_wardrobe::replace_mode::UNVERIFIED))
                .small()
                .color(theme::TEXT_SECONDARY),
            );
            for (ped, words) in [(Ped::Male, tr!("Male")), (Ped::Female, tr!("Female"))] {
                let mut mode = Mode::of(draft.replace.get(&ped));
                let before = mode;
                ui.horizontal_wrapped(|ui| {
                    ui.label(words);
                    ui.selectable_value(&mut mode, Mode::Adds, tr!("Adds its own collection"));
                    ui.selectable_value(&mut mode, Mode::Dlc, tr!("Replaces a collection of the game"));
                    ui.selectable_value(&mut mode, Mode::Base, tr!("Replaces the base game's drawables"));
                });
                if mode != before {
                    match mode {
                        Mode::Adds => {
                            draft.replace.remove(&ped);
                        }
                        Mode::Base => {
                            draft.replace.insert(ped, Replace::default());
                        }
                        Mode::Dlc => {
                            draft.replace.insert(ped, Replace { dlc: Some(String::new()) });
                        }
                    }
                }
                if mode == Mode::Dlc {
                    collection_of_the_game(ui, state, draft, ped);
                }
            }
        });
}

/// The name of the game's collection a ped's files replace: typed, or chosen from the DLC list of the install.
fn collection_of_the_game(ui: &mut egui::Ui, state: &AppState, draft: &mut Settings, ped: Ped) {
    let Some(replace) = draft.replace.get_mut(&ped) else {
        return;
    };
    let mut name = replace.dlc.clone().unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.add_space(theme::SPACING_SECTION);
        let edit = ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("male_freemode_beach")
                .desired_width(160.0),
        );
        edit.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("Collection of the game to replace"),
            )
        });
        let listed = state.workbench.collections.collections();
        let mine: Vec<_> = listed.iter().filter(|c| c.ped == ped.model()).collect();
        if !mine.is_empty() {
            egui::ComboBox::from_id_salt(("replace-collection", ped.model()))
                .selected_text(tr!("From the game's DLC list"))
                .show_ui(ui, |ui| {
                    for found in mine {
                        ui.selectable_value(
                            &mut name,
                            found.collection.clone(),
                            format!("{} ({})", found.collection, found.pack),
                        );
                    }
                });
        } else if state.workbench.collections.reading() {
            ui.label(egui::RichText::new(tr!("Reading the game's DLC list…")).small());
        } else if let Some(why) = state.workbench.collections.failed() {
            ui.label(
                egui::RichText::new(why)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        } else {
            ui.label(
                egui::RichText::new(tr!("Choose a game in Settings to list its collections"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
    replace.dlc = Some(name);
}

fn level_word(level: Level) -> &'static str {
    match level {
        Level::Info => tr!("Note"),
        Level::Warning => tr!("Warning"),
        Level::Error => tr!("Error"),
    }
}

/// One rule that a reason can mend: its title and its choice of how serious, a line that says what it is about, and the limit it holds a value to.
///
/// ⚠ The title is for a person and the rule's code is the tooltip's and the export's; the whole sentence is the line under the title and part of the name a screen reader speaks.
fn one(ui: &mut egui::Ui, draft: &mut Settings, rule: &Rule) {
    let set = draft.effective(rule);
    let sentence = rule.sentence();
    let title = title_or_sentence(rule);
    let changed = draft.settings.contains_key(rule.code);
    ui.horizontal(|ui| {
        let mut on = set.on;
        let response = ui
            .add(egui::Checkbox::new(
                &mut on,
                egui::RichText::new(title.as_str()).strong(),
            ))
            .on_hover_text(tr!("Rule {code}", code = rule.code));
        let said = format!("{title}: {sentence}");
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, said.clone())
        });
        if on != set.on {
            draft.set(rule.code, |s| s.on = Some(on));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // ⚠ Drawn only where the check is not at its default; its room is kept either way.
            if changed {
                let reset = ui
                    .add(
                        egui::Button::new(tr!("Reset"))
                            .small()
                            .min_size(egui::vec2(RESET, 0.0)),
                    )
                    .on_hover_text(tr!(
                        "Put this check back to the severity and the limit it has by default"
                    ));
                let reset =
                    crate::ui::accessible_name::named(reset, &tr!("Reset: {title}", title = title));
                if reset.clicked() {
                    draft.settings.remove(rule.code);
                }
            } else {
                ui.add_space(RESET);
            }
            let mut level = Level::from(set.severity);
            let combo = egui::ComboBox::from_id_salt(("rule-severity", rule.code))
                .width(86.0)
                .selected_text(level_word(level))
                .show_ui(ui, |ui| {
                    for each in [Level::Info, Level::Warning, Level::Error] {
                        ui.selectable_value(&mut level, each, level_word(each));
                    }
                });
            named_choice(
                &combo.response,
                &tr!("How serious: {sentence}", sentence = sentence),
                level_word(level),
            );
            if Severity::from(level) != set.severity {
                draft.set(rule.code, |s| s.severity = Some(level));
            }
        });
    });
    // The line that says what the check is about, whole: it wraps to the lines it needs.
    ui.horizontal(|ui| {
        // ⚠ A few points short of the edge: the sentence ends beside the scroll bar and not under it.
        let room = ui.available_width() - 10.0;
        ui.add_space(INDENT);
        ui.set_max_width(room);
        ui.add(
            egui::Label::new(
                egui::RichText::new(sentence.as_str())
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    });
    if let (Some(limit), Some(now)) = (rule.threshold, set.threshold) {
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            // A size in bytes is edited in mebibytes, which is how the rest of the screen counts it.
            let (unit, per) = match limit.unit {
                "bytes" => (tr!("MiB"), 1024 * 1024),
                other => (other, 1),
            };
            let mut shown = now / per;
            ui.label(egui::RichText::new(tr!("Limit")).small());
            let value = ui.add(
                egui::DragValue::new(&mut shown)
                    .speed(if per > 1 { 1.0 } else { 16.0 })
                    .suffix(format!(" {unit}")),
            );
            value.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    true,
                    tr!("Limit of {title}", title = title),
                )
            });
            if shown != now / per {
                draft.set(rule.code, |s| s.threshold = Some(shown * per));
            }
        });
    }
}
