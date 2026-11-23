//! The New project dialog, and the question a second project asks while one is open.
//!
//! Two steps, as Visual Studio's is: the template gallery, then the name, the folder, the game and the
//! resource, with the exact tree Create will write beside them. Nothing is written before Create.
//!
//! ⚠ **One size at every step.** A modal keeps the size of its last frame, so the body is a scroll area
//! of a fixed height and the dialog is as large for the gallery as for the details of any template.

use crate::platform::file_dialogs;
use crate::state::front_door::new_project::{Field, Refusal};
use crate::state::front_door::{counted, GameChoice, NewProject, Previews, Step, Switch, Template};
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::panels::status_bar::elide_middle;
use crate::ui::panels::{question_escaped, question_titled};
use crate::ui::theme;

/// How wide the dialog is, and the left column of each step, in a window that has the room.
const WIDTH: f32 = 780.0;
const LEFT: f32 = 360.0;
/// The gap between the columns and the frame of the right one.
const GUTTER: f32 = 30.0;
/// How tall the body of a step is, in a window that has the room, and what the rest of the dialog and the window's edges take.
/// ⚠ Room for every template of the gallery and the way to read another: a template added is one card more.
const BODY: f32 = 500.0;
const BODY_BESIDES: f32 = 200.0;
/// What the left column keeps for the search above its rows and the way to read another template under them.
const ROWS_BESIDES: f32 = 110.0;
/// The frame's margins and a little air.
const WINDOW_EDGES: f32 = 32.0;
/// How many files a folder could not place the dialog lists; the project's Properties lists them all.
const UNPLACED_SHOWN: usize = 6;

/// The dialog's measures in the window it is drawn in.
#[derive(Clone, Copy)]
struct Measures {
    width: f32,
    left: f32,
    aside: f32,
    body: f32,
}

impl Measures {
    fn of(ctx: &egui::Context) -> Self {
        let screen = ctx.content_rect();
        let width = WIDTH.min(screen.width() - WINDOW_EDGES);
        let left = LEFT.min(width * 0.46);
        Self {
            width,
            left,
            aside: width - left - GUTTER,
            body: (screen.height() - BODY_BESIDES).clamp(150.0, BODY),
        }
    }
}

/// The right side of a step: a framed block as tall as its contents.
///
/// ⚠ Not a separator and a column: a vertical separator takes all the height there is, and a modal
/// keeps the height of its last frame, so the dialog grew to the window's height within a few frames.
fn aside(ui: &mut egui::Ui, measures: Measures, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(measures.aside);
        ui.set_max_width(measures.aside);
        // ⚠ A frame takes the layout of the row it is in, which lays its lines side by side and wraps a sentence to a word a line.
        ui.vertical(add_contents);
    });
}

enum Outcome {
    Keep,
    Cancel,
    Create,
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    state.poll_creating();
    new_project(ctx, state);
    switching(ctx, state);
}

/// The title of the dialog: what the details are for, in the name the gallery and the goal cards give it.
fn title_of(draft: &NewProject) -> &'static str {
    match (draft.step, draft.template) {
        (Step::Details, Template::Clothing) => tr!("New clothing pack"),
        (Step::Details, Template::Files) => tr!("New files project"),
        (Step::Details, Template::Adopt) => tr!("New project from a folder"),
        _ => tr!("New project"),
    }
}

fn new_project(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut draft) = state.front_door.new_project.take() else {
        return;
    };
    let busy = state.is_creating();
    let measures = Measures::of(ctx);
    let mut outcome = Outcome::Keep;
    let previews = &mut state.front_door.previews;
    question_titled(
        ctx,
        tr!("New project"),
        title_of(&draft),
        measures.width,
        |ui| {
            // The body is a fixed height, so the buttons stay where they are as the step changes.
            ui.allocate_ui(egui::vec2(measures.width, measures.body), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match draft.step {
                        Step::Gallery => gallery(ui, measures, &mut draft),
                        Step::Details => details(ui, measures, &mut draft, previews),
                    });
            });
            ui.add_space(theme::SPACING_LOOSE);
            buttons(ui, &mut draft, &mut outcome, busy);
        },
    );
    if previews.is_reading() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if question_escaped(ctx, tr!("New project")) && !busy {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Cancel => {}
        Outcome::Keep => state.front_door.new_project = Some(draft),
        Outcome::Create => {
            draft.error = None;
            state.front_door.new_project = Some(draft.clone());
            state.start_creating(draft);
        }
    }
}

/// The row under the body, along the right edge: the button that goes on is the last and the only one filled.
fn buttons(ui: &mut egui::Ui, draft: &mut NewProject, outcome: &mut Outcome, busy: bool) {
    let (primary, can) = match draft.step {
        Step::Gallery => (tr!("Next"), true),
        Step::Details => {
            let label = match (busy, draft.template == Template::Adopt) {
                (true, _) => tr!("Creating…"),
                (false, true) => tr!("Make project"),
                (false, false) => tr!("Create"),
            };
            // ⚠ The look `details` drew its messages from, never `refusal()` or a look of its own: that reads the disk, and this runs every frame.
            (label, draft.clear_by_last_look() && !busy)
        }
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // ⚠ Waiting, it is a plain button with its reason on hover: the accent fill under the dimmed label of a disabled button is navy on navy.
        let go = if can {
            egui::Button::new(egui::RichText::new(primary).color(theme::ON_ACCENT.get()))
                .fill(theme::ACCENT.get())
        } else {
            egui::Button::new(primary)
        };
        let why = if busy {
            tr!("The project is being made")
        } else {
            draft.why_it_waits().unwrap_or(tr!("Reading the folder"))
        };
        let go = ui.add_enabled(can, go).on_disabled_hover_text(why);
        if go.clicked() || (draft.enter_confirms && can) {
            match draft.step {
                Step::Gallery => {
                    draft.step = Step::Details;
                    draft.focus = true;
                }
                Step::Details => *outcome = Outcome::Create,
            }
        }
        if ui
            .add_enabled(!busy, egui::Button::new(tr!("Cancel")))
            .clicked()
        {
            *outcome = Outcome::Cancel;
        }
        if draft.step == Step::Details
            && ui
                .add_enabled(!busy, egui::Button::new(tr!("Back")))
                .clicked()
        {
            draft.step = Step::Gallery;
            draft.error = None;
        }
    });
    draft.enter_confirms = false;
}

fn gallery(ui: &mut egui::Ui, measures: Measures, draft: &mut NewProject) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(measures.left);
            let search = ui.add(
                egui::TextEdit::singleline(&mut draft.search)
                    .hint_text(tr!("Search templates"))
                    .desired_width(f32::INFINITY),
            );
            named(search, tr!("Search templates"));
            ui.add_space(theme::SPACING_SNUG);
            let listed: Vec<Template> = Template::ALL
                .into_iter()
                .filter(|template| template.matches(&draft.search))
                .collect();
            // ⚠ The rows scroll by themselves, under the search and over the way to read another template, so that a template added is a row more and never pushes the details of the one chosen out of the window.
            egui::ScrollArea::vertical()
                .id_salt("template_rows")
                .max_height((measures.body - ROWS_BESIDES).max(120.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for template in &listed {
                        let template = *template;
                        let chosen = draft.template == template;
                        let (clicked, double) =
                            row(ui, measures, chosen, template.label(), template.blurb());
                        if clicked {
                            draft.choose(template);
                        }
                        if double {
                            draft.step = Step::Details;
                            draft.focus = true;
                        }
                    }
                    // The templates other projects made and shared, read from files.
                    let mut chosen_file: Option<usize> = None;
                    for (ix, loaded) in draft.loaded.iter().enumerate() {
                        let shown = draft.template == Template::Shared && draft.loaded_ix == ix;
                        let name = loaded.template.name.as_str();
                        if !Template::Shared.matches(&draft.search)
                            && !name
                                .to_lowercase()
                                .contains(&draft.search.trim().to_lowercase())
                        {
                            continue;
                        }
                        let (clicked, double) =
                            row(ui, measures, shown, name, Template::Shared.blurb());
                        if clicked || double {
                            chosen_file = Some(ix);
                        }
                        if double {
                            draft.step = Step::Details;
                            draft.focus = true;
                        }
                    }
                    if let Some(ix) = chosen_file {
                        draft.choose_loaded(ix);
                    }
                    if listed.is_empty() && draft.loaded.is_empty() {
                        ui.label(
                            egui::RichText::new(tr!("No template matches"))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                });
            // ⚠ Apart from the list, with a divider over it: it is not a fifth template but the way to read another one.
            ui.add_space(theme::SPACING_TIGHT);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui
                    .button(tr!("Template file…"))
                    .on_hover_text(tr!(
                        "Read a template another project made and shared as a file"
                    ))
                    .clicked()
                {
                    if let Some(file) = file_dialogs::open_template_file() {
                        draft.error = draft.load_template(&file).err();
                    }
                }
            });
            if let Some(why) = &draft.error {
                ui.add(egui::Label::new(egui::RichText::new(why).color(theme::ERROR)).wrap());
            }
        });
        aside(ui, measures, |ui| {
            let heading = draft
                .shared()
                .map_or(draft.template.label(), |loaded| {
                    loaded.template.name.as_str()
                })
                .to_string();
            ui.label(egui::RichText::new(heading).strong().size(16.0));
            ui.add(egui::Label::new(draft.blurb()).wrap());
            ui.add_space(theme::SPACING_DEFAULT);
            egui::Grid::new("template_facts")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    fact(ui, tr!("Content"), &draft.content());
                    fact(ui, tr!("Builds"), &draft.builds());
                    fact(
                        ui,
                        tr!("Opens on"),
                        &tr!(
                            "The {opens_on} workspace",
                            opens_on = draft.opens_on().label()
                        ),
                    );
                });
            if let Some(loaded) = draft.shared() {
                for finding in &loaded.findings {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(finding).small().color(theme::WARNING),
                        )
                        .wrap(),
                    );
                }
            }
        });
    });
}

/// A template's row: its name as a button and the first sentence of what it is under it, so a name is not all there is to choose by; whether either was clicked, and whether twice.
fn row(
    ui: &mut egui::Ui,
    measures: Measures,
    chosen: bool,
    name: &str,
    blurb: &str,
) -> (bool, bool) {
    let first = blurb
        .split_once(". ")
        .map_or(blurb, |(sentence, _)| &blurb[..=sentence.len()]);
    let button = ui.add(
        egui::Button::selectable(chosen, egui::RichText::new(name).strong())
            .min_size(egui::vec2(measures.left, 28.0)),
    );
    let said = ui.add(
        egui::Label::new(
            egui::RichText::new(first)
                .small()
                .color(theme::TEXT_SECONDARY),
        )
        .wrap()
        .sense(egui::Sense::click()),
    );
    ui.add_space(theme::SPACING_TIGHT);
    (
        button.clicked() || said.clicked(),
        button.double_clicked() || said.double_clicked(),
    )
}

fn fact(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.label(egui::RichText::new(name).color(theme::TEXT_SECONDARY));
    ui.add(egui::Label::new(value).wrap());
    ui.end_row();
}

/// The refusal about `field`, said under it: a hint while the field is still empty and a warning once it is wrong.
fn say(ui: &mut egui::Ui, refusal: &Option<Refusal>, field: Field, empty: bool) {
    if let Some(refusal) = refusal.as_ref().filter(|refusal| refusal.field == field) {
        let colour = if empty {
            theme::TEXT_SECONDARY
        } else {
            theme::WARNING
        };
        ui.add(egui::Label::new(egui::RichText::new(&refusal.why).color(colour)).wrap());
    }
}

fn details(ui: &mut egui::Ui, measures: Measures, draft: &mut NewProject, previews: &mut Previews) {
    let adopt = draft.template == Template::Adopt;
    let clothing = draft.template == Template::Clothing;
    let (refusal, tree) = draft.looked_at();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(measures.left);
            ui.label(
                egui::RichText::new(draft.template.label())
                    .strong()
                    .size(16.0),
            );
            ui.add_space(theme::SPACING_DEFAULT);

            ui.label(tr!("Name"));
            let mut name = draft.name.clone();
            let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            let field = named(
                field,
                if clothing {
                    tr!("Clothing pack name")
                } else {
                    tr!("Project name")
                },
            );
            // ⚠ Asked for each frame until the field has it: the click that opened the dialog gives the keyboard to its own button in the same frame, after this asked.
            if draft.focus {
                if field.has_focus() {
                    draft.focus = false;
                    // The name the dialog gave is selected, so that what is typed replaces it.
                    if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(name.chars().count()),
                        );
                        edit.cursor.set_char_range(Some(all));
                        edit.store(ui.ctx(), field.id);
                    }
                } else {
                    field.request_focus();
                }
            }
            if field.changed() {
                draft.set_name(name);
            }
            draft.enter_confirms |=
                field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            say(ui, &refusal, Field::Name, draft.name.is_empty());

            ui.add_space(theme::SPACING_TIGHT);
            ui.label(if adopt {
                tr!("Folder to make a project of")
            } else {
                tr!("Create in")
            });
            // ⚠ Browse… in its own slot at the right edge and the field over all that is left of the row, as wide as the field above it.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui.button(tr!("Browse…")).clicked() {
                    if let Some(folder) = file_dialogs::pick_open_folder() {
                        draft.pick_place(&folder);
                    }
                }
                let field = ui
                    .add(egui::TextEdit::singleline(&mut draft.place).desired_width(f32::INFINITY));
                named(field, tr!("Folder"));
            });
            // The end of a long path is the folder, and a field shows the start of it.
            if adopt {
                if let Some(leaf) = std::path::Path::new(draft.place.trim()).file_name() {
                    ui.label(
                        egui::RichText::new(tr!("The folder is called {name}", name = leaf.to_string_lossy()))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                    );
                }
            }
            say(ui, &refusal, Field::Place, draft.place.trim().is_empty());

            ui.add_space(theme::SPACING_TIGHT);
            ui.label(tr!("Game"));
            ui.horizontal(|ui| {
                for game in GameChoice::ALL {
                    ui.radio_value(&mut draft.game, game, game.label());
                }
            });
            // ⚠ A clothing pack has no builds in the words of the person who makes one.
            let for_whom = if clothing {
                tr!("The edition the pack is made for.")
            } else {
                tr!("The edition the builds are made for.")
            };
            ui.label(
                egui::RichText::new(for_whom)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );

            if draft.has_a_build() {
                ui.add_space(theme::SPACING_TIGHT);
                ui.label(tr!("FiveM resource name"));
                let mut resource = draft.resource.clone();
                let field =
                    ui.add(egui::TextEdit::singleline(&mut resource).desired_width(f32::INFINITY));
                if named(field, tr!("Resource name")).changed() {
                    draft.set_resource(resource);
                }
                ui.label(
                    egui::RichText::new(
                        tr!("The folder name of the resource on your server. Filled in from the name; change it if the folder there differs."),
                    )
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
                say(ui, &refusal, Field::Resource, false);
            }
            say(ui, &refusal, Field::Template, false);
            if let Some(why) = &draft.error {
                ui.add_space(theme::SPACING_TIGHT);
                ui.add(egui::Label::new(egui::RichText::new(why).color(theme::ERROR)).wrap());
            }
        });
        aside(ui, measures, |ui| {
            // ⚠ One heading for every template: it says what is written, and the button says what is done.
            ui.label(egui::RichText::new(tr!("What this writes")).strong());
            // ⚠ The sentence that makes Adopt safe is the first thing in the pane, and not the last of a scroll area.
            if adopt {
                ui.label(tr!("Nothing is moved or rewritten."));
            }
            ui.add_space(theme::SPACING_TIGHT);
            for line in &tree {
                let marker = if line.new { "+" } else { " " };
                let slash = if line.is_folder { "/" } else { "" };
                let text = format!("{marker} {}{}{slash}", "  ".repeat(line.depth), line.name);
                let text = egui::RichText::new(text).monospace();
                ui.label(if line.stands_in {
                    text.italics().color(theme::TEXT_SECONDARY)
                } else {
                    text
                });
            }
            if adopt {
                // ⚠ What the adoption adds beside the folder's own files is part of what Create writes.
                let folder = std::path::PathBuf::from(&draft.place);
                let adds = previews
                    .of(&folder)
                    .map(|preview| preview.adds.clone())
                    .unwrap_or_default();
                for added in adds {
                    ui.label(egui::RichText::new(format!("+   {added}")).monospace());
                }
                ui.add_space(theme::SPACING_DEFAULT);
                adoption_preview(ui, draft, previews);
            }
            if clothing {
                ui.add_space(theme::SPACING_DEFAULT);
                ui.add(
                    egui::Label::new(
                        tr!("An empty clothing pack, and a FiveM resource for it. The Wardrobe opens, \
                         and you drop in the models and textures."),
                    )
                    .wrap(),
                );
            }
        });
    });
}

/// What the folder to adopt holds, as the adoption would group it: what is recognised, what stays Files, and what could not be placed.
///
/// ⚠ In the type size of the form beside it: a preview in small print reads as a footnote and not as what Create will do.
fn adoption_preview(ui: &mut egui::Ui, draft: &NewProject, previews: &mut Previews) {
    let plain = |text: &str| egui::Label::new(text).wrap();
    let folder = std::path::PathBuf::from(&draft.place);
    if draft.place.trim().is_empty() || !folder.is_dir() {
        // Nothing to read yet; the promise below is all there is to say.
    } else if let Some(preview) = previews.of(&folder) {
        ui.label(egui::RichText::new(tr!("The folder holds")).strong());
        for found in &preview.found {
            ui.add(plain(found));
        }
        ui.add(plain(&tr!(
            "Files: {files}",
            files = counted(preview.files, "file")
        )));
        for note in &preview.notes {
            ui.add(egui::Label::new(egui::RichText::new(note).color(theme::WARNING)).wrap());
        }
        if !preview.unplaced.is_empty() {
            ui.add_space(theme::SPACING_TIGHT);
            unplaced(ui, &preview.unplaced);
        }
    } else {
        ui.add(
            egui::Label::new(
                egui::RichText::new(tr!("Reading the folder…")).color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    }
    ui.add_space(theme::SPACING_TIGHT);
    ui.add(
        egui::Label::new(
            egui::RichText::new(tr!(
                "The project file is added beside what is there, with a .gitignore and a \
                 .gitattributes where the folder has neither, and a wardrobe.json beside a \
                 clothing pack. A clothing pack becomes clothing content and every other file \
                 stays Files content."
            ))
            .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
}

/// The files the adoption does not make content: each reason once with how many files it holds, and their names folded away.
///
/// ⚠ Grouped by reason, since one fact said for every file is a wall of text that pushes the rest of the pane off the screen.
fn unplaced(ui: &mut egui::Ui, left: &[or_project::adopt::Unplaced]) {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for each in left {
        match groups.iter_mut().find(|(why, _)| *why == each.why) {
            Some((_, paths)) => paths.push(&each.path),
            None => groups.push((&each.why, vec![&each.path])),
        }
    }
    // What stays a plain file, then what is not carried because the build writes it again.
    for (heading, carried) in [
        (tr!("Kept as plain files"), false),
        (tr!("Not carried over"), true),
    ] {
        let of_this_kind: Vec<&(&str, Vec<&str>)> = (groups.iter())
            .filter(|(why, _)| why.contains("not carried") == carried)
            .collect();
        if of_this_kind.is_empty() {
            continue;
        }
        let total: usize = of_this_kind.iter().map(|(_, paths)| paths.len()).sum();
        ui.label(egui::RichText::new(format!("{heading} ({total})")).strong());
        for (why, paths) in of_this_kind {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{}: {why}", counted(paths.len(), "file")))
                        .color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
            egui::CollapsingHeader::new(tr!("Show the files"))
                .id_salt(("adoption_files", *why))
                .show(ui, |ui| {
                    for path in paths.iter().take(UNPLACED_SHOWN) {
                        ui.add(egui::Label::new(egui::RichText::new(*path).small()).wrap());
                    }
                    if paths.len() > UNPLACED_SHOWN {
                        ui.label(tr!(
                            "and {count} more, listed in the project's Properties",
                            count = paths.len() - UNPLACED_SHOWN
                        ));
                    }
                });
        }
    }
}

/// The question a project opened while another is open asks: here, in a new window, or not at all.
fn switching(ctx: &egui::Context, state: &mut AppState) {
    let Some(path) = state.front_door.switching.clone() else {
        return;
    };
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let open = state.project_name().unwrap_or_default();
    let mut answer: Option<Switch> = None;
    question_titled(
        ctx,
        tr!("Open project"),
        &tr!("Open {name}?", name = name),
        480.0,
        |ui| {
            ui.label(tr!("{open} is open in this window.", open = open));
            // ⚠ Where the dropped project is: two projects can have names a letter apart.
            let place = elide_middle(&path.display().to_string(), 72);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(place)
                        .small()
                        .color(theme::TEXT_SECONDARY),
                )
                .truncate(),
            )
            .on_hover_text(path.display().to_string());
            ui.add_space(theme::SPACING_LOOSE);
            // ⚠ The button that keeps both is the one that goes on; the one that replaces says so, and the question about unsaved work comes after it, as Close project asks it.
            ui.horizontal(|ui| {
                let go = egui::Button::new(
                    egui::RichText::new(tr!("Open in new window")).color(theme::ON_ACCENT.get()),
                )
                .fill(theme::ACCENT.get());
                if ui.add(go).clicked() {
                    answer = Some(Switch::NewWindow);
                }
                if ui.button(tr!("Replace {open}", open = open)).clicked() {
                    answer = Some(Switch::Here);
                }
                if ui.button(tr!("Cancel")).clicked() {
                    answer = Some(Switch::Cancel);
                }
            });
            ui.add_space(theme::SPACING_TIGHT);
            // What each button does, under the row: Replace is the one that looks as if it deletes.
            for said in [
                tr!(
                    "Open in new window: {open} stays here and {name} opens in a second window.",
                    open = open,
                    name = name
                ),
                tr!(
                    "Replace {open}: {open} closes in this window and {name} opens in its place. \
                 It asks first if there is unsaved work.",
                    open = open,
                    name = name
                ),
            ] {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(said)
                            .small()
                            .color(theme::TEXT_SECONDARY),
                    )
                    .wrap(),
                );
            }
        },
    );
    if let Some(answer) =
        answer.or_else(|| question_escaped(ctx, tr!("Open project")).then_some(Switch::Cancel))
    {
        state.answer_switch(answer);
    }
}
