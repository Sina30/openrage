//! The start page's own parts: the ways in, the goals of a first launch and the recent projects as cards.
//!
//! The installs, the recent files and the keys stay in [`super::game_picker`], as they were.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_phosphor::regular as icon;

use crate::platform::file_dialogs;
use crate::state::front_door::{Card, Step, Template};
use crate::state::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::panels::status_bar::elide_middle;
use crate::ui::theme;

/// How many recent projects the page shows as cards.
const CARDS_SHOWN: usize = 5;

/// A template the page starts a project on with one click: its chip has the name of the template, and its hover says what the template makes.
struct Goal {
    title: &'static str,
    text: &'static str,
    template: Template,
}

/// The goals in the order of the page: the Files project, the template of each task screen that has one, and the project from a folder.
///
/// ⚠ Only goals whose content exists: a task screen's template brings its chip with its row.
fn goals() -> Vec<Goal> {
    let mut goals = vec![Goal {
        title: Template::Files.label(),
        text: tr!("Scripts, interface and configuration, copied into a FiveM resource."),
        template: Template::Files,
    }];
    for starts in crate::ui::task_screens::all()
        .into_iter()
        .filter_map(|screen| screen.starts.as_ref())
    {
        goals.push(Goal {
            title: starts.template.label(),
            text: crate::i18n::tr(starts.goal),
            template: starts.template,
        });
    }
    goals.push(Goal {
        title: Template::Adopt.label(),
        text: tr!("A folder you already have, with nothing in it moved or rewritten."),
        template: Template::Adopt,
    });
    goals
}

/// What a click on a project's card asked for.
enum Ask {
    Open(PathBuf),
    NewWindow(PathBuf),
    Locate(PathBuf),
    Remove(PathBuf),
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::SPACING_RELAXED * 2.0);
    ui.label(egui::RichText::new(text).size(16.0).strong());
    ui.add_space(theme::SPACING_TIGHT);
}

/// Back, while the page is raised over the shell: it goes back to the workspace it covers, which has something to show while a document or a project is open.
pub fn draw_back(ui: &mut egui::Ui, state: &mut AppState) {
    let something_to_go_back_to = !state.documents.is_empty() || state.project_path.is_some();
    let covering = state.front_door.raised(state.active_workspace) && something_to_go_back_to;
    if covering
        && ui
            .button(tr!("{icon} Back", icon = icon::ARROW_LEFT))
            .clicked()
    {
        state.hide_start_page();
    }
}

/// What starts a project: New project…, the one filled button of the page since it is the one that makes something, and a chip for each template, which opens New project on it.
pub fn draw_new(ui: &mut egui::Ui, state: &mut AppState) {
    heading(ui, tr!("What do you want to make?"));
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        way_in(
            ui,
            state,
            Action::CreateProject,
            tr!("New project…"),
            icon::PLUS,
            true,
        );
        for goal in goals() {
            if ui.button(goal.title).on_hover_text(goal.text).clicked() {
                chosen = Some(goal.template);
            }
        }
    });
    if let Some(template) = chosen {
        state.start_new_project_on(template, Step::Details);
    }
}

/// Open project…, Import built pack…, Open folder… and Open file…: every way to what is already there is a button, and nothing a modder must hunt for in a menu.
pub fn draw_ways_in(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(theme::SPACING_DEFAULT);
    ui.horizontal_wrapped(|ui| {
        way_in(
            ui,
            state,
            Action::OpenProject,
            tr!("Open project…"),
            icon::FOLDER_STAR,
            false,
        );
        way_in(
            ui,
            state,
            Action::OpenBuiltPack,
            tr!("Import built pack…"),
            icon::PACKAGE,
            false,
        );
        way_in(
            ui,
            state,
            Action::OpenFolder,
            tr!("Open folder…"),
            icon::FOLDER_OPEN,
            false,
        );
        way_in(
            ui,
            state,
            Action::Open,
            tr!("Open file…"),
            icon::FILE,
            false,
        );
    });
}

fn way_in(
    ui: &mut egui::Ui,
    state: &mut AppState,
    action: Action,
    label: &str,
    glyph: &str,
    filled: bool,
) {
    let Some(command) = commands::of(state, action) else {
        return;
    };
    let text = egui::RichText::new(format!("{glyph} {label}"));
    let button = if filled {
        egui::Button::new(text.color(theme::ON_ACCENT.get())).fill(theme::ACCENT.get())
    } else {
        egui::Button::new(text)
    };
    let response = ui.add_enabled(command.enabled.is_ok(), button);
    if commands::hints(response, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
    }
}

/// The way for someone who does not know where to start: the sample, the tour and the guide, in one row.
pub fn draw_tips(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(theme::SPACING_RELAXED);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(tr!("Not sure where to start?")).color(theme::TEXT_SECONDARY));
        way_in(
            ui,
            state,
            Action::OpenSampleProject,
            tr!("Open the sample project"),
            icon::FLASK,
            false,
        );
        way_in(
            ui,
            state,
            Action::TakeTour,
            tr!("Take the tour"),
            icon::ARROW_RIGHT,
            false,
        );
        way_in(
            ui,
            state,
            Action::OpenGuide,
            tr!("Read the guide"),
            icon::BOOK_OPEN,
            false,
        );
    });
}

/// The recent projects as cards: what each holds, how it is built and when it was opened.
pub fn draw_projects(ui: &mut egui::Ui, state: &mut AppState) {
    heading(ui, tr!("Recent projects"));
    let paths: Vec<PathBuf> = (state.prefs.recent_projects.iter())
        .take(CARDS_SHOWN)
        .cloned()
        .collect();
    let cards: Vec<Option<Card>> = (state.front_door.cards.of(&paths))
        .into_iter()
        .map(|card| card.cloned())
        .collect();
    if state.front_door.cards.is_reading() {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
    let mut asked: Option<Ask> = None;
    for (path, card) in paths.iter().zip(&cards) {
        let opened = state.prefs.project_opened_ago(path);
        theme::card(ui, |ui| {
            ui.set_width(ui.available_width());
            project_card(ui, path, card.as_ref(), opened, &mut asked);
        });
        ui.add_space(theme::SPACING_SNUG);
    }
    match asked {
        Some(Ask::Open(path)) => state.open_project_asking(path),
        Some(Ask::NewWindow(path)) => state.open_project_in_new_window(&path),
        Some(Ask::Locate(old)) => {
            if let Some(new) = file_dialogs::open_project_file() {
                state.locate_recent_project(&old, new);
            }
        }
        Some(Ask::Remove(path)) => state.forget_recent_project(&path),
        None => {}
    }
}

fn project_card(
    ui: &mut egui::Ui,
    path: &Path,
    card: Option<&Card>,
    opened: Option<String>,
    asked: &mut Option<Ask>,
) {
    let name = card.map_or_else(
        || {
            path.file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
        },
        |card| card.name.clone(),
    );
    let gone = card.is_some_and(|card| !card.found);
    if ui.available_width() < NARROW_CARD {
        // ⚠ Too narrow for the buttons beside the name: they go under it, where nothing can be squeezed to a letter a line.
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            card_info(ui, path, &name, card, gone, asked);
            ui.horizontal_wrapped(|ui| card_buttons(ui, path, &name, gone, &opened, asked));
        });
    } else {
        // ⚠ The buttons and the age are laid out first, from the right, and what the project says of itself takes the room they leave: a long path is cut short and never runs under them.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            card_buttons(ui, path, &name, gone, &opened, asked);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                card_info(ui, path, &name, card, gone, asked);
            });
        });
    }
}

/// How narrow a card is before its buttons go under its name.
const NARROW_CARD: f32 = 560.0;

/// The card's buttons, in the order a row from the right lays them: Open first, since it is the card's main action.
fn card_buttons(
    ui: &mut egui::Ui,
    path: &Path,
    name: &str,
    gone: bool,
    opened: &Option<String>,
    asked: &mut Option<Ask>,
) {
    if gone {
        if ui
            .button(tr!("Remove from list"))
            .on_hover_text(tr!("Forget this project; its files are not touched"))
            .clicked()
        {
            *asked = Some(Ask::Remove(path.to_path_buf()));
        }
        if ui
            .button(tr!("Locate…"))
            .on_hover_text(tr!(
                "Point OpenRage at where the project file is now, and open it"
            ))
            .clicked()
        {
            *asked = Some(Ask::Locate(path.to_path_buf()));
        }
        return;
    }
    // ⚠ Open is the card's main action and the first button of its row: a returning person opens far more than they do anything else, and the name beside it is a link only for the hand that finds it.
    let open = ui.button(tr!("Open")).on_hover_text(tr!(
        "Open {name}: {path}",
        name = name,
        path = path.display()
    ));
    open.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Open")));
    if open.clicked() {
        *asked = Some(Ask::Open(path.to_path_buf()));
    }
    let second = ui
        .small_button(tr!(
            "{icon} Open in new window",
            icon = icon::ARROW_SQUARE_OUT
        ))
        .on_hover_text(tr!(
            "Open this project in a second OpenRage window and leave this one as it is"
        ));
    second.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Open in new window"))
    });
    if second.clicked() {
        *asked = Some(Ask::NewWindow(path.to_path_buf()));
    }
    if let Some(when) = opened {
        ui.label(
            egui::RichText::new(tr!("opened {when}", when = when))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
}

/// The card's name in the colour of a link, its path cut in the middle, and what it holds.
fn card_info(
    ui: &mut egui::Ui,
    path: &Path,
    name: &str,
    card: Option<&Card>,
    gone: bool,
    asked: &mut Option<Ask>,
) {
    let title = egui::RichText::new(name).strong().size(15.0);
    if gone {
        ui.add(egui::Label::new(title).truncate());
    } else if ui
        .link(title.color(theme::ACCENT_TEXT.get()))
        .on_hover_text(path.display().to_string())
        .clicked()
    {
        *asked = Some(Ask::Open(path.to_path_buf()));
    }
    // ⚠ Cut in the middle: the end of a path is what tells two projects apart, and the whole of it is the hover's.
    let room = (ui.available_width() / 5.5) as usize;
    ui.add(
        egui::Label::new(
            egui::RichText::new(elide_middle(&path.display().to_string(), room.max(24)))
                .small()
                .color(theme::TEXT_SECONDARY),
        )
        .truncate(),
    )
    .on_hover_text(path.display().to_string());
    details(ui, card);
}

/// What the card says under the path: Not found, why it did not read, or what the project holds and how it is built.
fn details(ui: &mut egui::Ui, card: Option<&Card>) {
    let Some(card) = card else {
        ui.label(
            egui::RichText::new(tr!("Reading…"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        return;
    };
    if !card.found {
        ui.label(
            egui::RichText::new(tr!("Not found"))
                .small()
                .color(theme::ERROR),
        );
    } else if let Some(why) = &card.unreadable {
        ui.label(
            egui::RichText::new(tr!("Could not be read: {why}", why = why))
                .small()
                .color(theme::ERROR),
        );
    } else {
        ui.horizontal(|ui| {
            let content = if card.content.is_empty() {
                tr!("No content yet").to_string()
            } else {
                card.content.join(", ")
            };
            ui.label(egui::RichText::new(content).small());
            ui.label(
                egui::RichText::new("·")
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            let built = card.builds == "Built";
            ui.label(egui::RichText::new(&card.builds).small().color(if built {
                theme::SUCCESS
            } else {
                theme::TEXT_SECONDARY
            }));
        });
    }
}
