//! The Content and Builds panel: what the project holds, with its problems, and what it builds, with its state.

use crate::state::front_door::KindWords as _;
use or_project::{Module, Output};

use super::assets::AssetsUi;
use super::{command_button, fitted, REFRESH};
use crate::state::workbench::{content_name, BuildDot, Scope};
use crate::state::AppState;
use crate::ui::commands::Action;
use crate::ui::dock::Workspace;
use crate::ui::task_screens;
use crate::ui::theme;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(name) = state.project_name() else {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    };
    ui.ctx().request_repaint_after(REFRESH);
    let modules: Vec<Module> = state.project_carried.container.modules.clone();
    let outputs: Vec<Output> = state.project_carried.container.outputs.clone();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(egui::RichText::new(&name).strong());
            ui.add_space(theme::SPACING_DEFAULT);

            let heading = section(ui, state, tr!("Content"), Action::AddContent);
            heading.context_menu(|ui| new_menu(ui, state));
            if modules.is_empty() {
                ui.label(egui::RichText::new(tr!("No content yet")).color(theme::TEXT_SECONDARY));
            }
            for module in &modules {
                content_row(ui, state, module);
            }

            ui.add_space(theme::SPACING_DEFAULT);
            section(ui, state, tr!("Builds"), Action::AddBuild);
            if outputs.is_empty() {
                ui.label(egui::RichText::new(tr!("No build yet")).color(theme::TEXT_SECONDARY));
            }
            let chosen = state.active_output_id();
            for output in &outputs {
                build_row(
                    ui,
                    state,
                    output,
                    chosen.as_deref() == Some(output.id.as_str()),
                );
            }
        });
}

/// What New and Add offer a person who right-clicks Content: a new source of each kind that starts empty, and the ways in that decompile.
fn new_menu(ui: &mut egui::Ui, state: &mut AppState) {
    crate::ui::popup_of_the_shell(ui.ctx());
    ui.menu_button(tr!("New"), |ui| {
        command_button(ui, state, Action::NewTexturesSource)
    });
    for action in [
        Action::AddContent,
        Action::ImportGameFiles,
        Action::ImportFolder,
        Action::AddImagesAsTexturesSource,
    ] {
        command_button(ui, state, action);
    }
}

/// A heading and the `+` that adds to it; the heading, which answers a right click.
fn section(ui: &mut egui::Ui, state: &mut AppState, title: &str, add: Action) -> egui::Response {
    ui.horizontal(|ui| {
        let heading = ui.label(egui::RichText::new(title).color(theme::TEXT_SECONDARY));
        let heading = heading.interact(egui::Sense::click());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add_it = crate::ui::commands::of(state, add);
            let enabled = add_it.as_ref().is_some_and(|c| c.enabled.is_ok());
            let plus = ui
                .add_enabled(enabled, egui::Button::new("+").small())
                .on_hover_text(add_it.as_ref().map_or("", |c| c.label.as_str()))
                .on_disabled_hover_text(
                    add_it
                        .as_ref()
                        .and_then(|c| c.enabled.err())
                        .unwrap_or_default(),
                );
            plus.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    enabled,
                    format!(
                        "{title}: {}",
                        add_it.as_ref().map_or("", |c| c.label.as_str())
                    ),
                )
            });
            if plus.clicked() {
                if let Some(command) = add_it {
                    crate::ui::commands::run(state, ui.ctx(), &command);
                }
            }
        });
        heading
    })
    .inner
}

fn content_row(ui: &mut egui::Ui, state: &mut AppState, module: &Module) {
    let counts = state.workbench.counts(Scope::Content(&module.id));
    let word = content_name(&state.project_carried.container, &module.id);
    let filtered = state.panels.get::<AssetsUi>().content.as_deref() == Some(module.id.as_str());
    let linked = module.is_link();
    let held = state.workbench.held_by(module);
    ui.horizontal_wrapped(|ui| {
        // ⚠ That a content is linked is said once, by the banner under its row.
        let text = format!("{word}  ·  {held}");
        let row = ui.selectable_label(filtered, text).on_hover_text(tr!(
            "{word}, {held}. Click to list only its assets in the Assets table",
            word = word,
            held = held
        ));
        if row.clicked() {
            state.workbench.selected_content = Some(module.id.clone());
        }
        row.context_menu(|ui| {
            crate::ui::popup_of_the_shell(ui.ctx());
            ui.menu_button(tr!("Open in"), |ui| open_in_items(ui, state, module));
            ui.separator();
            new_menu(ui, state);
        });
        let screen = task_screens::of_kind(&module.kind).filter(|_| row.clicked());
        if let Some(screen) = screen {
            // Its content is drawn on a screen of its own, which this opens.
            (screen.show)(state, &module.id);
            state.active_workspace = screen.workspace;
        } else if row.clicked() {
            let ui_state = state.panels.get::<AssetsUi>();
            ui_state.content = (!filtered).then(|| module.id.clone());
        }
    });
    // ⚠ On a line of its own under the name, as the builds' states are: errors and warnings by their names, since a bare `problems` said one number here and another beside a build.
    if let Some(worst) = counts.worst() {
        ui.label(
            egui::RichText::new(format!("   ● {}", counts.said()))
                .small()
                .color(super::severity_colour(worst)),
        )
        .on_hover_text(tr!(
            "An error stops a build that ships this content; a warning does not. See Problems"
        ));
    }
    if linked {
        link_banner(ui, state, module);
    }
}

/// The banner under linked content: who writes it, where it leads, and what is wrong with it.
///
/// ⚠ A link is edited at its source. This project reads the folder into its own builds and
/// writes nothing in it, so there is no way to edit it from here, only to open the project that
/// holds it or to stop linking it.
fn link_banner(ui: &mut egui::Ui, state: &mut AppState, module: &Module) {
    let Some(link) = state.workbench.link(&module.id).cloned() else {
        ui.label(
            egui::RichText::new(tr!("Linked: reading where the folder is…"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        return;
    };
    ui.indent(("link_banner", &module.id), |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(link.banner())
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
        // ⚠ Broken after a separator, never inside a name, whatever the width of the panel.
        let font = egui::FontId::monospace(egui::TextStyle::Small.resolve(ui.style()).size);
        let folder = fitted(ui, &link.folder.display().to_string(), &font, ui.available_width());
        ui.add(
            egui::Label::new(
                egui::RichText::new(folder)
                    .small()
                    .monospace()
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
        if let Some(problem) = &link.problem {
            ui.add(egui::Label::new(egui::RichText::new(problem).small().color(theme::ERROR)).wrap());
        }
        ui.horizontal(|ui| {
            let id = module.id.clone();
            let open = ui
                .add_enabled(link.owner.is_some(), egui::Button::new(tr!("Open project")).small())
                .on_hover_text(tr!("Open the project that holds this folder in a window of its own, where it is edited"))
                .on_disabled_hover_text(tr!("No project holds this folder"));
            if open.clicked() {
                state.open_linked_source(&id);
            }
            if ui
                .add(egui::Button::new(tr!("Unlink")).small())
                .on_hover_text(tr!("Stop reading this folder into the project; nothing in it is touched"))
                .clicked()
            {
                state.workbench.selected_content = Some(id);
                crate::ui::commands::run_action(state, ui.ctx(), Action::UnlinkContent);
            }
        });
    });
}

/// Each workspace the project offers, and whether it shows `module` or why not.
fn where_content_opens(
    state: &AppState,
    module: &Module,
) -> Vec<(Workspace, Result<(), &'static str>)> {
    (state.workspaces_offered().into_iter())
        .map(|ws| {
            let answer = match (ws, task_screens::of_workspace(ws)) {
                (Workspace::Project, _) => Ok(()),
                (_, Some(screen)) if screen.kind == module.kind => Ok(()),
                (_, Some(screen)) if !screen.edits_files => {
                    Err(crate::i18n::tr(screen.other_content))
                }
                _ => Err(tr!(
                    "This workspace shows a document: open one of the content's assets"
                )),
            };
            (ws, answer)
        })
        .collect()
}

/// The items of Open in for `module`: every workspace, since a workspace is a layout and not a kind of content; one that cannot show it opens and says why.
fn open_in_items(ui: &mut egui::Ui, state: &mut AppState, module: &Module) {
    for (ws, answer) in where_content_opens(state, module) {
        if super::open_in_item(ui, ws, answer) {
            ui.close_kind(egui::UiKind::Menu);
            if let Err(why) = answer {
                state.open_workspace_that_shows_nothing_of(ws, &module.id, why);
                continue;
            }
            match ws {
                // The Assets table, listing only what this content holds.
                Workspace::Project => {
                    state.panels.get::<AssetsUi>().content = Some(module.id.clone());
                }
                _ => {
                    if let Some(screen) = task_screens::of_workspace(ws) {
                        (screen.show)(state, &module.id);
                    }
                }
            }
            state.active_workspace = ws;
        }
    }
}

fn build_row(ui: &mut egui::Ui, state: &mut AppState, output: &Output, chosen: bool) {
    let dot = state.workbench.dot(&output.id);
    // ⚠ The errors of the plan, which the Build tab and the status bar say too: a row that said one while the tab said four was two numbers for one thing.
    let counts = state.workbench.counts(Scope::Build(&output.id));
    let mut kind = output.kind.words();
    if let Some(target) = output.target {
        kind = format!("{kind}, {}", target.generation().edition_label());
    }
    let colour = match dot {
        BuildDot::Never => theme::TEXT_SECONDARY,
        BuildDot::Running => theme::INFO,
        BuildDot::Built => theme::SUCCESS,
        BuildDot::Failed => theme::ERROR,
        BuildDot::Blocked | BuildDot::Unchecked => theme::WARNING,
    };
    let id = output.id.clone();
    // ⚠ The dot and the name on one line, and the name wrapping beside it: a name too long for what the dot leaves went to the next line and left the dot alone.
    let response = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new("●").color(colour))
                .on_hover_text(dot.label());
            ui.add(egui::Button::selectable(chosen, format!("{}  ·  {kind}", output.name)).wrap())
                .on_hover_text(tr!("Writes build/{name}. {dot}. Click to make it the build the Build button builds", name = output.name, dot = dot.label()))
        })
        .inner;
    // ⚠ On a line of its own under the name, where a narrow panel cannot split it from the name by the width of a word: the state in words, as the status bar says it, so it does not rest on the dot's colour.
    ui.label(
        egui::RichText::new(format!("   {}", dot.word(counts)))
            .small()
            .color(colour),
    )
    .on_hover_text(if counts.errors > 0 {
        counts.stopping()
    } else {
        dot.label().to_string()
    });
    if response.clicked() {
        state.workbench.selected_output = Some(id);
    }
    // ⚠ The row is a selectable label: a double click is how a person asks to change the build they are looking at.
    if response.double_clicked() {
        state.ask_about_builds(false);
    }
    response.context_menu(|ui| {
        crate::ui::popup_of_the_shell(ui.ctx());
        if state.workbench.selected_output.as_deref() != Some(output.id.as_str()) {
            state.workbench.selected_output = Some(output.id.clone());
        }
        for action in [
            Action::BuildProject,
            Action::BuildAll,
            Action::EditBuild,
            Action::RemoveBuild,
        ] {
            command_button(ui, state, action);
        }
    });
}
