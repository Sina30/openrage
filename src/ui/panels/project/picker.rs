//! The picker: the one dialog every content uses to name what it needs and does not own, and the question a file of another place asks.
//!
//! This project's own contents come first, then the files opened lately, then a way to look elsewhere and a field for the name of a game file.
//! It takes the keyboard whole: type to search, the arrow keys move, Enter chooses and Escape closes.

use std::path::{Path, PathBuf};

use crate::platform::file_dialogs;
use crate::state::workbench::{Choice, Part, Picker, Question, Wanting};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_titled, question_within};
use crate::ui::theme;

/// How wide the picker is, or as wide as the window leaves it.
const WIDTH: f32 = 540.0;

/// The height the list gets, at the least and at the most; the window's height decides between them, so a window of 720 points shows eight rows and one of 600 shows what fits.
const LIST_HEIGHT: std::ops::RangeInclusive<f32> = 200.0..=340.0;

/// What the picker's other lines take of the window's height: the title, the search, the two rows under the list and the buttons.
const AROUND_THE_LIST: f32 = 320.0;

pub fn draw(ctx: &egui::Context, state: &mut AppState, mut picker: Picker) {
    // ⚠ Says what it acts on: the part of the garment's name that tells garments apart, and the whole name under it.
    let title = match picker.garment.rsplit('^').next() {
        Some(short) if !short.is_empty() => format!("{} of {short}", picker.wanting.title()),
        _ => picker.wanting.title(),
    };
    let mut answer: Option<Choice> = None;
    let mut closed = false;
    let part = picker.wanting.part();
    let moved = ctx.input_mut(|input| {
        let down = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
        let up = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
        isize::from(down) - isize::from(up)
    });
    if moved != 0 {
        picker.step(moved);
        // One answer at a time: the row walked to is the one chosen, and a name typed before is not.
        picker.id.clear();
    }
    let list_height = (ctx.content_rect().height() - AROUND_THE_LIST)
        .clamp(*LIST_HEIGHT.start(), *LIST_HEIGHT.end());
    question_within(ctx, &title, WIDTH, |ui| {
        ui.set_max_width(WIDTH);
        // ⚠ Said only where the title shortened a name of the file's own: a garment's name is in the title already.
        if picker.garment.contains('^') {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("For {garment}", garment = picker.garment))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
        }
        let search = ui.add(
            egui::TextEdit::singleline(&mut picker.query)
                .hint_text(tr!("Search this project and recent files"))
                .desired_width(f32::INFINITY),
        );
        search.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Search the picker"))
        });
        // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
        if picker.focus && !search.has_focus() {
            search.request_focus();
        }
        let rows = picker.rows();
        // ⚠ A row is chosen by the person's click or arrow key; what a search narrows to one row is shown chosen, so Enter's target is on screen.
        if search.changed() {
            picker.selected = (rows == 1).then_some(0);
        }
        picker.selected = picker.selected.filter(|&at| at < rows);
        // ⚠ A one-line field gives up the keyboard in the frame Enter is pressed in, so it has lost it by now. The Enter that ran the command which opened the picker (the palette's) is still in the frame the picker opens in, and chooses nothing.
        let enter = !picker.focus && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter && (search.has_focus() || search.lost_focus()) {
            answer = picker.answer();
            if answer.is_none() {
                search.request_focus();
            }
        }
        ui.add_space(theme::SPACING_TIGHT);
        list(ui, &mut picker, moved != 0, &mut answer, list_height);
        ui.separator();
        let browsed = browse_row(ui, &picker.wanting);
        if let Some(path) = browsed {
            answer = Some(Choice::Outside(path));
        }
        let typed = id_row(ui, &mut picker.id, &mut picker.selected);
        if let Some(id) = typed {
            answer = Some(Choice::Id(id));
        }
        ui.add_space(theme::SPACING_SNUG);
        ui.horizontal(|ui| {
            // ⚠ One button answers: the row chosen, or the name typed when no row is.
            let choice = match picker.id.trim() {
                "" => picker.answer(),
                name => Some(Choice::Id(name.to_string())),
            };
            if ui
                .add_enabled(choice.is_some(), egui::Button::new(tr!("Choose")))
                .on_hover_text(tr!(
                    "Use what is chosen in the list, or the name typed under it"
                ))
                .on_disabled_hover_text(tr!(
                    "Select a {noun} in the list, or type the name of a game file",
                    noun = part.noun()
                ))
                .clicked()
            {
                answer = choice;
            }
            if ui.button(tr!("Cancel")).clicked() {
                closed = true;
            }
        });
    });
    picker.focus = false;
    if question_escaped(ctx, &title) {
        closed = true;
    }
    match (answer, closed) {
        (Some(choice), _) => state.choose(picker.wanting, choice),
        (None, true) => state.dismiss_picker(),
        (None, false) => state.workbench.asking = Some(Question::Pick(Box::new(picker))),
    }
}

/// The list: the project's own contents, then the recent files.
fn list(
    ui: &mut egui::Ui,
    picker: &mut Picker,
    moved: bool,
    answer: &mut Option<Choice>,
    height: f32,
) {
    let part = picker.wanting.part();
    let selected = picker.selected;
    let (mine, recent) = picker.shown();
    let (mine, recent): (Vec<_>, Vec<_>) = (
        mine.into_iter().cloned().collect(),
        recent.into_iter().cloned().collect(),
    );
    let mut clicked: Option<usize> = None;
    let mut chosen: Option<usize> = None;
    egui::ScrollArea::vertical()
        .id_salt("picker_list")
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr!("In this project  ·  {count}", count = mine.len()))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            if mine.is_empty() {
                ui.label(
                    egui::RichText::new(none_here(part))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
            for (n, candidate) in mine.iter().enumerate() {
                let row = ui.selectable_label(
                    selected == Some(n),
                    two_lines(ui, &candidate.label, &candidate.detail),
                );
                row.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::SelectableLabel,
                        true,
                        format!("{}, {}", candidate.label, candidate.detail),
                    )
                });
                if selected == Some(n) && moved {
                    row.scroll_to_me(None);
                }
                if row.clicked() {
                    clicked = Some(n);
                }
                if row.double_clicked() {
                    chosen = Some(n);
                }
            }
            if !recent.is_empty() {
                ui.add_space(theme::SPACING_SNUG);
                ui.label(
                    egui::RichText::new(tr!("Recent files  ·  {count}", count = recent.len()))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
            for (n, path) in recent.iter().enumerate() {
                let at = mine.len() + n;
                let row = ui.selectable_label(selected == Some(at), shown_path(path));
                row.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::SelectableLabel,
                        true,
                        tr!("Recent file {path}", path = shown_path(path)),
                    )
                });
                if selected == Some(at) && moved {
                    row.scroll_to_me(None);
                }
                if row.clicked() {
                    clicked = Some(at);
                }
                if row.double_clicked() {
                    chosen = Some(at);
                }
            }
        });
    if let Some(n) = clicked {
        picker.selected = Some(n);
        picker.id.clear();
    }
    if let Some(n) = chosen {
        picker.selected = Some(n);
        picker.id.clear();
        *answer = picker.answer();
    }
}

/// A row's words: the name, and under it in small grey the folder it is in and what it is.
fn two_lines(ui: &egui::Ui, name: &str, detail: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let body = egui::TextStyle::Body.resolve(ui.style());
    job.append(
        name,
        0.0,
        egui::TextFormat::simple(body, ui.visuals().text_color()),
    );
    if !detail.is_empty() {
        let small = egui::TextStyle::Small.resolve(ui.style());
        job.append(
            &format!("\n{detail}"),
            0.0,
            egui::TextFormat::simple(small, theme::TEXT_SECONDARY.get()),
        );
    }
    job
}

/// What the list says when the project holds none of what is wanted.
fn none_here(part: Part) -> String {
    match part.source() {
        Some(_) => tr!("No {noun} source in this project yet: Browse… takes one from elsewhere, and a game file is turned into one", noun = part.noun()),
        None => tr!("No {noun} in this project yet: Browse… takes one from elsewhere, or type the name of a game file", noun = part.noun()),
    }
}

/// `name  ·  folder` for a file, so a long path breaks at the folder and not inside the name.
fn shown_path(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let folder = path
        .parent()
        .map_or_else(String::new, |p| p.display().to_string());
    format!("{name}   {folder}")
}

/// Look elsewhere: a file, and, for what has a source form, a source folder. The file is the person's to import or only to reference.
fn browse_row(ui: &mut egui::Ui, wanting: &Wanting) -> Option<PathBuf> {
    let part = wanting.part();
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(tr!("Somewhere else"));
        if ui
            .button(tr!("Browse…"))
            .on_hover_text(tr!(
                "A file of another project, or any file on this computer"
            ))
            .clicked()
        {
            picked = file_dialogs::ask(&wanting.title(), |dialog| {
                dialog
                    .add_filter(part.noun(), part.extensions())
                    .add_filter(tr!("All Files"), &["*"])
                    .pick_file()
            });
        }
        if part.source().is_some()
            && ui
                .button(tr!("Browse a source folder…"))
                .on_hover_text(tr!("A .src folder of another project, taken as it is"))
                .clicked()
        {
            picked = file_dialogs::pick_folder(&wanting.title());
        }
    });
    picked
}

/// The field for the name of a game file, for a file nobody here has; the name when Enter uses it.
///
/// ⚠ No button of its own: Choose answers with it, so the dialog has one answer and not two.
fn id_row(ui: &mut egui::Ui, name: &mut String, selected: &mut Option<usize>) -> Option<String> {
    let mut used = None;
    ui.horizontal(|ui| {
        ui.label(tr!("Or name a game file"));
        let field = ui.add(
            egui::TextEdit::singleline(name)
                .hint_text(tr!("a file the game already has, by its name"))
                .desired_width(300.0),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("The name of a game file typed by hand"),
            )
        });
        let field = field.on_hover_text(tr!(
            "The build names the file by this name and ships none. Enter chooses it"
        ));
        if field.changed() {
            *selected = None;
        }
        let press = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if press && !name.trim().is_empty() {
            used = Some(name.trim().to_string());
        }
    });
    used
}

/// A file of another place was chosen: import it into the project, or only reference it.
///
/// ⚠ The key of the window is the question of old and its title says what is decided: which file is used as what, and for which garment.
pub fn import_or_reference(
    ctx: &egui::Context,
    state: &mut AppState,
    wanting: &Wanting,
    path: &Path,
    whose: Option<&str>,
) {
    const KEY: &str = "Add this file to the project?";
    let part = wanting.part();
    let shown = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let garment = state.garment_wanted(wanting);
    let role = wanting.role();
    let title = tr!(
        "Use {shown} as {role} of {garment}?",
        shown = shown,
        role = role,
        garment = garment
    );
    let folder = path
        .parent()
        .map_or_else(String::new, |p| p.display().to_string());
    let mut answer: Option<Option<bool>> = None;
    question_titled(ctx, KEY, &title, 440.0, |ui| {
        ui.set_max_width(520.0);
        let from = match whose {
            Some(project) => tr!(
                "It is a file of the project {project}, in {folder}.",
                project = project,
                folder = folder
            ),
            None => tr!(
                "It is not a file of this project. It is in {folder}.",
                folder = folder
            ),
        };
        ui.add(egui::Label::new(egui::RichText::new(from).color(theme::TEXT_SECONDARY)).wrap());
        // What it does to the garment, said once for both answers.
        let effect = match wanting {
            Wanting::NewTexture { .. } => tr!(
                "It is added to {garment}. Undo takes it off.",
                garment = garment
            ),
            Wanting::Cloth { .. } => {
                tr!(
                    "It becomes the cloth of {garment}. Undo brings {garment} back as it was.",
                    garment = garment
                )
            }
            Wanting::Locate { .. } => {
                tr!("{garment} now leads to it instead of the file that is missing. Undo brings that back.", garment = garment)
            }
            _ => tr!(
                "It replaces the {noun} {garment} has now. Undo brings that back.",
                garment = garment,
                noun = part.noun()
            ),
        };
        ui.add(egui::Label::new(effect).wrap());
        ui.add_space(theme::SPACING_DEFAULT);
        // ⚠ Each answer has its consequence under it at reading size, in the words of its button. Import is the one Enter takes: it only adds and never touches the original.
        let said = match part.source() {
            Some(_) => tr!("Turns it into a source in this project that you can edit. The original file is not changed."),
            None => tr!("Keeps a copy of it in this project as it is, since a cloth has no editable form. The original file is not changed."),
        };
        let import = ui.add(
            egui::Button::new(
                egui::RichText::new(tr!("Import into the project")).color(theme::ON_ACCENT.get()),
            )
            .fill(theme::ACCENT.get()),
        );
        if ui.memory(|m| m.focused().is_none()) {
            import.request_focus();
        }
        if import.clicked() {
            answer = Some(Some(true));
        }
        ui.add(egui::Label::new(said).wrap());
        ui.add_space(theme::SPACING_SNUG);
        if ui.button(tr!("Only reference it")).clicked() {
            answer = Some(Some(false));
        }
        ui.add(
            egui::Label::new(
                tr!("Keeps the file where it is. The project does not carry it, so on another computer, or if the file moves, it is missing until you Locate it."),
            )
            .wrap(),
        );
        ui.add_space(theme::SPACING_LOOSE);
        if ui.button(tr!("Cancel")).clicked() {
            answer = Some(None);
        }
    });
    if question_escaped(ctx, KEY) {
        answer = Some(None);
    }
    match answer {
        Some(Some(import)) => state.answer_import_or_reference(import),
        Some(None) => state.dismiss_import_or_reference(),
        None => {}
    }
}
