//! The report of an import: what became of each file, with Show in Assets and Undo import.

use egui_extras::{Column, TableBuilder};
use or_source::game_import::{FileReport, Verdict};

use crate::state::workbench::{counts, ImportReport};
use crate::state::AppState;
use crate::ui::theme::{self, Themed};

const ROW_HEIGHT: f32 = 20.0;
/// How many notes and files left out the window lists; the rest are counted.
const MAX_LISTED: usize = 8;

/// What a verdict is called, and in what colour.
fn words(verdict: &Verdict) -> (&'static str, Themed) {
    match verdict {
        Verdict::Made { .. } => (tr!("Imported"), theme::SUCCESS),
        Verdict::AlreadyASource { .. } => (tr!("Already a source"), theme::INFO),
        Verdict::Kept { .. } => (tr!("Kept as it is"), theme::WARNING),
        Verdict::Refused => (tr!("Refused"), theme::ERROR),
    }
}

/// What happened to `file`, as a sentence a person can act on: the library's words with its jargon put plain, the original kept for the hover.
///
/// ⚠ The words looked for are the library's own English, which it says whatever language is in use: they are not texts to translate.
fn plain(file: &FileReport) -> String {
    let reason = file.reason.as_str();
    match file.verdict {
        // `not a texture dictionary this build reads: Invalid RSC7 magic ...` names a header word nobody can act on.
        Verdict::Refused => match reason.split_once(" this build reads") {
            Some((head, _)) => {
                tr!(
                    "{head} OpenRage can read: its header is wrong or the file is damaged",
                    head = head
                )
            }
            None => reason.to_string(),
        },
        Verdict::Kept { .. } => reason
            .replace(
                " is a compiled file this import has no source form for yet, so",
                " has no editable form yet, so",
            )
            .replace(
                " is a compiled file this import has no source form for, so",
                " has no editable form, so",
            )
            .replace(
                " has no source form this import could make (",
                " could not be made into a source (",
            ),
        // `... in the project: the same file and SHA-256` says how it was told, which no one needs to read.
        Verdict::AlreadyASource { .. } => reason
            .split_once(": the same ")
            .map_or(reason, |(head, _)| head)
            .to_string(),
        Verdict::Made { .. } => reason.to_string(),
    }
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if !state.workbench.imports.shown {
        return;
    }
    let Some(shown) = state.workbench.imports.report().cloned() else {
        state.workbench.imports.shown = false;
        return;
    };
    let mut open = true;
    let (mut show, mut undo) = (false, false);
    egui::Window::new(tr!("Import report"))
        .id(egui::Id::new("import_report"))
        .open(&mut open)
        .collapsible(false)
        // ⚠ At the foot of the window, so the table the import filled stays in view above it.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -34.0))
        .default_size([760.0, 340.0])
        .show(ctx, |ui| body(ui, &shown, &mut show, &mut undo));
    if show {
        state.show_import_in_assets();
    }
    if undo {
        state.undo_import();
    }
    let escaped =
        ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input();
    if !open || escaped {
        state.workbench.imports.shown = false;
    }
}

fn body(ui: &mut egui::Ui, shown: &ImportReport, show: &mut bool, undo: &mut bool) {
    let report = &shown.report;
    let [made, already, kept, _] = counts(report);
    ui.label(crate::state::workbench::said(report));
    if report.cancelled {
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    tr!("Stopped before the last file: what it made stays, and the files it did not reach are listed below. Import them again to finish."),
                )
                .color(theme::WARNING),
            )
            .wrap(),
        );
    }
    // ⚠ Said only when it cut something: a bound that left nothing out is a number nobody needs.
    if let (Some(bounds), true) = (
        report.bounds,
        report.cancelled || !report.left_out.is_empty(),
    ) {
        ui.label(
            egui::RichText::new(tr!(
                "A folder is read up to {depth} levels down and {files} files at once.",
                depth = bounds.depth,
                files = super::log::grouped(bounds.files as u64)
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
    }
    match &shown.undone {
        Some(Ok(said)) => {
            ui.label(egui::RichText::new(said).color(theme::SUCCESS));
        }
        Some(Err(why)) => {
            ui.add(egui::Label::new(egui::RichText::new(why).color(theme::ERROR)).wrap());
        }
        None => {}
    }
    ui.add_space(theme::SPACING_TIGHT);

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .max_scroll_height(220.0)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(90.0).at_least(60.0))
        // ⚠ The file keeps the room a name needs, and the sentence beside it is what is clipped: the end of a name is what tells the files of a family apart.
        .column(Column::initial(260.0).at_least(160.0).clip(true))
        .column(Column::remainder().at_least(120.0).clip(true))
        .header(ROW_HEIGHT, |mut header| {
            for title in [tr!("Result"), tr!("File"), tr!("What happened")] {
                header.col(|ui| {
                    ui.label(egui::RichText::new(title).color(theme::TEXT_SECONDARY));
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, report.files.len(), |mut row| {
                let Some(file) = report.files.get(row.index()) else {
                    return;
                };
                let (word, colour) = words(&file.verdict);
                row.col(|ui| {
                    ui.label(egui::RichText::new(word).color(colour));
                });
                row.col(|ui| {
                    let name = file.input.rsplit(['/', '\\']).next().unwrap_or(&file.input);
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let shown =
                        super::project::elided_in_the_middle(ui, name, &font, ui.available_width());
                    ui.add(egui::Label::new(shown).wrap_mode(egui::TextWrapMode::Extend))
                        .on_hover_text(&file.input);
                });
                row.col(|ui| {
                    // The sentence a person reads, with what the library said, hex and all, in the hover.
                    ui.label(plain(file)).on_hover_text(&file.reason);
                });
            });
        });

    // ⚠ The table says each file kept as it is, so the list of the kinds it kept is not said again.
    let notes =
        (report.notes.iter()).filter(|note| !note.starts_with(or_source::game_import::KEPT_NOTE));
    for note in notes.take(MAX_LISTED) {
        ui.add(egui::Label::new(egui::RichText::new(note).small().color(theme::WARNING)).wrap());
    }
    for left in report.left_out.iter().take(MAX_LISTED) {
        ui.add(
            egui::Label::new(
                egui::RichText::new(tr!(
                    "Left out: {path}: {why}",
                    path = left.path,
                    why = left.why
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    }
    let more = report.left_out.len().saturating_sub(MAX_LISTED);
    if more > 0 {
        ui.label(
            egui::RichText::new(tr!("and {more} more left out", more = more))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }

    ui.add_space(theme::SPACING_DEFAULT);
    ui.horizontal(|ui| {
        let anything = made + already + kept > 0;
        if ui
            .add_enabled(anything, egui::Button::new(tr!("Show in Assets")))
            .on_hover_text(tr!(
                "Select the first source this import made, in the Project workspace"
            ))
            .on_disabled_hover_text(tr!("This import made and kept nothing"))
            .clicked()
        {
            *show = true;
        }
        let undone = matches!(shown.undone, Some(Ok(_)));
        let nothing = report.created.is_empty();
        if ui
            .add_enabled(!undone && !nothing, egui::Button::new(tr!("Undo import")))
            .on_hover_text(tr!(
                "Remove exactly the folders and files this import created. Nothing is removed if \
                 any of them was changed since"
            ))
            .on_disabled_hover_text(if undone {
                tr!("This import was taken back")
            } else {
                tr!("This import created nothing")
            })
            .clicked()
        {
            *undo = true;
        }
    });
}

#[cfg(test)]
mod tests {
    use or_source::game_import::{Bounds, LeftOut, Report};

    use super::*;

    /// Every text the report's body paints.
    fn painted(report: Report) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let shown = ImportReport {
            report,
            undone: None,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let output = egui::Context::default().run_ui(input, |ui| {
            let (mut show, mut undo) = (false, false);
            body(ui, &shown, &mut show, &mut undo);
        });
        let mut out = Vec::new();
        (output.shapes.iter()).for_each(|c| walk(&c.shape, &mut out));
        out
    }

    #[test]
    fn a_file_already_a_source_is_not_said_with_the_hash_that_told_it() {
        let file = FileReport {
            input: "road.ytd".into(),
            verdict: Verdict::AlreadyASource {
                folder: "sources/road.textures.src".into(),
            },
            reason:
                "is already sources/road.textures.src in the project: the same file and SHA-256"
                    .into(),
        };
        assert_eq!(
            plain(&file),
            "is already sources/road.textures.src in the project"
        );
    }

    /// ⚠ The library says its reason in English whatever language is in use, so the words `plain` looks for are the library's.
    #[test]
    fn a_reason_is_put_plain_in_french_as_it_is_in_english() {
        let refused = FileReport {
            input: "road.ytd".into(),
            verdict: Verdict::Refused,
            reason: "not a texture dictionary this build reads: Invalid RSC7 magic: expected 0x37435352, found 0xC4C4C4C4".into(),
        };
        let already = FileReport {
            input: "road.ytd".into(),
            verdict: Verdict::AlreadyASource {
                folder: "sources/road.textures.src".into(),
            },
            reason:
                "is already sources/road.textures.src in the project: the same file and SHA-256"
                    .into(),
        };
        for code in ["en", "fr"] {
            let _language = crate::i18n::pin(code);
            let said = plain(&refused);
            assert!(
                !said.contains("RSC7")
                    && !said.contains("0x")
                    && !said.contains("this build reads"),
                "{code}: the header jargon is left in: {said}"
            );
            assert!(
                said.starts_with("not a texture dictionary"),
                "{code}: the library's head is kept: {said}"
            );
            assert_eq!(
                plain(&already),
                "is already sources/road.textures.src in the project",
                "{code}: the hash is left in"
            );
        }
        let _french = crate::i18n::pin("fr");
        assert!(
            plain(&refused).contains("son en-tête est faux"),
            "French says the sentence in French: {}",
            plain(&refused)
        );
    }

    #[test]
    fn a_long_name_keeps_its_end_in_the_file_column_and_the_first_result_is_called_imported() {
        let name = "mp_f_freemode_01_mp_f_uniforms^jbib_000_u_with_a_long_tail.ydd";
        let shown = painted(Report {
            files: vec![
                FileReport {
                    input: format!("C:/game/{name}"),
                    verdict: Verdict::Refused,
                    reason: "not a .ydd this build reads: Invalid RSC7 magic".into(),
                },
                FileReport {
                    input: "ok.ydd".into(),
                    verdict: Verdict::Made {
                        folder: "sources/ok.drawables.src".into(),
                    },
                    reason: "became sources/ok.drawables.src".into(),
                },
            ],
            ..Report::default()
        });
        // ⚠ The end of a name is what tells the files of a family apart, and for a refused file no other cell repeats the name.
        assert!(
            shown
                .iter()
                .any(|t| t.contains('…') && t.ends_with("with_a_long_tail.ydd")),
            "the end of the name is cut off: {shown:?}"
        );
        assert!(
            shown.iter().any(|t| t == "Imported") && !shown.iter().any(|t| t == "Made"),
            "{shown:?}"
        );
    }

    #[test]
    fn a_walked_folder_names_its_bounds_and_a_stopped_import_says_it_stopped_and_what_it_did_not_reach(
    ) {
        let bounds = Bounds {
            depth: 3,
            files: 25,
        };
        let walked = painted(Report {
            bounds: Some(bounds),
            left_out: vec![LeftOut {
                path: "props/deep/lamp.ytd".into(),
                why: "is below the deepest folder read".into(),
            }],
            ..Report::default()
        });
        let line = walked
            .iter()
            .find(|t| t.contains("levels down"))
            .unwrap_or_else(|| panic!("no line names the bounds: {walked:?}"));
        assert!(
            line.contains(&bounds.depth.to_string()) && line.contains(&bounds.files.to_string()),
            "{line}"
        );
        assert!(!walked.iter().any(|t| t.contains("Stopped")), "{walked:?}");
        // A bound that left nothing out is not said.
        let whole = painted(Report {
            bounds: Some(bounds),
            ..Report::default()
        });
        assert!(
            !whole.iter().any(|t| t.contains("levels down")),
            "{whole:?}"
        );

        let alone = painted(Report::default());
        assert!(
            !alone.iter().any(|t| t.contains("levels down")),
            "{alone:?}"
        );

        let stopped = painted(Report {
            cancelled: true,
            left_out: vec![LeftOut {
                path: "props/lamp.ytd".into(),
                why: "was not reached before the import was stopped".into(),
            }],
            ..Report::default()
        });
        assert!(
            stopped.iter().any(|t| t.contains("Stopped")),
            "the window says it stopped: {stopped:?}"
        );
        assert!(
            stopped.iter().any(|t| t.contains("props/lamp.ytd")),
            "and lists what it did not reach: {stopped:?}"
        );
    }
}
