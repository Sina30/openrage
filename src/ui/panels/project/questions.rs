//! The questions the project asks before it acts: the Builds window, and whether to clean the builds.

use egui_phosphor::regular as icon;

use super::{builds_editor, rules_window};
use crate::state::front_door::counted;
use crate::state::workbench::{Concern, Question};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_titled, question_within};
use crate::ui::theme;

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    match state.workbench.asking.clone() {
        Some(Question::Builds { chosen, adding }) => {
            builds_editor::draw(ctx, state, chosen, adding);
        }
        Some(Question::Clean {
            resources,
            files,
            bytes,
        }) => clean(ctx, state, &resources, files, bytes),
        Some(Question::ReplaceGeometry {
            key,
            file,
            entries,
            chosen,
        }) => replace_geometry(ctx, state, key, file, entries, chosen),
        Some(Question::Geometry {
            name,
            from,
            replaced,
            lines,
        }) => geometry(ctx, state, &name, &from, replaced, &lines),
        Some(Question::Bulk {
            module,
            query,
            ids,
            draft,
            shown,
        }) => super::bulk_window::draw(ctx, state, module, query, ids, draft, shown),
        Some(Question::SaveQuery {
            module,
            query,
            name,
            error,
            focus,
        }) => super::save_query_window::draw(ctx, state, module, query, name, error, focus),
        Some(Question::Review {
            module,
            from,
            to,
            shown,
        }) => super::review_window::draw(ctx, state, module, from, to, shown),
        Some(Question::Credit {
            module,
            garment,
            draft,
            had,
        }) => super::credit_window::draw(ctx, state, module, garment, draft, had),
        Some(Question::Fix { module, finding }) => fix(ctx, state, module, finding),
        Some(Question::Waive {
            module,
            finding,
            reason,
            focus,
        }) => waive(ctx, state, module, finding, reason, focus),
        Some(Question::WaivePlugin {
            code,
            reason,
            focus,
        }) => waive_plugin(ctx, state, code, reason, focus),
        Some(Question::PluginFix {
            name,
            code,
            message,
            fix,
        }) => plugin_fix(ctx, state, &name, &code, &message, &fix),
        Some(Question::Rules { module, draft }) => rules_window::draw(ctx, state, module, draft),
        Some(Question::Optimise {
            module,
            garments,
            draft,
        }) => super::optimise_window::draw(ctx, state, module, garments, draft),
        Some(Question::Maps {
            module,
            garments,
            draft,
            lines,
        }) => super::maps_window::draw(ctx, state, module, garments, draft, lines),
        Some(Question::Budget { module, garments }) => {
            super::budget_window::draw(ctx, state, module, garments)
        }
        Some(Question::DropClothes { paths }) => dropped_clothes(ctx, state, paths.len()),
        Some(Question::DropContent { folder, kind, .. }) => {
            dropped_content(ctx, state, &folder, &kind)
        }
        Some(Question::Pick(picker)) => super::picker::draw(ctx, state, *picker),
        Some(Question::NewSource {
            kind,
            label,
            name,
            error,
            focus,
        }) => super::create::draw(ctx, state, kind, label, name, error, focus),
        Some(asked @ Question::Reopen { .. }) => super::reopen_question::draw(ctx, state, asked),
        Some(Question::ImportOrReference {
            wanting,
            path,
            whose,
        }) => super::picker::import_or_reference(ctx, state, &wanting, &path, whose.as_deref()),
        Some(Question::SharingRead { .. }) => reading_for_sharing(ctx, state),
        Some(Question::Sharing { concerns }) => sharing(ctx, state, &concerns),
        Some(Question::RollBack {
            step,
            label,
            sources,
        }) => roll_back(ctx, state, step, &label, &sources),
        None => {}
    }
}

/// Whether to go back to an earlier step of the History that takes the sources of an import with it.
fn roll_back(
    ctx: &egui::Context,
    state: &mut AppState,
    step: u64,
    label: &str,
    sources: &[String],
) {
    const TITLE: &str = tr_key!("Go back in the History");
    let mut answer: Option<bool> = None;
    // ⚠ What it deletes, counted: a source is a folder and a file an import kept is a file.
    let folders = (sources.iter())
        .filter(|name| or_source::asset::parse_folder_name(name).is_some())
        .count();
    let removes = [
        (folders > 0).then(|| counted(folders, tr!("source folder"))),
        (sources.len() > folders).then(|| counted(sources.len() - folders, "file")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" and ");
    question_within(ctx, crate::i18n::tr(TITLE), 520.0, |ui| {
        ui.add(
            egui::Label::new(tr!(
                "Going back to \"{label}\" undoes the imports after it.",
                label = label
            ))
            .wrap(),
        );
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!(
                "It removes {removes} from your project folder:",
                removes = removes
            ))
            .strong(),
        );
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .show(ui, |ui| {
                // ⚠ A name that is wider than the window is cut in the middle, where the family prefix is, and never broken inside; the hover has it whole.
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                for source in sources {
                    let shown =
                        super::elided_in_the_middle(ui, source, &font, ui.available_width());
                    let cut = shown != *source;
                    let line = ui.add(
                        egui::Label::new(egui::RichText::new(shown).monospace())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    if cut {
                        line.on_hover_text(source);
                    }
                }
            });
        ui.add_space(theme::SPACING_TIGHT);
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    tr!("They are deleted only while nobody has changed them since the import. The game files they came from are not touched. To get them back, import those files again."),
                )
                .small()
                .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if ui
                .button(
                    egui::RichText::new(tr!("Remove them and go back")).color(theme::ERROR.get()),
                )
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            if let Err(why) = state.roll_back_project_to(step) {
                state.show_warning_toast(why);
            }
        }
        Some(false) => state.workbench.asking = None,
        None => {}
    }
}

/// Check before sharing while the project's folders are read again, which a scan takes a moment to do.
fn reading_for_sharing(ctx: &egui::Context, state: &mut AppState) {
    let title = tr!("Check before sharing");
    let mut closed = false;
    question_within(ctx, title, 520.0, |ui| {
        ui.label(tr!("Reading the project's folders…"));
        ui.add_space(theme::SPACING_LOOSE);
        if ui.button(tr!("Cancel")).clicked() {
            closed = true;
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    if closed || question_escaped(ctx, title) {
        state.workbench.asking = None;
    }
}

/// Which drawable of a dictionary source the model goes into.
fn replace_geometry(
    ctx: &egui::Context,
    state: &mut AppState,
    key: String,
    file: std::path::PathBuf,
    entries: Vec<String>,
    mut chosen: usize,
) {
    let mut answer: Option<bool> = None;
    let shown = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    question_within(ctx, tr!("Replace geometry"), 420.0, |ui| {
        ui.label(tr!(
            "A dictionary holds many drawables. Which one does {shown} go into?",
            shown = shown
        ));
        ui.add_space(theme::SPACING_SNUG);
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .show(ui, |ui| {
                for (n, entry) in entries.iter().enumerate() {
                    ui.radio_value(&mut chosen, n, egui::RichText::new(entry).monospace());
                }
            });
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if ui.button(tr!("Replace geometry")).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, tr!("Replace geometry")) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            if let Some(entry) = entries.get(chosen).cloned() {
                state.replace_geometry(&key, Some(entry), file);
            }
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::ReplaceGeometry {
                key,
                file,
                entries,
                chosen,
            });
        }
    }
}

/// What Check before sharing found, one line each, and what it did not look at.
fn sharing(ctx: &egui::Context, state: &mut AppState, concerns: &[Concern]) {
    let title = tr!("Check before sharing");
    let mut closed = false;
    let mut show = false;
    question_within(ctx, title, 520.0, |ui| {
        ui.label(if concerns.is_empty() {
            tr!("Found nothing in the project file, its sources or its folder that is yours alone.")
        } else {
            tr!("Before you send this folder:")
        });
        ui.add_space(theme::SPACING_TIGHT);
        for concern in concerns {
            // A private folder is routine: Git leaves it out already. Only what would leak is drawn as a warning.
            let (text, colour) = match concern.kind {
                or_content::sharing::Kind::PrivateFolder => (
                    format!("{} {}", egui_phosphor::regular::FOLDER, concern.text),
                    theme::TEXT_PRIMARY,
                ),
                _ => (format!("• {}", concern.text), theme::WARNING),
            };
            ui.add(egui::Label::new(egui::RichText::new(text).color(colour)).wrap());
        }
        ui.add_space(theme::SPACING_SNUG);
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    tr!("This looks at the project file, the sources taken from the game and the \
                     .openrage folder, not the files you ship."),
                )
                .small()
                .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        )
        .on_hover_text(
            tr!("Nothing here says a build loads in the game. Git leaves out build/, vfs/ and the game's sources once the OpenRage block of .gitignore names them; a zip of the folder carries them"),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if !concerns.is_empty()
                && state.project_path.is_some()
                && ui
                    .button(tr!("Show the folder"))
                    .on_hover_text(tr!(
                        "Open the project folder in the file manager, to take these out"
                    ))
                    .clicked()
            {
                show = true;
            }
            if ui.button(tr!("Close")).clicked() {
                closed = true;
            }
        });
    });
    if show {
        if let Some(path) = state.project_path.as_deref() {
            crate::platform::shell::reveal(path);
        }
    }
    if closed || question_escaped(ctx, title) {
        state.workbench.asking = None;
    }
}

/// What the import of a model into a source did, with the lines `openrage-cli source import-model` prints for the same file.
fn geometry(
    ctx: &egui::Context,
    state: &mut AppState,
    name: &str,
    from: &str,
    replaced: bool,
    lines: &[String],
) {
    let title = if replaced {
        tr!("Geometry of {name} replaced", name = name)
    } else {
        tr!("{name} was not changed", name = name)
    };
    let mut closed = false;
    question_within(ctx, &title, 420.0, |ui| {
        if replaced {
            ui.label(tr!(
                "{from} is the model of {name} now, and the source is cooked again.",
                from = from,
                name = name
            ));
        } else {
            ui.label(tr!(
                "{from} was refused, and {name} is as it was:",
                from = from,
                name = name
            ));
        }
        ui.add_space(theme::SPACING_TIGHT);
        let colour = if replaced {
            theme::TEXT_SECONDARY
        } else {
            theme::ERROR
        };
        for line in lines {
            ui.add(egui::Label::new(egui::RichText::new(line).monospace().color(colour)).wrap());
        }
        ui.add_space(theme::SPACING_LOOSE);
        if ui.button(tr!("Close")).clicked() {
            closed = true;
        }
    });
    if closed || question_escaped(ctx, &title) {
        state.workbench.asking = None;
    }
}

fn clean(ctx: &egui::Context, state: &mut AppState, resources: &[String], files: u64, bytes: u64) {
    let mut answer: Option<bool> = None;
    question_within(ctx, tr!("Clean the builds"), 420.0, |ui| {
        if resources.is_empty() {
            ui.label(tr!(
                "OpenRage built nothing that is in build/ now, so there is nothing to remove."
            ));
        } else {
            ui.label(
                tr!("This removes what OpenRage built in build/, whichever build of this project wrote it, and the build each one replaced:"),
            );
        }
        for name in resources {
            ui.label(egui::RichText::new(tr!("build/{name}", name = name)).monospace());
        }
        if !resources.is_empty() {
            ui.add_space(theme::SPACING_TIGHT);
            ui.label(
                egui::RichText::new(tr!("{files} file(s), {size}, are deleted for good: they do not go to the recycle bin.", files = files, size = or_job::human_bytes(bytes)))
                .strong(),
            );
        }
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(
                tr!("The project's own files are never touched, and a folder OpenRage did not build is left alone. Build writes them again."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            // ⚠ Red, with the bin: the one button of the project that deletes folders. With nothing to remove there is none.
            if !resources.is_empty() {
                let clean = egui::Button::new(
                    egui::RichText::new(tr!("{icon} Clean", icon = egui_phosphor::regular::TRASH))
                        .color(theme::ERROR),
                );
                if ui.add(clean).clicked() {
                    answer = Some(true);
                }
            }
            let leave = if resources.is_empty() {
                tr!("Close")
            } else {
                tr!("Cancel")
            };
            if ui.button(leave).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, tr!("Clean the builds")) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.clean_builds();
        }
        Some(false) => state.workbench.asking = None,
        None => {}
    }
}

/// Models or textures dropped on a project that holds no clothing: make clothing for them, or import them like any game file.
fn dropped_clothes(ctx: &egui::Context, state: &mut AppState, files: usize) {
    const TITLE: &str = tr_key!("Clothes dropped");
    // `Some(true)` makes clothing, `Some(false)` imports as sources, and `None` is still asking.
    let mut answer: Option<Option<bool>> = None;
    question_within(ctx, crate::i18n::tr(TITLE), 440.0, |ui| {
        ui.label(tr!(
            "{files} dropped could be clothes, and this project holds no clothing yet.",
            files = crate::state::front_door::counted(files, "file")
        ));
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(
                tr!("Make clothing gives the project a clothing pack and the Wardrobe, and the dropped files are copied in as its garments. \
                 Import as sources decompiles each file into a source of the project instead, as any game file is."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            let make = egui::Button::new(
                egui::RichText::new(tr!("Make clothing")).color(theme::ON_ACCENT.get()),
            )
            .fill(theme::ACCENT.get());
            if ui.add(make).clicked() {
                answer = Some(Some(true));
            }
            if ui.button(tr!("Import as sources")).clicked() {
                answer = Some(Some(false));
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(None);
            }
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(None);
    }
    match answer {
        Some(Some(make)) => state.answer_dropped_clothes(make),
        Some(None) => state.dismiss_dropped_clothes(),
        None => {}
    }
}

/// A folder dropped on a project that holds content: link it as that content, or import what was dropped as sources.
fn dropped_content(
    ctx: &egui::Context,
    state: &mut AppState,
    folder: &std::path::Path,
    kind: &str,
) {
    const TITLE: &str = tr_key!("Folder dropped");
    // `Some(true)` links, `Some(false)` imports as sources, and `None` is still asking.
    let mut answer: Option<Option<bool>> = None;
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let link = tr!("Link as {kind}", kind = kind);
    question_within(ctx, crate::i18n::tr(TITLE), 440.0, |ui| {
        ui.label(tr!(
            "{name} holds {kind} content.",
            name = name,
            kind = kind.to_lowercase()
        ));
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(
                tr!("Link keeps the folder where it is and shows it in this project as read-only content.                  Import as sources copies what is in it into the project as sources instead, as any game file is."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            let button =
                egui::Button::new(egui::RichText::new(&link).color(theme::ON_ACCENT.get()))
                    .fill(theme::ACCENT.get());
            if ui.add(button).clicked() {
                answer = Some(Some(true));
            }
            if ui.button(tr!("Import as sources")).clicked() {
                answer = Some(Some(false));
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(None);
            }
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(None);
    }
    match answer {
        Some(Some(link)) => state.answer_dropped_content(link),
        Some(None) => state.dismiss_dropped_content(),
        None => {}
    }
}

/// What a fix of a finding of a clothing content will change, said before it is done.
fn fix(ctx: &egui::Context, state: &mut AppState, module: String, finding: or_wardrobe::Finding) {
    let remedy = (state.workbench.packs().iter())
        .find(|p| p.module == module)
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| or_wardrobe::edit::remedy(&read.wardrobe, &finding));
    let mut answer: Option<bool> = None;
    // ⚠ The title and the button say what the fix does, as the waiver's say what it waives; the key is `Fix` whatever they say, so that Escape and the window keep one identity.
    let act = remedy
        .as_ref()
        .map_or(tr!("Fix"), |remedy| remedy.act.as_str());
    let (mark, colour) = match finding.severity {
        or_wardrobe::Severity::Error => (icon::X_CIRCLE, theme::ERROR),
        or_wardrobe::Severity::Warning => (icon::WARNING, theme::WARNING),
        or_wardrobe::Severity::Info => (icon::INFO, theme::TEXT_SECONDARY),
    };
    question_titled(ctx, tr!("Fix"), &format!("{act}?"), 440.0, |ui| {
        // The problem is what is being decided, so it is the plainest text here.
        ui.horizontal_top(|ui| {
            ui.label(egui::RichText::new(mark).color(colour));
            ui.add(egui::Label::new(finding.message.clone()).wrap());
        });
        ui.add_space(theme::SPACING_SNUG);
        match &remedy {
            Some(remedy) => {
                ui.label(tr!("This changes exactly:"));
                ui.add(egui::Label::new(egui::RichText::new(&remedy.what).strong()).wrap());
                ui.label(
                    egui::RichText::new(tr!("Undo takes it back.")).color(theme::TEXT_SECONDARY),
                );
            }
            None => {
                ui.label(tr!("There is nothing to fix there any more."));
            }
        }
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(remedy.is_some(), egui::Button::new(act))
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, tr!("Fix")) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.fix_finding(module, finding);
        }
        Some(false) => state.workbench.asking = None,
        None => {}
    }
}

/// What a validator plugin suggests for its finding: the files it would write, each said to be new or to replace one, and the button that writes them together.
fn plugin_fix(
    ctx: &egui::Context,
    state: &mut AppState,
    name: &str,
    code: &str,
    message: &str,
    fix: &or_plugin::SuggestedFix,
) {
    let listed = state.plugin_fix_files(fix);
    let mut answer: Option<bool> = None;
    // ⚠ The title says what the fix does, as the Fix question's does; the key is `Plugin fix` whatever it says, so Escape and the window keep one identity.
    question_titled(
        ctx,
        tr!("Plugin fix"),
        &format!("{}?", fix.label),
        460.0,
        |ui| {
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(icon::WARNING).color(theme::WARNING));
                ui.add(egui::Label::new(message).wrap());
            });
            ui.label(
                egui::RichText::new(tr!(
                    "Suggested by {name} ({code})",
                    name = name,
                    code = code
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            );
            ui.add_space(theme::SPACING_SNUG);
            ui.label(tr!("This writes these files, all together:"));
            for (path, replaces) in &listed {
                let what = if *replaces {
                    tr!("replaces the file")
                } else {
                    tr!("a new file")
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(format!("{path}  ·  {what}")).strong())
                        .wrap(),
                );
            }
            ui.label(
                egui::RichText::new(tr!("Undo takes them all back as one step."))
                    .color(theme::TEXT_SECONDARY),
            );
            ui.add_space(theme::SPACING_LOOSE);
            ui.horizontal(|ui| {
                if ui.button(tr!("Apply the fix")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr!("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        },
    );
    if question_escaped(ctx, tr!("Plugin fix")) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.apply_plugin_fix(name, fix);
        }
        Some(false) => state.workbench.asking = None,
        None => {}
    }
}

/// The line under a reason that is not written yet: Record the waiver waits for it, and says so where it is read.
fn reason_needed(ui: &mut egui::Ui, reason: &str) {
    if reason.trim().is_empty() {
        ui.label(
            egui::RichText::new(tr!("A reason is needed"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
}

/// The button that records a waiver, greyed with its reason while there is none; true once it is clicked.
fn record_button(ui: &mut egui::Ui, reason: &str) -> bool {
    ui.add_enabled(
        !reason.trim().is_empty(),
        egui::Button::new(tr!("Record the waiver")),
    )
    .on_disabled_hover_text(tr!("Write why first: the reason is saved with the waiver"))
    .clicked()
}

/// The reason a finding of a clothing content stands, asked for before it is recorded.
/// Why a finding of a validator plugin stands in this project, asked for before it is let stand.
fn waive_plugin(
    ctx: &egui::Context,
    state: &mut AppState,
    code: String,
    mut reason: String,
    focus: bool,
) {
    let mut answer: Option<bool> = None;
    question_within(ctx, tr!("Waive"), 440.0, |ui| {
        ui.label(
            egui::RichText::new(code.clone())
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            tr!("A waiver says why this stands. The build goes ahead, and the reason is kept in the project file for whoever reads it next."),
        );
        ui.horizontal(|ui| {
            ui.label(tr!("Reason"));
            let field = ui.add(egui::TextEdit::singleline(&mut reason).desired_width(320.0));
            field.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit,
                    true,
                    tr!("Reason for the waiver"),
                )
            });
            // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
            if focus || (reason.is_empty() && !field.has_focus()) {
                field.request_focus();
            }
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !reason.trim().is_empty()
            {
                answer = Some(true);
            }
        });
        reason_needed(ui, &reason);
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if record_button(ui, &reason) {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, tr!("Waive")) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            if let Err(why) = state.waive_plugin_finding(&code, &reason) {
                state.show_error_toast(why);
            }
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::WaivePlugin {
                code,
                reason,
                focus: false,
            });
        }
    }
}

fn waive(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    finding: or_wardrobe::Finding,
    mut reason: String,
    focus: bool,
) {
    let mut answer: Option<bool> = None;
    // ⚠ The title says what is waived, and its key is `Waive` whatever it says, so that Escape and the window keep one identity.
    let (kind, mark, colour) = match finding.severity {
        or_wardrobe::Severity::Error => ("error", icon::X_CIRCLE, theme::ERROR),
        or_wardrobe::Severity::Warning => ("warning", icon::WARNING, theme::WARNING),
        or_wardrobe::Severity::Info => ("note", icon::INFO, theme::TEXT_SECONDARY),
    };
    question_titled(
        ctx,
        tr!("Waive"),
        &tr!("Waive this {kind}", kind = kind),
        440.0,
        |ui| {
            // The finding is what is being decided, so it is the plainest text here, with what kind it is.
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(mark).color(colour));
                ui.add(egui::Label::new(finding.message.clone()).wrap());
            });
            ui.add_space(theme::SPACING_SNUG);
            ui.add(
            egui::Label::new(
                egui::RichText::new(
                    tr!("The build still goes ahead. Your reason is saved in the pack, so the next person who opens it sees why."),
                )
                .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
            ui.horizontal(|ui| {
                ui.label(tr!("Reason"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut reason)
                        .hint_text(
                            egui::RichText::new(tr!(
                                "For example: the shop wants the larger texture"
                            ))
                            .italics(),
                        )
                        .desired_width(320.0),
                );
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        tr!("Reason for the waiver"),
                    )
                });
                // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
                if focus || (reason.is_empty() && !field.has_focus()) {
                    field.request_focus();
                }
                if field.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !reason.trim().is_empty()
                {
                    answer = Some(true);
                }
            });
            reason_needed(ui, &reason);
            ui.add_space(theme::SPACING_LOOSE);
            ui.horizontal(|ui| {
                if record_button(ui, &reason) {
                    answer = Some(true);
                }
                if ui.button(tr!("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        },
    );
    if question_escaped(ctx, tr!("Waive")) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.waive_finding(module, finding.code.to_string(), finding.garment, reason);
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::Waive {
                module,
                finding,
                reason,
                focus: false,
            });
        }
    }
}
