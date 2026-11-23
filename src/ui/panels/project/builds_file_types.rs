//! The types a singleplayer DLC registers the files of the Files content as: the files with no type to pick from, the types stated, and the row that states one.
//!
//! ⚠ **Folded and closed.** A person who makes clothes has no file to type, and an open form at full height pushed the build being edited off the window. A reason that points here ([`ask_to_open`]) opens it.

use or_project::OutputKind;

use crate::state::AppState;
use crate::ui::theme;

/// What a locked cell's `Go to File types` leaves in the context for the next frame: open the fold and scroll to it.
const OPEN: &str = "builds_scroll_to_file_types";

/// How many files with no type are listed before the list scrolls.
const LIST_HEIGHT: f32 = 110.0;

/// Ask for the fold to open and scroll into view on the next frame it is drawn.
pub(super) fn ask_to_open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN), true));
}

pub(super) fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let container = &state.project_carried.container;
    let stated = state.own_files_content().map(|m| m.file_types());
    let wanted = container
        .outputs
        .iter()
        .any(|o| o.kind == OutputKind::SingleplayerDlc)
        || stated.as_ref().is_some_and(|types| !types.is_empty());
    let Some(stated) = stated.filter(|_| wanted) else {
        return;
    };
    let asked = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<bool>(egui::Id::new(OPEN)))
        .is_some();
    ui.add_space(theme::SPACING_SNUG);
    ui.separator();
    let mut fold = egui::CollapsingHeader::new(egui::RichText::new(tr!("File types")).strong())
        .id_salt("builds_file_types");
    if asked {
        fold = fold.open(Some(true));
    }
    let shown = fold.show(ui, |ui| rows(ui, state, stated));
    if asked {
        shown.header_response.scroll_to_me(Some(egui::Align::TOP));
    }
}

fn rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    stated: std::collections::BTreeMap<String, String>,
) {
    ui.label(
        egui::RichText::new(
            tr!("A singleplayer DLC tells the game a type for each file it registers. An extension settles one only for .ityp and .rpf, so every other file needs one here, such as HANDLING_FILE for data/handling.meta."),
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );
    let key = egui::Id::new("builds_file_type_draft");
    let (mut path, mut file_type): (String, String) =
        ui.data_mut(|d| d.get_temp(key)).unwrap_or_default();

    // ⚠ The files the lock is about, to pick from: the lock says how many have no type, and this says which.
    let untyped = match state.own_files_content() {
        Some(files) => state.workbench.untyped_files(&files.id).to_vec(),
        None => Vec::new(),
    };
    if !untyped.is_empty() {
        ui.label(
            egui::RichText::new(tr!("Files with no type ({count})", count = untyped.len()))
                .strong(),
        );
        egui::ScrollArea::vertical()
            .id_salt("builds_untyped_files")
            .max_height(LIST_HEIGHT)
            .show(ui, |ui| {
                for each in &untyped {
                    let pick = ui.small_button(each).on_hover_text(tr!(
                        "Put this file in the File field below, to give it a type"
                    ));
                    if pick.clicked() {
                        path.clone_from(each);
                    }
                }
            });
    }

    let mut remove: Option<String> = None;
    for (path, file_type) in &stated {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(path).monospace());
            ui.label(
                egui::RichText::new(file_type)
                    .monospace()
                    .color(theme::TEXT_SECONDARY),
            );
            let take = ui
                .small_button(tr!("Remove"))
                .on_hover_text(tr!("The file stays in the project and states no type"));
            take.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr!("Remove the type of {path}", path = path),
                )
            });
            if take.clicked() {
                remove = Some(path.clone());
            }
        });
    }
    if let Some(path) = remove {
        let _ = state.state_file_type(&path, None);
    }

    let mut stated_now = false;
    ui.horizontal(|ui| {
        ui.label(tr!("File"));
        let at = ui.add(
            egui::TextEdit::singleline(&mut path)
                .hint_text("data/handling.meta")
                .desired_width(190.0),
        );
        at.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Path of the file"))
        });
        ui.label(tr!("Type"));
        let ty = ui.add(
            egui::TextEdit::singleline(&mut file_type)
                .hint_text("HANDLING_FILE")
                .desired_width(150.0),
        );
        ty.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Type of the file"))
        });
        let ready = !path.trim().is_empty() && !file_type.trim().is_empty();
        let button = ui.add_enabled(ready, egui::Button::new(tr!("Set type")));
        button.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ready, tr!("Set type"))
        });
        stated_now = button.clicked();
    });
    if stated_now {
        match state.state_file_type(path.trim(), Some(file_type.trim())) {
            Ok(()) => (path, file_type) = (String::new(), String::new()),
            Err(why) => state.show_error_toast(why),
        }
    }
    ui.data_mut(|d| d.insert_temp(key, (path, file_type)));
}
