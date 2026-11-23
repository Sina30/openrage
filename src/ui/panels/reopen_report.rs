//! The report of a reopened pack: what became of it, and the files that could not be placed with where each was kept.

use egui_extras::{Column, TableBuilder};

use crate::state::front_door::{counted, spoken};
use crate::state::workbench::ReopenReport;
use crate::state::AppState;
use crate::ui::theme;

const ROW_HEIGHT: f32 = 20.0;
/// How many notes the window lists.
const MAX_LISTED: usize = 6;

/// What a press in the window asks for.
#[derive(Default)]
struct Asked {
    open: bool,
    show: bool,
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if !state.workbench.reopen.shown {
        return;
    }
    let Some(shown) = state.workbench.reopen.report().cloned() else {
        state.workbench.reopen.shown = false;
        return;
    };
    let mut open = true;
    let mut asked = Asked::default();
    egui::Window::new(tr!("Pack imported"))
        .id(egui::Id::new("reopen_report"))
        .open(&mut open)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_size([640.0, 420.0])
        .show(ctx, |ui| body(ui, &shown, &mut asked));
    if asked.show {
        crate::platform::shell::reveal(&shown.into);
    }
    if asked.open {
        state.open_reopened_project();
    }
    let escaped =
        ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input();
    if !open || escaped {
        state.workbench.reopen.shown = false;
    }
}

/// How many problems the report lists before it points at the Problems list for the rest.
const MAX_BROKEN: usize = 8;

/// What the rules of the Wardrobe found wrong in the pack made: a line for each, the worst first, and a clean pack says so.
fn broken(ui: &mut egui::Ui, made: &or_wardrobe::reopen::Reopened) {
    if made.broken.is_empty() {
        ui.label(
            egui::RichText::new(tr!("The Wardrobe's rules found nothing wrong in it."))
                .color(theme::SUCCESS),
        );
        return;
    }
    let worst = made
        .broken
        .iter()
        .any(|b| b.severity == or_wardrobe::rules::Severity::Error);
    ui.label(
        egui::RichText::new(tr!(
            "{count} found in it by the Wardrobe's rules; the Problems list shows them too:",
            count = counted(made.broken.len(), "problem")
        ))
        .color(if worst { theme::ERROR } else { theme::WARNING }),
    );
    let mut listed: Vec<_> = made.broken.iter().collect();
    listed.sort_by_key(|b| std::cmp::Reverse(b.severity));
    for found in listed.iter().take(MAX_BROKEN) {
        let line = match &found.garment {
            Some(garment) => format!("{garment}: {}", found.message),
            None => found.message.clone(),
        };
        let colour = match found.severity {
            or_wardrobe::rules::Severity::Error => theme::ERROR,
            _ => theme::WARNING,
        };
        ui.add(egui::Label::new(egui::RichText::new(line).small().color(colour)).wrap());
    }
    if listed.len() > MAX_BROKEN {
        ui.label(
            egui::RichText::new(tr!("and {count} more", count = listed.len() - MAX_BROKEN))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
}

/// `text` with its first letter in capitals, as the engine's notes start in the middle of a sentence.
fn as_a_sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn body(ui: &mut egui::Ui, shown: &ReopenReport, asked: &mut Asked) {
    let made = &shown.made;
    let file = shown
        .built
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    ui.add(
        egui::Label::new(tr!(
            "{pack} is now a project of sources: {garments} and {textures}, read from {file}.",
            file = file,
            pack = made.pack,
            garments = counted(made.garments, "garment"),
            textures = counted(made.textures, "texture")
        ))
        .wrap(),
    );
    let folder = shown.into.display().to_string();
    ui.add(egui::Label::new(egui::RichText::new(&folder).small().monospace()).truncate())
        .on_hover_text(folder);
    ui.add(
        egui::Label::new(
            egui::RichText::new(
                tr!("The pack was not changed, and this made that folder and nothing else. There is no Undo: deleting the folder takes it back."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
    ui.add_space(theme::SPACING_TIGHT);

    if made.unplaced.is_empty() {
        ui.label(
            egui::RichText::new(tr!("Every file of the pack was placed.")).color(theme::SUCCESS),
        );
    } else {
        ui.label(
            egui::RichText::new(tr!(
                "{count} could not be placed where it would be used:",
                count = counted(made.unplaced.len(), "file")
            ))
            .color(theme::WARNING),
        );
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .max_scroll_height(220.0)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(200.0).at_least(80.0).clip(true))
            .column(Column::remainder().at_least(160.0).clip(true))
            .header(ROW_HEIGHT, |mut header| {
                for title in [tr!("File of the pack"), tr!("What became of it")] {
                    header.col(|ui| {
                        ui.label(egui::RichText::new(title).color(theme::TEXT_SECONDARY));
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, made.unplaced.len(), |mut row| {
                    let Some(left) = made.unplaced.get(row.index()) else {
                        return;
                    };
                    row.col(|ui| {
                        ui.label(&left.path).on_hover_text(&left.path);
                    });
                    row.col(|ui| {
                        ui.label(&left.why).on_hover_text(&left.why);
                    });
                });
            });
    }
    broken(ui, made);
    for note in made.notes.iter().take(MAX_LISTED) {
        let note = as_a_sentence(&spoken(note));
        ui.add(egui::Label::new(egui::RichText::new(note).small().color(theme::WARNING)).wrap());
    }
    ui.add_space(theme::SPACING_TIGHT);
    ui.add(
        egui::Label::new(
            egui::RichText::new(
                tr!("This reads files. A build of the project reads back what it wrote; nothing says the game loads it."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );

    ui.add_space(theme::SPACING_DEFAULT);
    ui.horizontal(|ui| {
        if ui
            .button(tr!("Open project"))
            .on_hover_text(tr!(
                "Open the new project, here or in a new window when one is open"
            ))
            .clicked()
        {
            asked.open = true;
        }
        if ui
            .button(tr!("Reveal in the file manager"))
            .on_hover_text(tr!("Show the new project folder in the file manager"))
            .clicked()
        {
            asked.show = true;
        }
    });
}

#[cfg(test)]
mod tests {
    use or_wardrobe::import::LeftOut;
    use or_wardrobe::reopen::Reopened;

    use super::*;

    fn painted(report: &ReopenReport) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let output = egui::Context::default().run_ui(input, |ui| {
            body(ui, report, &mut Asked::default());
        });
        let mut out = Vec::new();
        (output.shapes.iter()).for_each(|c| walk(&c.shape, &mut out));
        out
    }

    fn report(unplaced: Vec<LeftOut>) -> ReopenReport {
        ReopenReport {
            built: "dlc.rpf".into(),
            into: "projects/demo".into(),
            made: Reopened {
                project: "projects/demo/demo.openrage-proj".into(),
                pack: "demo".into(),
                garments: 4,
                textures: 6,
                unplaced,
                broken: Vec::new(),
                notes: vec![
                    "the 1 file(s) under files/ are in the project and no build takes them".into(),
                ],
            },
        }
    }

    #[test]
    fn what_could_not_be_placed_is_listed_with_where_it_was_kept_and_a_clean_pack_says_it_placed_all(
    ) {
        let kept = painted(&report(vec![LeftOut {
            path: "data/handling.meta".into(),
            why: "no kind of content here reads it; kept as files/data/handling.meta".into(),
        }]));
        assert!(
            kept.iter().any(|t| t.contains("data/handling.meta")
                && kept.iter().any(|u| u.contains("files/data/handling.meta"))),
            "{kept:?}"
        );
        assert!(
            kept.iter()
                .any(|t| t.contains("1 file could not be placed")),
            "{kept:?}"
        );
        assert!(!kept.iter().any(|t| t.contains("Every file")), "{kept:?}");
        assert!(
            kept.iter()
                .any(|t| t.starts_with("The 1 file under files/")),
            "a note the engine wrote is a sentence a person reads: {kept:?}"
        );

        let clean = painted(&report(Vec::new()));
        assert!(
            clean
                .iter()
                .any(|t| t.contains("Every file of the pack was placed")),
            "{clean:?}"
        );
        assert!(
            clean.iter().any(|t| t.contains("There is no Undo")),
            "the window says there is nothing to undo: {clean:?}"
        );
    }

    #[test]
    fn what_the_rules_found_broken_is_listed_with_its_garment_worst_first_and_a_clean_pack_says_so()
    {
        use or_wardrobe::reopen::Broken;
        use or_wardrobe::rules::Severity;
        let mut with = report(Vec::new());
        with.made.broken = vec![
            Broken {
                garment: Some("jacket.ydd".into()),
                severity: Severity::Warning,
                message: "jacket.ydd names a texture of 4096 by 4096".into(),
            },
            Broken {
                garment: Some("shirt.ydd".into()),
                severity: Severity::Error,
                message: "shirt.ydd owns cloth and no cloth file is linked".into(),
            },
            Broken {
                garment: None,
                severity: Severity::Warning,
                message: "the pack is called Mixed".into(),
            },
        ];
        let said = painted(&with);
        assert!(
            said.iter()
                .any(|t| t.contains("3 problems found in it by the Wardrobe's rules")),
            "{said:?}"
        );
        let at = |needle: &str| {
            said.iter()
                .position(|t| t.contains(needle))
                .unwrap_or(usize::MAX)
        };
        assert!(
            at("shirt.ydd: shirt.ydd owns cloth") < at("jacket.ydd: jacket.ydd names"),
            "an error first: {said:?}"
        );
        assert!(
            at("the pack is called Mixed") < usize::MAX,
            "a problem of the pack alone has no garment in front"
        );
        assert!(!said.iter().any(|t| t.contains("nothing wrong")));

        let clean = painted(&report(Vec::new()));
        assert!(
            clean
                .iter()
                .any(|t| t.contains("found nothing wrong in it")),
            "{clean:?}"
        );

        // Past the lines the report has room for, it counts the rest.
        let mut many = report(Vec::new());
        many.made.broken = (0..12)
            .map(|n| Broken {
                garment: None,
                severity: Severity::Warning,
                message: format!("problem {n}"),
            })
            .collect();
        assert!(painted(&many).iter().any(|t| t == "and 4 more"));
    }
}
