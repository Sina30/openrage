//! What the Builds window lets a person say about one build beyond its name, kind and ceilings: the author, version and description
//! its manifest or package states, and the options of the clothing it ships.
//!
//! ⚠ **Each control is one edit** of the build, as in the rest of the window. The options are keys of the build's own entry, only the ones
//! that differ from the default, so each build is its own override of how a pack is written.

use or_project::{ModuleKind, Output, OutputKind};
use or_wardrobe::options::{prefix_problem, DumpKind, Options};

use super::builds_editor::named_choice;
use crate::state::AppState;
use crate::ui::theme;

/// The words a build states about itself, with the label each has and what it is for.
const WORDS: [(&str, &str, &str); 3] = [
    (
        "author",
        tr_key!("Author"),
        tr_key!("Who made it, as the manifest or the package says"),
    ),
    (
        "version",
        tr_key!("Version"),
        tr_key!("A FiveM resource states any text; a package states major.minor, such as 1.2"),
    ),
    (
        "description",
        tr_key!("Description"),
        tr_key!("What it is, in a sentence"),
    ),
];

/// Whether the build `output` states `key` as text, and why `text` cannot be the value of it, none when it can.
fn word_problem(kind: &OutputKind, key: &str, text: &str) -> Option<&'static str> {
    if text.chars().any(char::is_control) {
        return Some(tr!("holds a character that is not text"));
    }
    match (kind, key) {
        (OutputKind::OivPackage, "version") => {
            let parts: Vec<&str> = text.split('.').collect();
            let ok = matches!(parts.as_slice(), [a, b] if a.parse::<u16>().is_ok() && b.parse::<u16>().is_ok());
            (!ok).then_some(tr!(
                "is not major.minor, two whole numbers with a dot between them"
            ))
        }
        _ => None,
    }
}

/// What the heading over the section says, which a person opens to see the words and the options.
pub(super) const HEADING: &str = tr_key!("Author, version and options");

/// The author, version and description, and the clothing options when the build ships a pack, under a heading that starts closed so the window stays short.
pub(super) fn draw(ui: &mut egui::Ui, state: &mut AppState, build: &Output) {
    let ships_clothing = (build.take.iter())
        .filter_map(|id| state.project_carried.container.module(id))
        .any(|m| m.kind == ModuleKind::Clothing);
    let has_words = build.kind != OutputKind::SingleplayerDlc;
    if !has_words && !ships_clothing {
        return;
    }
    // ⚠ A pack has no place for an author or a version, and a heading that said so would ask for them.
    let heading = if has_words {
        crate::i18n::tr(HEADING)
    } else {
        tr!("Options")
    };
    egui::CollapsingHeader::new(egui::RichText::new(heading).color(theme::TEXT_SECONDARY))
        .id_salt(("build_options", &build.id))
        .default_open(false)
        .show(ui, |ui| {
            words(ui, state, build);
            if ships_clothing {
                clothing(ui, state, build);
            }
        });
}

fn words(ui: &mut egui::Ui, state: &mut AppState, build: &Output) {
    if build.kind == OutputKind::SingleplayerDlc {
        return;
    }
    ui.add_space(theme::SPACING_HAIR);
    for (key, label, hint) in WORDS {
        let (label, hint) = (crate::i18n::tr(label), crate::i18n::tr(hint));
        let current = build
            .extra
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let problem_of = |text: &str| word_problem(&build.kind, key, text);
        let wanted = text_row(
            ui,
            ("build_word", &build.id, key),
            label,
            hint,
            &current,
            problem_of,
        );
        if let Some(text) = wanted {
            let what = tr!(
                "The build {name} states the {label}: {text}",
                name = build.name,
                label = label.to_lowercase(),
                text = text
            );
            let _ = state.edit_build(&build.id, what, |o| match text.is_empty() {
                true => {
                    o.extra.remove(key);
                }
                false => {
                    o.extra
                        .insert(key.to_string(), serde_json::Value::String(text.clone()));
                }
            });
        }
    }
}

/// A line of text with its label, kept as it is typed and returned when it is entered or left and is a value the build can take.
fn text_row(
    ui: &mut egui::Ui,
    salt: (&str, &str, &str),
    label: &str,
    hint: &str,
    current: &str,
    problem_of: impl Fn(&str) -> Option<&'static str>,
) -> Option<String> {
    let mut wanted = None;
    ui.horizontal(|ui| {
        ui.label(label);
        let key = egui::Id::new(salt);
        let mut draft: String = ui
            .data_mut(|d| d.get_temp(key))
            .unwrap_or_else(|| current.to_string());
        let field = ui.add(egui::TextEdit::singleline(&mut draft).desired_width(300.0));
        field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
        field.clone().on_hover_text(hint);
        let problem = problem_of(&draft);
        if field.lost_focus() && draft != current && problem.is_none() {
            wanted = Some(draft.clone());
        }
        match problem {
            Some(why) if draft != current => {
                ui.label(
                    egui::RichText::new(tr!("`{draft}` {why}", draft = draft, why = why))
                        .small()
                        .color(theme::ERROR),
                );
            }
            // ⚠ A draft that is the value, or was entered, is the value from now on.
            _ if !field.has_focus() => draft = current.to_string(),
            _ => {}
        }
        ui.data_mut(|d| d.insert_temp(key, draft));
    });
    wanted
}

fn clothing(ui: &mut egui::Ui, state: &mut AppState, build: &Output) {
    let (options, _) = Options::of(build);
    let resource = build.kind == OutputKind::FivemResource;
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(egui::RichText::new(tr!("Clothing")).color(theme::TEXT_SECONDARY));
    toggle(
        ui,
        state,
        build,
        &Toggle {
            label: tr!("A folder for each slot"),
            hover: tr!("Inside the folder of a ped's files, one folder for jackets, one for trousers and so on. Off, the files of a ped sit in one folder."),
            why_not: tr!("A pack is laid out as the game's own packs are, flat inside an archive for each ped's components and props"),
            on: options.subfolders,
            enabled: resource,
            history: (
                tr!("The build {name} has a folder for each slot", name = build.name),
                tr!("The build {name} no longer has a folder for each slot", name = build.name),
            ),
            set: |o, on| o.subfolders = on,
        },
    );
    toggle(
        ui,
        state,
        build,
        &Toggle {
            label: tr!("Creature metadata"),
            hover: tr!("The extra .ymt that carries the expressions a garment states, such as a heel, and the line of the shop file that names it. Off, those expressions are not written."),
            why_not: "",
            on: options.creature_metadata,
            enabled: true,
            history: (
                tr!("The build {name} writes the creature metadata", name = build.name),
                tr!("The build {name} no longer writes the creature metadata", name = build.name),
            ),
            set: |o, on| o.creature_metadata = on,
        },
    );
    toggle(
        ui,
        state,
        build,
        &Toggle {
            label: tr!("Alternate models"),
            hover: tr!("The models the game shows in place of a drawable, such as one with the hair hidden under a hat, and the count the variation metadata states for them. Off, they are left out."),
            why_not: "",
            on: options.alternates,
            enabled: true,
            history: (
                tr!("The build {name} ships the alternate models", name = build.name),
                tr!("The build {name} no longer ships the alternate models", name = build.name),
            ),
            set: |o, on| o.alternates = on,
        },
    );
    let wanted = text_row(
        ui,
        ("build_prefix", &build.id, "collection"),
        tr!("Collection prefix"),
        tr!("Text put in front of every collection name this build writes, in the file names, the metadata and the claim. Letters, digits and underscores."),
        &options.prefix,
        prefix_problem,
    );
    if let Some(prefix) = wanted {
        let what = tr!(
            "The build {name} puts \"{prefix}\" in front of its collections",
            prefix = prefix,
            name = build.name
        );
        let _ = state.edit_build(&build.id, what, |o| {
            let (mut options, _) = Options::of(o);
            options.prefix = prefix;
            options.store(o);
        });
    }
    toggle(
        ui,
        state,
        build,
        &Toggle {
            label: tr!("Credits file"),
            hover: tr!("A credits.md at the root of the resource that lists each source the pack uses with its author, licence and link, as the sources state them, and lists apart the sources that state none. Credit garment… states them."),
            why_not: tr!("A pack registers only files the game reads, so it leaves the credits file out"),
            on: options.credits,
            enabled: resource,
            history: (
                tr!("The build {name} writes a credits file", name = build.name),
                tr!("The build {name} no longer writes a credits file", name = build.name),
            ),
            set: |o, on| o.credits = on,
        },
    );
    let wanted = text_row(
        ui,
        ("build_tags", &build.id, "include"),
        tr!("Only garments tagged"),
        tr!("Words separated by commas. Only the garments filed under one of them ship in this build; the others keep their place as a drawable with no model, so the numbers of the rest do not move. Empty ships them all."),
        &options.include_tags.join(", "),
        |_| None,
    );
    if let Some(words) = wanted {
        let what = tr!(
            "The build {name} ships only the garments tagged {tags}",
            name = build.name,
            tags = if words.trim().is_empty() {
                tr!("with anything")
            } else {
                words.trim()
            }
        );
        let _ = state.edit_build(&build.id, what, |o| {
            let (mut options, _) = Options::of(o);
            let _ = options.set("include-tags", &words);
            options.store(o);
        });
    }
    dump(ui, state, build, &options, resource);
}

/// One option that is a yes or a no.
struct Toggle<'a> {
    label: &'a str,
    hover: &'a str,
    /// What the control says when it cannot be changed for this kind of build.
    why_not: &'a str,
    on: bool,
    enabled: bool,
    /// What the history says the build did when the option is turned on, and when it is turned off.
    history: (String, String),
    set: fn(&mut Options, bool),
}

fn toggle(ui: &mut egui::Ui, state: &mut AppState, build: &Output, option: &Toggle) {
    let mut value = option.on;
    let check = ui.add_enabled(
        option.enabled,
        egui::Checkbox::new(&mut value, option.label),
    );
    check.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            option.enabled,
            option.on,
            option.label,
        )
    });
    check
        .clone()
        .on_hover_text(option.hover)
        .on_disabled_hover_text(option.why_not);
    if check.changed() {
        let what = if value {
            option.history.0.clone()
        } else {
            option.history.1.clone()
        };
        let set = option.set;
        let _ = state.edit_build(&build.id, what, |o| {
            let (mut options, _) = Options::of(o);
            set(&mut options, value);
            options.store(o);
        });
    }
}

fn dump(
    ui: &mut egui::Ui,
    state: &mut AppState,
    build: &Output,
    options: &Options,
    resource: bool,
) {
    let mut chosen = options.dump;
    ui.horizontal(|ui| {
        ui.label(tr!("For a server's scripts"));
        let shown = ui.add_enabled_ui(resource, |ui| {
            egui::ComboBox::from_id_salt(("build_dump", &build.id))
                .selected_text(options.dump.label())
                .show_ui(ui, |ui| {
                    for choice in DumpKind::ALL {
                        ui.selectable_value(&mut chosen, choice, choice.label());
                    }
                })
        });
        named_choice(&shown.inner.response, tr!("Clothes dump of the build"), options.dump.label());
        shown
            .response
            .on_hover_text(tr!("A file in the resource that lists every texture of the pack: its collection, slot, place, texture and shop id. A resource carries it; a pack registers only files the game reads"));
    });
    if chosen != options.dump {
        let what = tr!(
            "The build {name} writes: {chosen}",
            name = build.name,
            chosen = chosen.label()
        );
        let _ = state.edit_build(&build.id, what, |o| {
            let (mut options, _) = Options::of(o);
            options.dump = chosen;
            options.store(o);
        });
    }
}
