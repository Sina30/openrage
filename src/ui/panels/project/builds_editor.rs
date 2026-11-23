//! The Builds window: the builds a project declares, a form for the chosen one, the matrix of content by builds, the types a singleplayer DLC registers files as, and a new build.
//!
//! ⚠ **Each control is one edit.** A toggle, a choice or a ceiling changes the project's builds when it is touched,
//! through `AppState::edit_build`, and a refusal says why beside the control and changes nothing; a name is
//! kept as it is typed and changes the build when it is entered, and a number dragged or typed changes it as it
//! moves and is said once in the history, when it is let go (`AppState::edit_build_live`). Nothing here writes a file: Save project does.
//! The matrix appears only when there is something to choose between: two contents or two builds.

use crate::state::front_door::KindWords as _;
use or_project::{Container, Output, OutputKind, Target};

use super::build_tab::{figure_is_bytes, figure_words};
use crate::state::workbench::{content_name, BuildDot, NewBuild, Question, Scope, KINDS};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Builds");

/// How wide the window is, or as wide as the application window leaves it.
const WIDTH: f32 = 620.0;

/// Whether there is anything to choose between: two contents or two builds, and a build to take them.
pub fn shows_matrix(container: &Container) -> bool {
    !container.outputs.is_empty() && (container.modules.len() >= 2 || container.outputs.len() >= 2)
}

/// What the window explains of each kind of build.
fn about(kind: &OutputKind) -> &'static str {
    match kind {
        OutputKind::SingleplayerDlc => {
            tr!("A singleplayer DLC is a dlc.rpf in a folder of build/, with the line dlclist.xml needs. \
             A path through a folder named x.rpf is packed as that archive, and a file whose \
             extension settles no type needs one, stated under File types.")
        }
        OutputKind::OivPackage => {
            tr!("An OIV package is one .oiv in a folder of build/, with a manifest that names every \
             file. A build checks that it reads back; it does not test that OpenIV installs it.")
        }
        _ => {
            tr!("A FiveM resource is a folder in build/ with its fxmanifest.lua. A build writes it to \
             a fresh folder, reads it back and swaps it in.")
        }
    }
}

/// A choice named `name` for a screen reader, which also says what it is set to.
pub(super) fn named_choice(response: &egui::Response, name: &str, value: &str) {
    response.widget_info(|| egui::WidgetInfo {
        current_text_value: Some(value.to_string()),
        ..egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name)
    });
}

fn edition_words(target: Option<Target>) -> &'static str {
    match target {
        Some(target) => target.generation().edition_label(),
        None => tr!("Keep as made"),
    }
}

/// A content as the matrix and the lists say it: the name every screen gives it, and that it is linked.
fn content_words(container: &Container, id: &str) -> String {
    let mut words = content_name(container, id);
    if container.module(id).is_some_and(|module| module.is_link()) {
        words.push_str(tr!(" (linked)"));
    }
    words
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    mut chosen: Option<String>,
    mut adding: Option<NewBuild>,
) {
    let outputs = state.project_carried.container.outputs.clone();
    if chosen
        .as_ref()
        .is_none_or(|id| !outputs.iter().any(|o| &o.id == id))
    {
        chosen = outputs.first().map(|o| o.id.clone());
    }
    let mut closed = false;
    let room = (ctx.content_rect().height() - 140.0).max(160.0);
    question_within(ctx, crate::i18n::tr(TITLE), WIDTH, |ui| {
        egui::ScrollArea::vertical()
            .max_height(room)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                // ⚠ A build being named is the only thing in the window: two forms that look alike, one above the other, are one too many, and the other form is not what Add build… asked for.
                if adding.is_some() {
                    new_build(ui, state, &mut adding, &mut chosen);
                    return;
                }
                // ⚠ The matrix, which is what this window is for, comes before the form of one build.
                list(ui, state, &outputs, &mut chosen);
                if shows_matrix(&state.project_carried.container) {
                    matrix(ui, state);
                }
                super::builds_file_types::draw(ui, state);
                if let Some(id) = chosen.clone() {
                    form(ui, state, &id, &mut chosen);
                }
            });
        ui.add_space(theme::SPACING_SNUG);
        ui.separator();
        // ⚠ Beside Done and not at the foot of a scroll: a person who wants another build is not sent to look for the button, and what a window this tall scrolls to is not the way to it.
        ui.horizontal(|ui| {
            if adding.is_none() && ui.button(tr!("Add a build…")).clicked() {
                adding = Some(state.new_build_draft());
            }
            if ui.button(tr!("Done")).clicked() {
                closed = true;
            }
        });
    });
    if closed || question_escaped(ctx, crate::i18n::tr(TITLE)) {
        state.settle_build_edit();
        state.workbench.asking = None;
        return;
    }
    // ⚠ Whatever a control asked for in this frame (a build removed, a question opened over this one) is not undone by writing the window back.
    if matches!(state.workbench.asking, Some(Question::Builds { .. })) {
        state.workbench.asking = Some(Question::Builds { chosen, adding });
    }
}

/// The builds, one to a row; a click shows it in the form below and makes it the build the Build button builds.
fn list(ui: &mut egui::Ui, state: &mut AppState, outputs: &[Output], chosen: &mut Option<String>) {
    if outputs.is_empty() {
        ui.label(
            egui::RichText::new(tr!("The project declares no build yet: name one below"))
                .color(theme::TEXT_SECONDARY),
        );
        return;
    }
    for output in outputs {
        let dot = state.workbench.dot(&output.id);
        let colour = match dot {
            BuildDot::Never => theme::TEXT_SECONDARY,
            BuildDot::Running => theme::INFO,
            BuildDot::Built => theme::SUCCESS,
            BuildDot::Failed => theme::ERROR,
            BuildDot::Blocked | BuildDot::Unchecked => theme::WARNING,
        };
        let selected = chosen.as_deref() == Some(output.id.as_str());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("●").color(colour))
                .on_hover_text(dot.label());
            let edition = output
                .target
                .map(|t| format!(", {}", t.generation().edition_label()))
                .unwrap_or_default();
            let row = ui.selectable_label(
                selected,
                format!("{}  ·  {}{edition}", output.name, output.kind.words()),
            );
            if row.clicked() {
                *chosen = Some(output.id.clone());
                state.workbench.selected_output = Some(output.id.clone());
            }
        });
    }
}

/// The form of the build `id`: its name, kind and edition, whether it divides, its ceilings, and Remove.
fn form(ui: &mut egui::Ui, state: &mut AppState, id: &str, chosen: &mut Option<String>) {
    let Some(build) = state.project_carried.container.output(id).cloned() else {
        return;
    };
    ui.add_space(theme::SPACING_TIGHT);
    ui.separator();
    ui.label(egui::RichText::new(&build.name).strong().size(15.0));
    ui.label(
        egui::RichText::new(about(&build.kind))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(theme::SPACING_TIGHT);

    // Name: kept as it is typed, entered with Enter or by leaving the field.
    ui.horizontal(|ui| {
        ui.label(tr!("Name"));
        let key = egui::Id::new(("builds_rename", id));
        let mut draft: String = ui
            .data_mut(|d| d.get_temp(key))
            .unwrap_or_else(|| build.name.clone());
        let field = ui.add(egui::TextEdit::singleline(&mut draft).desired_width(220.0));
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Resource name of the build"))
        });
        let checked = state
            .project_carried
            .container
            .check_build_name(&draft, Some(id));
        if field.lost_focus() && draft != build.name && checked.is_ok() {
            let what = tr!("Renamed the build {name} to {draft}", draft = draft, name = build.name);
            let named = draft.clone();
            if let Err(why) = state.edit_build(id, what, |o| o.name = named) {
                state.show_error_toast(why);
            }
        }
        match &checked {
            Err(why) if draft != build.name => {
                ui.label(egui::RichText::new(why).small().color(theme::ERROR));
            }
            // ⚠ A draft that is the name, or was entered, is the name from now on.
            _ if !field.has_focus() => draft = build.name.clone(),
            _ => {}
        }
        ui.data_mut(|d| d.insert_temp(key, draft));
    })
    .response
    .on_hover_text(tr!("The folder the build writes in build/. Renaming leaves the folder of the old name there until Clean"));

    ui.horizontal(|ui| {
        let mut kind = build.kind.clone();
        ui.label(tr!("Kind"));
        let shown = egui::ComboBox::from_id_salt(("builds_kind", id))
            .selected_text(kind.words())
            .show_ui(ui, |ui| {
                for choice in KINDS {
                    let label = choice.label();
                    ui.selectable_value(&mut kind, choice, label);
                }
            });
        named_choice(&shown.response, tr!("Kind of the build"), &build.kind.words());
        if kind != build.kind {
            let what = tr!("The build {name} is now a {kind}", name = build.name, kind = kind.words());
            let _ = state.edit_build(id, what, |o| o.kind = kind);
        }
        let mut target = build.target;
        ui.label(tr!("Edition"));
        let shown = egui::ComboBox::from_id_salt(("builds_target", id))
            .selected_text(edition_words(target))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target, None, tr!("Keep as made"));
                for choice in [Target::Legacy, Target::Enhanced] {
                    let label = choice.generation().edition_label();
                    ui.selectable_value(&mut target, Some(choice), label);
                }
            });
        named_choice(&shown.response, tr!("Edition of the build"), edition_words(build.target));
        shown.response.on_hover_text(
            tr!("Every drawable and texture dictionary the build ships is moved to this edition's layout, and what the edition has no field for is named"),
        );
        if target != build.target {
            let what = tr!("The build {name} ships {edition}", name = build.name, edition = edition_words(target));
            let _ = state.edit_build(id, what, |o| o.target = target);
        }
    });

    let mut split = build.split;
    let check = ui.checkbox(
        &mut split,
        tr!("Divide into several resources when over a ceiling"),
    );
    check.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            build.split,
            tr!("Divide into several resources when over a ceiling"),
        )
    });
    check.on_hover_text(
        tr!("A build over a ceiling is cut between contents into <name>_1, <name>_2 and on, and never inside one. A split for update cadence is two builds over different contents, which the matrix below sets."),
    );
    if split != build.split {
        let what = if split {
            tr!("The build {name} divides over a ceiling", name = build.name)
        } else {
            tr!(
                "The build {name} no longer divides over a ceiling",
                name = build.name
            )
        };
        let _ = state.edit_build(id, what, |o| o.split = split);
    }

    ceilings(ui, state, &build);

    let mut ship = build.ship_unchanged;
    let check = ui.checkbox(
        &mut ship,
        tr!("Ship a source that is still the game's own file"),
    );
    check.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            build.ship_unchanged,
            tr!("Ship a source that is still the game's own file"),
        )
    });
    check.on_hover_text(tr!("Off, the build leaves out a source taken from the game that has not changed. On, it ships, as a pack that replaces the game's file needs."));
    if ship != build.ship_unchanged {
        let what = tr!(
            "The build {name} ships unchanged originals: {ship}",
            ship = ship,
            name = build.name
        );
        let _ = state.edit_build(id, what, |o| o.ship_unchanged = ship);
    }

    super::build_options::draw(ui, state, &build);

    let resources = state.workbench.resources_of(id);
    let stopping = state.workbench.counts(Scope::Build(id)).stopping();
    let said = match resources {
        [] => tr!("Not planned yet").to_string(),
        [only] => tr!(
            "Plan: {files}, {size}, {stopping}",
            stopping = stopping,
            files = crate::state::front_door::counted(only.plan.totals.files as usize, "file"),
            size = super::size(only.plan.totals.bytes)
        ),
        many => tr!(
            "Plan: {files}, {size}, {stopping}",
            stopping = stopping,
            files = crate::state::front_door::counted(many.len(), "resource"),
            size = crate::state::front_door::counted(
                many.iter().map(|r| r.plan.totals.files).sum::<u64>() as usize,
                "file"
            )
        ),
    };
    ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));

    ui.add_space(theme::SPACING_TIGHT);
    let running = state.workbench.builds().is_running();
    let remove = ui
        .add_enabled(!running, egui::Button::new(tr!("Remove build")))
        .on_hover_text(tr!(
            "Stop declaring the build. What it already wrote stays until Clean"
        ))
        .on_disabled_hover_text(tr!("A build is running"));
    if remove.clicked() {
        state.remove_build(id);
        *chosen = None;
    }
}

/// The ceilings of a build: files, size, and each figure its content contributes, with what the plan uses of each.
///
/// ⚠ Folded, and closed until one is set: twelve ticks nobody has ticked pushed the matrix, which this window is for, off the screen.
fn ceilings(ui: &mut egui::Ui, state: &mut AppState, build: &Output) {
    let set = usize::from(build.max_files.is_some())
        + usize::from(build.max_bytes.is_some())
        + build.limits.len();
    let title = match set {
        0 => tr!("Limits · none set").to_string(),
        n => tr!("Limits · {n} set", n = n),
    };
    let (files, bytes, figures) = {
        let resources = state.workbench.resources_of(&build.id);
        let plan = resources.first().map(|r| &r.plan);
        let files = plan.map_or(0, |p| p.totals.files);
        let bytes = plan.map_or(0, |p| p.totals.bytes);
        let mut figures: Vec<(String, u64)> = (plan.iter())
            .flat_map(|p| p.budgets.iter().map(|b| (b.name.clone(), b.used)))
            .collect();
        for named in build.limits.keys() {
            if !figures.iter().any(|(name, _)| name == named) {
                figures.push((named.clone(), 0));
            }
        }
        (files, bytes, figures)
    };

    // ⚠ A number being dragged or typed changes the build as it moves and is said once, when it is let go.
    let id = build.id.as_str();
    let mut busy = false;
    egui::CollapsingHeader::new(title)
        .id_salt(("builds_limits", id, set > 0))
        .default_open(set > 0)
        .show(ui, |ui| {
            if let Some(new) = count_row(
                ui,
                tr!("Files"),
                build.max_files,
                files.max(1),
                files,
                &mut busy,
            ) {
                let what = tr!(
                    "The build {name} allows {limit} files",
                    name = build.name,
                    limit = shown(new)
                );
                let _ = state.edit_build_live(id, "files", what, |o| o.max_files = new);
            }
            if let Some(new) = size_row(ui, tr!("Size"), build.max_bytes, bytes, &mut busy) {
                let what = tr!(
                    "The build {name} allows {limit} bytes",
                    name = build.name,
                    limit = shown(new)
                );
                let _ = state.edit_build_live(id, "bytes", what, |o| o.max_bytes = new);
            }
            for (name, used) in figures {
                let words = figure_words(&name);
                let current = build.limits.get(&name).copied();
                // A figure that counts bytes is set in MiB, as the size is: a number of single bytes cannot be dragged.
                let changed = if figure_is_bytes(&name) {
                    size_row(ui, &words, current, used, &mut busy)
                } else {
                    count_row(ui, &words, current, used.max(1), used, &mut busy)
                };
                if let Some(new) = changed {
                    let what = tr!(
                        "The build {name} allows {limit} {words}",
                        name = build.name,
                        limit = shown(new),
                        words = words
                    );
                    let _ = state.edit_build_live(id, &name, what, |o| match new {
                        Some(max) => {
                            o.limits.insert(name.clone(), max);
                        }
                        None => {
                            o.limits.remove(&name);
                        }
                    });
                }
            }
        });
    if !busy {
        state.settle_build_edit();
    }
}

fn shown(ceiling: Option<u64>) -> String {
    ceiling.map_or("no".to_string(), |max| max.to_string())
}

/// A ceiling on a count: a check that turns it on and its number; `Some` once the person changed it, and `busy` is set while its number is being dragged or typed.
fn count_row(
    ui: &mut egui::Ui,
    words: &str,
    current: Option<u64>,
    start: u64,
    used: u64,
    busy: &mut bool,
) -> Option<Option<u64>> {
    let mut changed = None;
    ui.horizontal(|ui| {
        let mut on = current.is_some();
        let mut value = current.unwrap_or(start);
        let label = tr!("{words} at most", words = words);
        let check = ui.checkbox(&mut on, &label);
        check.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, current.is_some(), &label)
        });
        let mut dragged = false;
        if on {
            let drag = ui.add(
                egui::DragValue::new(&mut value)
                    .range(0..=u64::MAX / 4)
                    .speed(1.0),
            );
            drag.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    true,
                    tr!("{words} ceiling", words = words),
                )
            });
            dragged = drag.changed();
            *busy |= drag.dragged() || drag.has_focus();
        }
        ui.label(
            egui::RichText::new(tr!("({used} now)", used = used))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        if check.changed() {
            changed = Some(on.then_some(value));
        } else if dragged {
            changed = Some(Some(value));
        }
    });
    changed
}

/// The ceiling on a count of bytes the build contributes, in MiB; `busy` is set as [`count_row`] sets it.
fn size_row(
    ui: &mut egui::Ui,
    words: &str,
    current: Option<u64>,
    used: u64,
    busy: &mut bool,
) -> Option<Option<u64>> {
    const MIB: f64 = 1_048_576.0;
    let mut changed = None;
    ui.horizontal(|ui| {
        let mut on = current.is_some();
        let mut mib =
            current.map_or_else(|| (used as f64 / MIB).ceil().max(1.0), |b| b as f64 / MIB);
        let label = tr!("{words} at most", words = words);
        let check = ui.checkbox(&mut on, &label);
        check.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, current.is_some(), &label)
        });
        let mut dragged = false;
        if on {
            let drag = ui.add(
                egui::DragValue::new(&mut mib)
                    .range(0.0..=1.0e7)
                    .speed(1.0)
                    .fixed_decimals(1)
                    .suffix(tr!(" MiB")),
            );
            drag.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    true,
                    tr!("{words} ceiling in MiB", words = words),
                )
            });
            dragged = drag.changed();
            *busy |= drag.dragged() || drag.has_focus();
        }
        ui.label(
            egui::RichText::new(tr!("({size} now)", size = super::size(used)))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        let bytes = (mib * MIB).round() as u64;
        if check.changed() {
            changed = Some(on.then_some(bytes));
        } else if dragged {
            changed = Some(Some(bytes));
        }
    });
    changed
}

/// Contents by builds: a cell is ticked when the build takes the content, and locked, with its reason, when that kind of build cannot ship it.
fn matrix(ui: &mut egui::Ui, state: &mut AppState) {
    let container = state.project_carried.container.clone();
    ui.add_space(theme::SPACING_SNUG);
    ui.separator();
    ui.label(egui::RichText::new(tr!("Content by builds")).strong());
    ui.label(
        egui::RichText::new(tr!("Tick the content each build ships. A split for update cadence is two builds over different contents."))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    let mut reasons: Vec<String> = Vec::new();
    egui::Grid::new("builds_matrix")
        .striped(true)
        .num_columns(container.outputs.len() + 1)
        .show(ui, |ui| {
            ui.label("");
            for build in &container.outputs {
                ui.label(egui::RichText::new(&build.name).strong())
                    .on_hover_text(build.kind.words());
            }
            ui.end_row();
            for module in &container.modules {
                let words = content_words(&container, &module.id);
                ui.label(&words);
                for build in &container.outputs {
                    let taken = build.take.contains(&module.id);
                    let lock = state
                        .workbench
                        .cannot_ship(&module.id, &build.kind)
                        .map(str::to_string);
                    // ⚠ A locked cell that is ticked can still be unticked: the build would be refused, and the way out is not to take it.
                    let enabled = taken || lock.is_none();
                    let mut on = taken;
                    let name = tr!("{words} in {build}", words = words, build = build.name);
                    ui.horizontal(|ui| {
                        let cell = ui.add_enabled(enabled, egui::Checkbox::without_text(&mut on));
                        let spoken = match &lock {
                            Some(why) => tr!("{name}, locked: {why}", name = name, why = why),
                            None => name.clone(),
                        };
                        cell.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                enabled,
                                taken,
                                &spoken,
                            )
                        });
                        if let Some(why) = &lock {
                            ui.label(
                                egui::RichText::new(egui_phosphor::regular::LOCK)
                                    .color(theme::WARNING),
                            )
                            .on_hover_text(why);
                            if taken {
                                reasons.push(tr!(
                                    "{name} would be refused: {why}",
                                    name = name,
                                    why = why
                                ));
                            } else {
                                reasons.push(tr!(
                                    "{words} cannot go in {name}: {why}",
                                    words = words,
                                    why = why,
                                    name = build.name
                                ));
                            }
                        }
                        if cell.changed() {
                            if let Err(why) = state.take_content(&build.id, &module.id, on) {
                                state.show_error_toast(why);
                            }
                        }
                    });
                }
                ui.end_row();
            }
        });
    let to_the_types = reasons.iter().any(|reason| reason.contains("File types"));
    for reason in reasons {
        ui.add(
            egui::Label::new(
                egui::RichText::new(reason)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    }
    // ⚠ The place the reason points at is further down this window: a button takes the person there.
    if to_the_types
        && ui
            .small_button(tr!("Go to File types"))
            .on_hover_text(tr!(
                "Open File types, where the type of each file of a singleplayer DLC is stated"
            ))
            .clicked()
    {
        super::builds_file_types::ask_to_open(ui.ctx());
    }
}

/// The new build: its name, kind, edition and the content it takes.
fn new_build(
    ui: &mut egui::Ui,
    state: &mut AppState,
    adding: &mut Option<NewBuild>,
    chosen: &mut Option<String>,
) {
    let Some(mut draft) = adding.take() else {
        return;
    };
    ui.label(egui::RichText::new(tr!("New build")).strong());
    let container = state.project_carried.container.clone();
    let (first_name, first_kind, first_target) =
        (draft.name.clone(), draft.kind.clone(), draft.target);
    let device = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let refused = match draft.name.is_empty() {
        true => Some(tr!("Name the build").to_string()),
        false => container
            .check_build_name(&draft.name, None)
            .err()
            .or_else(|| {
                (draft.kind == OutputKind::SingleplayerDlc && !draft.name.chars().all(device)).then(
                    || {
                        tr!("A singleplayer DLC name can use letters, digits and underscores only")
                            .to_string()
                    },
                )
            }),
    };
    ui.label(
        egui::RichText::new(about(&draft.kind))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    let mut answer: Option<bool> = None;
    ui.horizontal(|ui| {
        ui.label(tr!("Kind"));
        let shown = egui::ComboBox::from_id_salt("builds_new_kind")
            .selected_text(draft.kind.words())
            .show_ui(ui, |ui| {
                for choice in KINDS {
                    let label = choice.label();
                    ui.selectable_value(&mut draft.kind, choice, label);
                }
            });
        named_choice(&shown.response, tr!("Kind"), &draft.kind.words());
        ui.label(tr!("Edition"));
        let edition = edition_words(draft.target);
        let shown = egui::ComboBox::from_id_salt("builds_new_target")
            .selected_text(edition)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut draft.target, None, tr!("Keep as made"));
                for choice in [Target::Legacy, Target::Enhanced] {
                    let label = choice.generation().edition_label();
                    ui.selectable_value(&mut draft.target, Some(choice), label);
                }
            });
        named_choice(&shown.response, tr!("Edition"), edition);
    });
    // The name follows what the build is until somebody types one.
    if !draft.named && (draft.kind != first_kind || draft.target != first_target) {
        draft.name = state.default_build_name(&draft.kind, draft.target);
    }
    ui.horizontal(|ui| {
        ui.label(tr!("Name"));
        let field = ui.text_edit_singleline(&mut draft.name);
        field.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("Name of the new build"),
            )
        });
        if field.changed() {
            draft.named = true;
        }
        // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
        if draft.focus && !field.has_focus() {
            field.request_focus();
        } else {
            draft.focus = false;
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && refused.is_none()
        {
            answer = Some(true);
        }
    });
    if !draft.name.is_empty() {
        ui.label(
            egui::RichText::new(tr!("Written to build/{name}", name = draft.name))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
    // A kind of build that cannot ship a content leaves it out of the new build, and does not take it back.
    if draft.kind != first_kind {
        draft
            .take
            .retain(|id| state.workbench.cannot_ship(id, &draft.kind).is_none());
    }
    if container.modules.len() >= 2 {
        ui.label(tr!("Takes"));
        for module in &container.modules {
            let mut on = draft.take.contains(&module.id);
            let words = content_words(&container, &module.id);
            let lock = state.workbench.cannot_ship(&module.id, &draft.kind);
            let check = ui.add_enabled(lock.is_none(), egui::Checkbox::new(&mut on, &words));
            let spoken = match lock {
                Some(why) => tr!(
                    "New build takes {words}, locked: {why}",
                    words = words,
                    why = why
                ),
                None => tr!("New build takes {words}", words = words),
            };
            check.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    lock.is_none(),
                    draft.take.contains(&module.id),
                    &spoken,
                )
            });
            if let Some(why) = lock {
                ui.label(
                    egui::RichText::new(format!("{} {why}", egui_phosphor::regular::LOCK))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
            if check.changed() {
                draft.take.retain(|id| id != &module.id);
                if on {
                    draft.take.push(module.id.clone());
                }
                draft.edited = true;
            }
        }
    }
    // ⚠ Held until something is changed: a field nobody has touched is not a mistake to name.
    draft.edited |=
        draft.name != first_name || draft.kind != first_kind || draft.target != first_target;
    if let Some(why) = refused.as_ref().filter(|_| draft.edited) {
        ui.label(egui::RichText::new(why).color(theme::ERROR));
    }
    ui.horizontal(|ui| {
        let mut add = ui.add_enabled(refused.is_none(), egui::Button::new(tr!("Add build")));
        if let Some(why) = &refused {
            add = add.on_disabled_hover_text(why);
        }
        if add.clicked() {
            answer = Some(true);
        }
        if ui.button(tr!("Cancel")).clicked() {
            answer = Some(false);
        }
    });
    match answer {
        Some(true) => {
            let (name, kind, target, take) = (
                draft.name.clone(),
                draft.kind.clone(),
                draft.target,
                draft.take.clone(),
            );
            match state.add_build_taking(&name, kind, target, &take) {
                Ok(id) => *chosen = Some(id),
                Err(why) => {
                    state.show_error_toast(why);
                    *adding = Some(draft);
                }
            }
        }
        Some(false) => {}
        None => *adding = Some(draft),
    }
}

#[cfg(test)]
mod tests {
    use or_project::{Module, Output};

    use super::*;

    fn declared(contents: &[&str], builds: &[&str]) -> Container {
        Container {
            modules: contents.iter().map(|id| Module::files(id)).collect(),
            outputs: builds
                .iter()
                .map(|id| Output::fivem_resource(id, id, contents))
                .collect(),
        }
    }

    #[test]
    fn the_matrix_is_there_from_two_contents_or_two_builds_and_not_before() {
        assert!(!shows_matrix(&declared(&["files"], &["pack"])), "1 x 1");
        assert!(
            shows_matrix(&declared(&["files", "hats"], &["pack"])),
            "2 x 1"
        );
        assert!(
            shows_matrix(&declared(&["files"], &["pack", "dlc"])),
            "1 x 2"
        );
        assert!(shows_matrix(&declared(&["a", "b"], &["x", "y"])), "2 x 2");
        assert!(
            !shows_matrix(&declared(&["files", "hats"], &[])),
            "with no build there is nothing to tick"
        );
    }

    #[test]
    fn a_content_is_said_by_its_kind_and_by_its_id_only_when_that_adds_something() {
        let mut container = declared(&["files"], &[]);
        container
            .modules
            .push(Module::clothing("uniforms", "uniforms"));
        container
            .modules
            .push(Module::clothing("clothing", "clothing"));
        container.modules.push(Module::linked(
            "brand",
            or_project::ModuleKind::Files,
            "../brand",
        ));
        assert_eq!(content_words(&container, "files"), "Files");
        assert_eq!(content_words(&container, "uniforms"), "Clothing uniforms");
        assert_eq!(content_words(&container, "clothing"), "Clothing");
        assert_eq!(content_words(&container, "brand"), "Files brand (linked)");
    }
}
