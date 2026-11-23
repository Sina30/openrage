//! The line under the Wardrobe's list: how many problems the clothing has, the figures of the plan that have a ceiling or are over it, and the button that opens every figure.
//!
//! ⚠ **One line, not a table.** The figures a person can act on are on the line; the rest, and what the last build read back, are in the menu of All figures.

use crate::state::workbench::{BuildDot, Counts, LastBuild, Scope, Severity};
use crate::state::AppState;
use crate::ui::theme;

/// A figure of the plan: what it counts, what it is now and the ceiling the build declares for it, if it does.
pub(super) struct Figure {
    pub name: String,
    pub used: String,
    pub ceiling: Option<String>,
    pub over: bool,
}

impl Figure {
    /// `Drawables: 2 of 128`, or `Drawables: 2` where no ceiling is declared.
    pub(super) fn said(&self) -> String {
        match &self.ceiling {
            Some(ceiling) => tr!(
                "{name}: {used} of {ceiling}",
                ceiling = ceiling,
                name = self.name,
                used = self.used
            ),
            None => format!("{}: {}", self.name, self.used),
        }
    }
}

/// The id of the build the Build button makes, when it ships `module`.
fn output_of(state: &AppState, module: &str) -> Option<String> {
    (state.project_carried.container.outputs.iter())
        .find(|o| {
            Some(&o.id) == state.active_output_id().as_ref() && o.take.iter().any(|t| t == module)
        })
        .map(|o| o.id.clone())
}

pub(super) fn draw(ui: &mut egui::Ui, state: &AppState, module: &str) {
    let output = output_of(state, module);
    let figures = output
        .as_deref()
        .map_or_else(Vec::new, |o| figures(state, o));
    ui.horizontal_wrapped(|ui| {
        problems(ui, state, module);
        for figure in &figures {
            // ⚠ A figure with no ceiling is no limit to meet and waits in the menu; the memory is on the line whether or not the build declares a ceiling for it.
            if figure.ceiling.is_some() || figure.over || figure.name == crate::i18n::tr(MEMORY) {
                let colour = if figure.over {
                    theme::ERROR
                } else {
                    theme::TEXT_SECONDARY
                };
                ui.label(
                    egui::RichText::new(format!("· {}", figure.said()))
                        .small()
                        .color(colour),
                );
            }
        }
        if let Some(output) = output.as_deref() {
            all_figures(ui, state, output, &figures);
        }
    });
}

/// The figure that is the estimate of what the game streams for the files.
const MEMORY: &str = tr_key!("Estimated memory");

/// How many problems this clothing has, by kind: the count the Content panel and Properties say of the same content.
fn problems(ui: &mut egui::Ui, state: &AppState, module: &str) {
    let counts = state.workbench.counts(Scope::Content(module));
    let (words, colour) = match counts.worst() {
        None => (tr!("No problems").to_string(), theme::TEXT_SECONDARY.get()),
        Some(Severity::Warning) => (counts.said(), theme::WARNING.get()),
        Some(Severity::Error) => (counts.said(), theme::ERROR.get()),
    };
    ui.label(egui::RichText::new(words).small().color(colour));
}

/// Every figure of the plan of `output` against its ceiling, in the order a creator reads them, with the plan's own files and size last.
pub(super) fn figures(state: &AppState, output: &str) -> Vec<Figure> {
    let mut found = Vec::new();
    for resource in state.workbench.resources_of(output) {
        let plan = &resource.plan;
        let mut budgets: Vec<_> = plan.budgets.iter().collect();
        budgets.sort_by_key(|b| (FIGURES.iter().position(|f| *f == b.name), b.name.clone()));
        for budget in budgets {
            let (name, bytes) = figure_words(&budget.name);
            let shown = |n: u64| if bytes { size(n) } else { n.to_string() };
            found.push(Figure {
                name,
                used: shown(budget.used),
                ceiling: budget.ceiling.map(shown),
                over: budget.ceiling.is_some_and(|c| budget.used > c),
            });
        }
        found.push(Figure {
            name: tr!("Files").to_string(),
            used: plan.totals.files.to_string(),
            ceiling: plan.limits.files.map(|n| n.to_string()),
            over: plan.limits.files.is_some_and(|c| plan.totals.files > c),
        });
        found.push(Figure {
            name: tr!("Size").to_string(),
            used: size(plan.totals.bytes),
            ceiling: plan.limits.bytes.map(size),
            over: plan.limits.bytes.is_some_and(|c| plan.totals.bytes > c),
        });
    }
    found
}

/// The button that opens every figure, and what the last build read back.
fn all_figures(ui: &mut egui::Ui, state: &AppState, output: &str, figures: &[Figure]) {
    let menu = ui.menu_button(egui::RichText::new(tr!("All figures")).small(), |ui| {
        crate::ui::popup_of_the_shell(ui.ctx());
        ui.set_min_width(240.0);
        for figure in figures {
            ui.label(
                egui::RichText::new(figure.said())
                    .small()
                    .color(if figure.over {
                        theme::ERROR
                    } else {
                        theme::TEXT_SECONDARY
                    }),
            );
        }
        ui.separator();
        ui.add(egui::Label::new(egui::RichText::new(last_build(state, output)).small()).wrap());
    });
    menu.response.on_hover_text(
        tr!("Every figure of the plan of the chosen build against the ceiling the build declares for it, and what the last build read back"),
    );
}

/// The figures of a clothing content in the order the strip says them; any other figure follows, by name.
const FIGURES: [&str; 9] = [
    "clothing_drawables",
    "clothing_slots",
    "clothing_props",
    "clothing_textures",
    "clothing_shop_entries",
    "clothing_archive_entries",
    "clothing_metadata_bytes",
    "clothing_memory",
    "clothing_outfit_memory",
];

/// A size as a person reads it: bytes, KiB, MiB or GiB, in one unit everywhere.
fn size(bytes: u64) -> String {
    or_job::human_bytes(bytes)
}

/// A figure of the plan as words, and whether it counts bytes: the plan names it in snake case (`clothing_drawables`), and the memory is an estimate from the pages the files state.
fn figure_words(name: &str) -> (String, bool) {
    let (words, bytes) = match name {
        "clothing_drawables" => (tr!("Drawables"), false),
        "clothing_slots" => (tr!("Component drawables"), false),
        "clothing_props" => (tr!("Prop drawables"), false),
        "clothing_textures" => (tr!("Textures"), false),
        "clothing_shop_entries" => (tr!("Shop entries"), false),
        "clothing_archive_entries" => (tr!("Archive entries"), false),
        "clothing_metadata_bytes" => (tr!("Metadata size"), true),
        "clothing_memory" => (crate::i18n::tr(MEMORY), true),
        "clothing_outfit_memory" => (tr!("Heaviest outfit"), true),
        other => {
            let spaced = other.replace('_', " ");
            let mut chars = spaced.chars();
            let words = chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default();
            return (words, false);
        }
    };
    (words.to_string(), bytes)
}

/// The state of the chosen build beside its Build button: one or two words, in the status bar's own, and a colour for it.
pub(super) fn build_state(state: &AppState, output: &str) -> (String, egui::Color32) {
    let dot = state.workbench.dot(output);
    let colour = match dot {
        BuildDot::Never => theme::TEXT_SECONDARY,
        BuildDot::Running => theme::INFO,
        BuildDot::Built => theme::SUCCESS,
        BuildDot::Failed => theme::ERROR,
        BuildDot::Blocked | BuildDot::Unchecked => theme::WARNING,
    };
    (dot.word(Counts::default()), colour.get())
}

/// What the last build of `output` said, in a sentence; it never says the build loads in the game.
pub(super) fn last_build(state: &AppState, output: &str) -> String {
    let said = how_it_ended(state, output);
    let counts = state.workbench.counts(Scope::Build(output));
    match counts.errors {
        0 => said,
        _ => format!("{}.\n\n{said}", counts.stopping()),
    }
}

fn how_it_ended(state: &AppState, output: &str) -> String {
    match state.workbench.builds().last(output) {
        None => tr!("Not built in this session").to_string(),
        Some(LastBuild::Failed(reasons)) => tr!(
            "The last build stopped: {why}",
            why = crate::state::front_door::spoken(reasons.first().map_or("", String::as_str))
        ),
        Some(LastBuild::Built(done)) => {
            let checks: usize = done
                .parts
                .iter()
                .flat_map(|p| &p.pack_checks)
                .map(|(_, c)| c.len())
                .sum();
            let problems = done.pack_problems();
            match problems.first() {
                Some(first) => {
                    tr!("Built and read back; the pack check found a problem: {first}", first = first)
                }
                None => tr!(
                    "Built and read back: {files}, {checks} held.{frozen} Whether it loads in the game is not tested",
                    files = crate::state::front_door::counted(done.files() as usize, "file"),
                    checks = crate::state::front_door::counted(checks, "pack check"),
                    frozen = if done.frozen.is_empty() {
                        ""
                    } else {
                        tr!(" Its drawables are frozen.")
                    },
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_figure_is_named_in_a_persons_words_and_its_bytes_are_in_one_unit() {
        assert_eq!(
            figure_words("clothing_drawables"),
            ("Drawables".into(), false)
        );
        assert_eq!(
            figure_words("clothing_memory"),
            (crate::i18n::tr(MEMORY).into(), true)
        );
        assert_eq!(figure_words("clothing_shop_entries").0, "Shop entries");
        assert_eq!(figure_words("some_new_figure").0, "Some new figure");
        for name in FIGURES {
            assert!(
                !figure_words(name).0.contains(['_', '·'])
                    && !figure_words(name).0.contains("clothing"),
                "{name}"
            );
        }
        // The same count of bytes reads the same in a size and in a memory figure.
        assert_eq!(size(367 * 1024 * 1024 + 512 * 1024), "367.5 MiB");
    }
}
