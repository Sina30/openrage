//! The size map of a build and what its clothing needs: the files the plan writes ranked by content, by garment and by slot, and the files and
//! references the garments name.
//!
//! ⚠ **Thin by design.** The weights are `or-project`'s (by content) and `or-wardrobe`'s (by garment and slot, and the dependencies), as the scan read
//! them or the build that wrote them; this lists them. A source a build cooks counts the payload it is made from until a build has cooked it, and
//! the figures of a build count the files it wrote only while the plan is still what that build wrote.

use or_project::sizes::{by_content, Weight};
use or_project::{Diff, Output};
use or_wardrobe::sizes::{Leads, Sizes};

use super::size;
use crate::state::front_door::counted;
use crate::state::workbench::{content_name, BuiltPart, LastBuild, PackRead, Resource};
use crate::state::AppState;
use crate::ui::panels::wardrobe::slot_words;
use crate::ui::theme;

/// How many rows the map lists before it counts the rest.
const SHOWN: usize = 40;

/// What the map is ranked by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    Content,
    Garment,
    Slot,
    Needs,
}

impl By {
    const ALL: [(By, &'static str); 4] = [
        (By::Content, tr_key!("By content")),
        (By::Garment, tr_key!("By garment")),
        (By::Slot, tr_key!("By slot")),
        (By::Needs, tr_key!("What it needs")),
    ];
}

/// The clothing the build ships, as the scan read it, each with the id of its content.
fn packs<'a>(state: &'a AppState, output: &Output) -> Vec<(&'a str, &'a PackRead)> {
    (state.workbench.packs().iter())
        .filter(|p| output.take.contains(&p.module))
        .filter_map(|p| Some((p.module.as_str(), p.read.as_ref().ok()?)))
        .collect()
}

/// The part of the last build of `output` that wrote `resource`, while the plan of that resource is what the build wrote.
fn written<'a>(state: &'a AppState, output: &Output, resource: &Resource) -> Option<&'a BuiltPart> {
    let LastBuild::Built(done) = state.workbench.builds().last(&output.id)? else {
        return None;
    };
    let unchanged = resource.since.as_ref().is_some_and(Diff::is_empty);
    unchanged
        .then(|| {
            done.parts
                .iter()
                .find(|p| p.resource == resource.plan.resource)
        })
        .flatten()
}

/// Which sizes the figures above it are: the files a build wrote, or the plan, which counts a source's payload until it is cooked.
fn basis(ui: &mut egui::Ui, built: bool) {
    let said = match built {
        true => tr!("As the last build wrote them, the cooked files at the sizes they came out at."),
        false => tr!("As planned: a source a build cooks counts the payload it is made from until a build has cooked it."),
    };
    ui.label(
        egui::RichText::new(said)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

/// How wide the bar of a row's share is, which the header's blank cell keeps.
const SHARE: f32 = 80.0;

/// The names of the columns of a row, over them: a bare `44` is a count of files only where the column says so.
fn header(ui: &mut egui::Ui, first: &str) {
    ui.horizontal(|ui| {
        ui.add_space(SHARE);
        ui.label(
            egui::RichText::new(format!("{:>10}  {:>5}  {first}", "Size", "Files"))
                .monospace()
                .color(theme::TEXT_SECONDARY),
        );
    });
}

fn row(ui: &mut egui::Ui, name: &str, files: u64, bytes: u64, of: u64) {
    ui.horizontal(|ui| {
        ui.add(
            egui::ProgressBar::new(bytes as f32 / of.max(1) as f32)
                .desired_width(SHARE)
                .text(format!("{:.0}%", 100.0 * bytes as f64 / of.max(1) as f64)),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("{:>10}  {:>5}  {name}", size(bytes), files))
                    .monospace(),
            )
            .truncate(),
        )
        .on_hover_text(name);
    });
}

/// The rows of a size map under a header, `first` naming what each row is.
fn weights(ui: &mut egui::Ui, first: &str, rows: &[Weight], total: u64) {
    header(ui, first);
    egui::ScrollArea::vertical()
        .id_salt("size_map_rows")
        .max_height(200.0)
        .show(ui, |ui| {
            for w in rows.iter().take(SHOWN) {
                row(ui, &w.name, w.files, w.bytes, total);
            }
            if rows.len() > SHOWN {
                ui.label(
                    egui::RichText::new(tr!("and {count} more", count = rows.len() - SHOWN))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });
}

pub(super) fn draw(ui: &mut egui::Ui, state: &AppState, output: &Output, resources: &[Resource]) {
    let key = egui::Id::new(("size_map_by", &output.id));
    let mut by: By = ui.data(|d| d.get_temp(key)).unwrap_or(By::Content);
    ui.horizontal(|ui| {
        for (choice, label) in By::ALL.map(|(by, label)| (by, crate::i18n::tr(label))) {
            ui.selectable_value(&mut by, choice, label);
        }
    });
    ui.data_mut(|d| d.insert_temp(key, by));
    match by {
        By::Content => {
            for resource in resources {
                let built = written(state, output, resource);
                let made = by_content(&resource.plan, built.map_or(&[], |p| p.cooked.as_slice()));
                if resources.len() > 1 {
                    ui.label(
                        egui::RichText::new(tr!(
                            "Resource {resource}",
                            resource = resource.plan.resource
                        ))
                        .strong(),
                    );
                }
                // ⚠ By the name the Content panel gives each content, and not the id the project file keeps it by.
                let mut rows: Vec<Weight> = (made.rows.iter())
                    .map(|w| Weight {
                        name: content_name(&state.project_carried.container, &w.name),
                        ..w.clone()
                    })
                    .collect();
                if made.own.files > 0 {
                    rows.push(made.own.clone());
                }
                weights(ui, tr!("Content"), &rows, made.total.bytes);
                ui.label(
                    egui::RichText::new(tr!(
                        "Total: {files}, {size}",
                        files = counted(made.total.files as usize, "file"),
                        size = size(made.total.bytes)
                    ))
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
                basis(ui, built.is_some());
            }
        }
        By::Garment | By::Slot => {
            let read = packs(state, output);
            if read.is_empty() {
                ui.label(
                    egui::RichText::new(tr!("This build ships no clothing."))
                        .color(theme::TEXT_SECONDARY),
                );
                return;
            }
            let mut all_built = true;
            for (module, pack) in read {
                let built = resources.iter().find_map(|r| {
                    let part = written(state, output, r)?;
                    let sizes = part.sizes.iter().find(|(content, _)| content == module);
                    sizes.map(|(_, sizes)| sizes)
                });
                all_built &= built.is_some();
                let sizes: &Sizes = built
                    .or_else(|| state.workbench.sizes_of(&output.id, module))
                    .unwrap_or(&pack.sizes);
                let rows: Vec<Weight> = match by {
                    By::Garment => sizes.garments.clone(),
                    _ => (sizes.slots.iter())
                        .map(|s| Weight {
                            name: format!(
                                "{} · {}",
                                crate::ui::panels::wardrobe::ped_words(Some(s.ped)),
                                slot_words(s.slot)
                            ),
                            files: s.files,
                            bytes: s.bytes,
                        })
                        .collect(),
                };
                weights(
                    ui,
                    if by == By::Garment {
                        tr!("Garment")
                    } else {
                        tr!("Slot")
                    },
                    &rows,
                    sizes.bytes,
                );
                ui.label(
                    egui::RichText::new(tr!(
                        "{pack} · {size} in all; the rest, {rest_size} in {rest_files}, is metadata and documents",
                        pack = pack.wardrobe.pack,
                        size = size(sizes.bytes),
                        rest_size = size(sizes.rest.bytes),
                        rest_files = counted(sizes.rest.files as usize, "file")
                    ))
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
            }
            basis(ui, all_built);
        }
        By::Needs => needs(ui, &packs(state, output)),
    }
}

fn needs(ui: &mut egui::Ui, read: &[(&str, &PackRead)]) {
    let deps: Vec<_> = read.iter().flat_map(|(_, r)| &r.dependencies).collect();
    if deps.is_empty() {
        ui.label(
            egui::RichText::new(tr!("The clothing names no file or source yet."))
                .color(theme::TEXT_SECONDARY),
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("size_map_needs")
        .max_height(200.0)
        .show(ui, |ui| {
            for dep in deps.iter().take(SHOWN) {
                let (word, colour) = match &dep.leads {
                    Leads::Inside => (tr!("in the pack").to_string(), theme::TEXT_SECONDARY),
                    Leads::Reference(words) => {
                        (tr!("refers to {words}", words = words), theme::TEXT_PRIMARY)
                    }
                    Leads::GameId(id) => (tr!("the game file {id}", id = id), theme::TEXT_PRIMARY),
                    Leads::Missing => (tr!("not there").to_string(), theme::ERROR),
                };
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(tr!(
                            "•  {size}  {path}  ({word}); used by {users}",
                            size = format!("{:>10}", size(dep.bytes)),
                            path = dep.path,
                            word = word,
                            users = dep.used_by.join(", ")
                        ))
                        .color(colour),
                    )
                    .wrap(),
                );
            }
            if deps.len() > SHOWN {
                ui.label(
                    egui::RichText::new(tr!("and {count} more", count = deps.len() - SHOWN))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });
}
