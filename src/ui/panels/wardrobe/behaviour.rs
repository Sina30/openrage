//! How the garment selected behaves: its flags, its drawable's mask, its audio, what a hat does to the hair, its first
//! person alternate and the sets of alternate variations it causes; and, with no garment selected, the sets of the pack.
//!
//! ⚠ A flag or a bit is offered by name only when a staged document states it (`or_wardrobe::flags::MEASURED`); the
//! others are kept as the number they are and shown as one, so nothing writes a bit nobody has measured.

use std::ops::RangeInclusive;

use or_wardrobe::alternate::{AlternateSet, Asset};
use or_wardrobe::audio::{self, Which};
use or_wardrobe::behaviour::Behaviour;
use or_wardrobe::edit::Edit;
use or_wardrobe::flags::{self, Flag};
use or_wardrobe::mask::{self, Property, RenderFlag};
use or_wardrobe::{hair, heels, Component, Garment, Ped, Slot, Wardrobe};

use super::garment::heading;
use super::WardrobeUi;
use crate::state::workbench::AlternatesEdit;
use crate::ui::theme;

/// What the panel keeps between frames.
#[derive(Default)]
pub(super) struct Ui {
    /// The flags being typed as a number: the garment and the text.
    flags: Option<(String, String)>,
    /// An audio id being typed: the garment, which one and the text.
    audio: Option<(String, Which, String)>,
    /// A number being dragged: the garment, the field and the value.
    number: Option<(String, &'static str, f32)>,
    /// The name of the set being made.
    set: String,
    /// The new name being typed for a set: the set and the text.
    rename: Option<(String, String)>,
    /// The switch being made: the collection, the slot, the drawable and the alternate.
    switch: Draft,
}

#[derive(Default)]
struct Draft {
    dlc: String,
    slot: Option<Component>,
    pack_drawable: Option<String>,
    index: f32,
    alt: f32,
}

/// What the controls asked for, carried out once the panel is drawn.
#[derive(Default)]
pub(super) struct Asked {
    pub edit: Option<(String, Edit)>,
    pub sets: Vec<AlternatesEdit>,
}

fn behave(asked: &mut Asked, g: &Garment, change: Behaviour) {
    asked.edit = Some((g.id.clone(), Edit::Behaviour(change)));
}

fn check(ui: &mut egui::Ui, label: &str, hint: &str, on: bool) -> Option<bool> {
    let mut now = on;
    let response = ui.checkbox(&mut now, label).on_hover_text(hint);
    response.changed().then_some(now)
}

/// A number set by dragging or typing, said once it is let go; none while it is as it was.
fn number(
    ui: &mut egui::Ui,
    held: &mut Option<(String, &'static str, f32)>,
    (who, field, name): (&str, &'static str, &str),
    now: f32,
    (range, speed, decimals): (RangeInclusive<f32>, f64, usize),
) -> Option<f32> {
    let mut value = match held {
        Some((g, f, v)) if g == who && *f == field => *v,
        _ => now,
    };
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .speed(speed)
            .range(range)
            .fixed_decimals(decimals),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, name));
    if response.changed() {
        *held = Some((who.to_string(), field, value));
    }
    if (response.drag_stopped() || response.lost_focus()) && held.is_some() {
        *held = None;
        if value != now {
            return Some(value);
        }
    }
    None
}

fn small(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    wardrobe: &Wardrobe,
    g: &Garment,
    asked: &mut Asked,
) {
    // The heels of shoes stay where they were, under their own heading.
    if g.slot == Some(Slot::Component(Component::Feet)) {
        heading(ui, tr!("Flags"));
        heels_row(ui, shown, g, asked);
    }
    egui::CollapsingHeader::new(tr!("Behaviour"))
        .id_salt(("behaviour", &g.id))
        .show(ui, |ui| {
            flag_boxes(ui, &mut shown.behaviour, g, asked);
            drawable(ui, &mut shown.behaviour, g, asked);
            hair_row(ui, &mut shown.behaviour, g, asked);
            audio_rows(ui, &mut shown.behaviour, g, asked);
            first_person(ui, &mut shown.behaviour, g, asked);
            heading(ui, tr!("Alternate variations"));
            causes(ui, &mut shown.behaviour, wardrobe, g, asked);
        });
}

/// The heels of shoes, with the value they state.
fn heels_row(ui: &mut egui::Ui, shown: &mut WardrobeUi, g: &Garment, asked: &mut Asked) {
    if g.slot != Some(Slot::Component(Component::Feet)) {
        return;
    }
    let now = heels::of(g);
    let mut on = now.is_some();
    let toggle = ui.checkbox(&mut on, tr!("Heels"));
    toggle
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, true, tr!("Heels")));
    if toggle
        .on_hover_text(tr!("Written as expression 4 of the shoes and its value, as the staged heels of the game state them"))
        .changed()
    {
        // 1 is the value the game's own heels state.
        asked.edit = Some((g.id.clone(), Edit::Heels(on.then_some(1.0))));
    }
    ui.add_enabled_ui(on, |ui| {
        let mut value = match &shown.heel {
            Some((who, v)) if *who == g.id => *v,
            _ => now.unwrap_or(1.0),
        };
        ui.horizontal(|ui| {
            ui.label(tr!("Value"));
            let field = ui
                .add(
                    egui::DragValue::new(&mut value)
                        .speed(0.01)
                        .range(-heels::LARGEST..=heels::LARGEST)
                        .fixed_decimals(2),
                )
                .on_disabled_hover_text(tr!("Tick Heels first"));
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, on, tr!("Heel value"))
            });
            if field.changed() {
                shown.heel = Some((g.id.clone(), value));
            }
            if (field.drag_stopped() || field.lost_focus()) && shown.heel.is_some() {
                shown.heel = None;
                if Some(value) != now {
                    asked.edit = Some((g.id.clone(), Edit::Heels(Some(value))));
                }
            }
        });
    });
    // ⚠ Outside the part that is greyed while Heels is off: a note that is dimmed like a disabled control reads as one.
    small(
        ui,
        tr!(
            "Preview only: the ped is raised by a tenth of this value. Ticking Heels sets it to 1."
        ),
    );
}

/// One checkbox for each flag some staged document states, and the number for the rest.
fn flag_boxes(ui: &mut egui::Ui, ui_state: &mut Ui, g: &Garment, asked: &mut Asked) {
    let word = flags::word(g);
    egui::CollapsingHeader::new(tr!("Behaviour flags"))
        .id_salt(("behaviour flags", &g.id))
        .default_open(word != 0)
        .show(ui, |ui| {
            egui::Grid::new((tr!("behaviour flag grid"), &g.id)).show(ui, |ui| {
                let mut column = 0;
                for flag in Flag::ALL.into_iter().filter(|f| f.measured()) {
                    let hint = format!("{} ({})", flag.hint(), flag.name());
                    if let Some(on) = check(ui, flag.label(), &hint, flags::has(g, flag)) {
                        behave(asked, g, Behaviour::Flag(flag, on));
                    }
                    column += 1;
                    if column % 2 == 0 {
                        ui.end_row();
                    }
                }
            });
            let mut text = match &ui_state.flags {
                Some((who, typed)) if *who == g.id => typed.clone(),
                _ => format!("{word:#X}"),
            };
            ui.horizontal(|ui| {
                ui.label(tr!("Flags as a number"));
                let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Flags as a number"))
                });
                if field.changed() {
                    ui_state.flags = Some((g.id.clone(), text.clone()));
                }
                if field.lost_focus() {
                    ui_state.flags = None;
                    let typed = text.trim();
                    let parsed = match typed.strip_prefix("0x").or_else(|| typed.strip_prefix("0X")) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => typed.parse::<u32>().ok(),
                    };
                    if let Some(new) = parsed.filter(|n| *n != word) {
                        behave(asked, g, Behaviour::Flags(new));
                    }
                }
            });
            let rest = flags::unnamed(word);
            if rest != 0 {
                let bits: Vec<String> = (0..32u32)
                    .filter(|b| rest >> b & 1 != 0)
                    .map(|b| match Flag::from_bit(b) {
                        Some(f) => tr!("bit {b} ({name})", b = b, name = f.name()),
                        None => format!("bit {b}"),
                    })
                    .collect();
                small(
                    ui,
                    &tr!("Stated and not named here, because no staged document states it: {bits}. It is kept as it is.", bits = bits.join(", ")),
                );
            }
        });
}

/// A component's mask, or a prop's render flags and how firmly it stays on.
fn drawable(ui: &mut egui::Ui, ui_state: &mut Ui, g: &Garment, asked: &mut Asked) {
    match g.slot {
        Some(Slot::Component(_)) => {
            egui::CollapsingHeader::new(tr!("Drawable properties"))
                .id_salt(("drawable properties", &g.id))
                .show(ui, |ui| {
                    for property in mask::offered(g) {
                        let hint = tr!("{name} bit {bit} of propMask", name = property.name(), bit = property.bit());
                        if let Some(on) = check(ui, property.label(), &hint, mask::has_property(g, property)) {
                            behave(asked, g, Behaviour::Property(property, on));
                        }
                    }
                    small(ui, tr!("The game asks for the r form of the files when the skin bit is set, and the pack ships the form its skin says"));
                    if mask::has_property(g, Property::MatchPrev) {
                        small(ui, tr!("A mask that matches the drawable before it asks for the m form of the files, which a pack never ships: take it off to build"));
                    }
                });
        }
        Some(Slot::Prop(_)) => {
            egui::CollapsingHeader::new(tr!("Prop properties"))
                .id_salt(("prop properties", &g.id))
                .show(ui, |ui| {
                    for flag in [RenderFlag::Alpha, RenderFlag::Cutout] {
                        let hint = tr!("{name} bit {bit} of renderFlags", name = flag.name(), bit = flag.bit());
                        if let Some(on) = check(ui, flag.label(), &hint, mask::has_render(g, flag)) {
                            behave(asked, g, Behaviour::Render(flag, on));
                        }
                    }
                    if mask::render_flags(g) & RenderFlag::Decal.mask() != 0 {
                        small(ui, tr!("Stated and not named here: the decal pass (PRF_DECAL), which no staged prop uses. It is kept as it is."));
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr!("Stays on"));
                        let now = f32::from(g.meta.stickyness.unwrap_or(0));
                        if let Some(v) = number(
                            ui,
                            &mut ui_state.number,
                            (g.id.as_str(), "stickyness", tr!("How firmly the prop stays on")),
                            now,
                            (0.0..=255.0, 1.0, 0),
                        ) {
                            behave(asked, g, Behaviour::Stickyness(Some(v as u8)));
                        }
                    });
                    small(ui, tr!("0 comes off whenever the ped is hurt and 255 never does"));
                });
        }
        None => {}
    }
}

/// What a hat does to the hair under it, and how tall a hair stands.
fn hair_row(ui: &mut egui::Ui, ui_state: &mut Ui, g: &Garment, asked: &mut Asked) {
    let (now, label, field, hint) = match g.slot {
        Some(Slot::Prop(or_wardrobe::Prop::Head)) => (
            hair::scale_of(g),
            tr!("Squash the hair under it"),
            tr!("hair scale"),
            tr!("Written as the first modifier of the hat and the expression that drives the hair scale bone; the staged hats state -0.5 most often, from -1.2 to -0.1"),
        ),
        Some(Slot::Component(Component::Hair)) => (
            hair::height_of(g),
            tr!("State a hair height"),
            tr!("hair height"),
            tr!("Written as the third modifier of the hair and the expression that drives the hair height bone; the staged hair states 0.18 to 1.9"),
        ),
        _ => return,
    };
    let on = now.is_some();
    let make = |value: Option<f32>| match g.slot {
        Some(Slot::Prop(_)) => Behaviour::HairScale(value),
        _ => Behaviour::HairHeight(value),
    };
    if let Some(turned) = check(ui, label, hint, on) {
        // -0.5 is the value most staged hats state; 0.19 the one most staged hair does.
        let first = if matches!(g.slot, Some(Slot::Prop(_))) {
            -0.5
        } else {
            0.19
        };
        behave(asked, g, make(turned.then_some(first)));
    }
    ui.add_enabled_ui(on, |ui| {
        ui.horizontal(|ui| {
            ui.label(tr!("Value"));
            // ⚠ A greyed field says why, in the row, where a keyboard reaches it too.
            if !on {
                small(ui, tr!("needs the box above"));
            }
            if let Some(v) = number(
                ui,
                &mut ui_state.number,
                (
                    g.id.as_str(),
                    field,
                    &tr!("Value of the {field}", field = field),
                ),
                now.unwrap_or(0.0),
                (-hair::LARGEST..=hair::LARGEST, 0.01, 2),
            ) {
                behave(asked, g, make(Some(v)));
            }
        });
    });
    if matches!(g.slot, Some(Slot::Prop(_))) {
        let hides = hair::hides_hair(g);
        let hide = ui.add_enabled(!hides, egui::Button::new(tr!("Hide hair")));
        let hint = tr!("States a hair scale of {hides}, which is how staged hats take the hair off: measured on staged hats and not verified in game", hides = hair::HIDES);
        let hide = hide
            .on_hover_text(hint)
            .on_disabled_hover_text(tr!("This hat already hides the hair"));
        if hide.clicked() {
            behave(asked, g, Behaviour::HairScale(Some(hair::HIDES)));
        }
    }
}

/// The audio ids of the garment, with the ids the game's own drawables of its slot state.
fn audio_rows(ui: &mut egui::Ui, ui_state: &mut Ui, g: &Garment, asked: &mut Asked) {
    let Some(slot) = g.slot else {
        return;
    };
    egui::CollapsingHeader::new(tr!("Audio"))
        .id_salt(("audio", &g.id))
        .show(ui, |ui| {
            let which: &[(Which, &str)] = match slot {
                Slot::Prop(_) => &[(Which::First, tr!("Audio id"))],
                Slot::Component(_) => &[(Which::First, tr!("Audio id")), (Which::Second, tr!("Second audio id"))],
            };
            for (which, name) in which.iter().copied() {
                let now = audio::get(g, which);
                let mut text = match &ui_state.audio {
                    Some((who, w, typed)) if *who == g.id && *w == which => typed.clone(),
                    _ => now.clone(),
                };
                ui.horizontal(|ui| {
                    ui.label(name);
                    let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(130.0));
                    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
                    if field.changed() {
                        ui_state.audio = Some((g.id.clone(), which, text.clone()));
                    }
                    if field.lost_focus() {
                        ui_state.audio = None;
                        if text.trim() != now {
                            behave(asked, g, Behaviour::Audio(which, Some(text.clone())));
                        }
                    }
                    let Some(presets) = audio::presets(slot) else {
                        return;
                    };
                    let list = match which {
                        Which::First => presets.first,
                        Which::Second => presets.second,
                    };
                    if list.is_empty() {
                        return;
                    }
                    let combo = egui::ComboBox::from_id_salt(("audio presets", &g.id, name))
                        .selected_text(tr!("Game's"))
                        .show_ui(ui, |ui| {
                            for (hash, uses) in list {
                                let id = audio::text_of(*hash);
                                let label = tr!("{id}, {uses} of {drawables}", id = id, uses = uses, drawables = presets.drawables);
                                if ui.selectable_label(id == now, label).clicked() {
                                    behave(asked, g, Behaviour::Audio(which, Some(id)));
                                }
                            }
                        });
                    combo.response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr!("{name} the game's drawables use", name = name))
                    });
                });
            }
            small(ui, tr!("The ids the game's own drawables of this slot state, most used first; the names are hashes this tree does not hold"));
        });
}

/// The alternate model the first person view streams in beside a component drawable.
fn first_person(ui: &mut egui::Ui, ui_state: &mut Ui, g: &Garment, asked: &mut Asked) {
    if !matches!(g.slot, Some(Slot::Component(_))) {
        return;
    }
    let now = g.meta.first_person_alternate;
    if let Some(on) = check(
        ui,
        tr!("First person alternate"),
        tr!("The local player's first person view streams in the alternate model of this number beside the drawable (a mask or a hat with the face cut away)"),
        now.is_some(),
    ) {
        behave(asked, g, Behaviour::FirstPerson(on.then_some(1)));
    }
    ui.add_enabled_ui(now.is_some(), |ui| {
        ui.horizontal(|ui| {
            ui.label(tr!("Alternate model number"));
            if let Some(v) = number(
                ui,
                &mut ui_state.number,
                (
                    g.id.as_str(),
                    tr!("first person"),
                    tr!("First person alternate model number"),
                ),
                f32::from(now.unwrap_or(1)),
                (1.0..=254.0, 1.0, 0),
            ) {
                behave(asked, g, Behaviour::FirstPerson(Some(v as u8)));
            }
        });
    });
    // A greyed number says why on the screen, where a tip is not read by everyone.
    if now.is_none() {
        small(ui, tr!("Tick First person alternate first"));
    }
    if now.is_some_and(|n| usize::from(n) > g.alternates.len()) {
        small(
            ui,
            tr!("The garment has fewer alternate models than that number"),
        );
    }
}

/// The sets this garment causes, and the sets it could.
fn causes(
    ui: &mut egui::Ui,
    ui_state: &mut Ui,
    wardrobe: &Wardrobe,
    g: &Garment,
    asked: &mut Asked,
) {
    let Some(ped) = g.ped else {
        small(ui, tr!("Give the garment a ped first"));
        return;
    };
    let mine: Vec<&AlternateSet> = wardrobe
        .alternates
        .iter()
        .filter(|s| s.has_source(&g.id))
        .collect();
    if mine.is_empty() {
        small(ui, tr!("Wearing it changes no other drawable"));
    }
    for set in &mine {
        ui.horizontal(|ui| {
            ui.label(tr!(
                "{name} ({count} switch(es))",
                name = set.name,
                count = set.switches.len()
            ));
            let stop = ui.small_button(tr!("Stop"));
            stop.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr!("Stop it changing the drawables of {name}", name = set.name),
                )
            });
            if stop.clicked() {
                asked.sets.push(AlternatesEdit::Leave {
                    set: set.id.clone(),
                    source: Asset::garment(&g.id),
                });
            }
        });
    }
    let others: Vec<&AlternateSet> = wardrobe
        .alternates
        .iter()
        .filter(|s| s.ped == ped && !s.has_source(&g.id))
        .collect();
    if !others.is_empty() {
        ui.horizontal(|ui| {
            let combo = egui::ComboBox::from_id_salt(("join a set", &g.id))
                .selected_text(tr!("Make it change the drawables of a set"))
                .show_ui(ui, |ui| {
                    for set in &others {
                        if ui.selectable_label(false, &set.name).clicked() {
                            asked.sets.push(AlternatesEdit::Join {
                                set: set.id.clone(),
                                source: Asset::garment(&g.id),
                            });
                        }
                    }
                });
            combo.response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::ComboBox,
                    true,
                    tr!("Make the garment cause a set"),
                )
            });
        });
    }
    ui.horizontal_wrapped(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut ui_state.set)
                .hint_text(tr!("A new set…"))
                .desired_width(130.0),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("Name of a new set of alternate variations"),
            )
        });
        let make = ui.add_enabled(
            !ui_state.set.trim().is_empty(),
            egui::Button::new(tr!("Make a set with this garment")),
        );
        let make = make.on_disabled_hover_text(tr!("Type a name for the set first"));
        if make.clicked() {
            asked.sets.push(AlternatesEdit::New {
                ped,
                name: std::mem::take(&mut ui_state.set),
                join: Some(g.id.clone()),
            });
        }
    });
}

/// Every set of the pack, with the drawables it changes and a place to make more.
pub(super) fn sets(ui: &mut egui::Ui, ui_state: &mut Ui, wardrobe: &Wardrobe, asked: &mut Asked) {
    heading(ui, tr!("Alternate variations"));
    small(
        ui,
        tr!("A set makes drawables wear an alternate model (the game's hair, a bag, a drawable of this pack) while the garments that cause it are worn"),
    );
    if wardrobe.alternates.is_empty() {
        small(ui, tr!("This pack has none"));
    }
    for set in &wardrobe.alternates {
        egui::CollapsingHeader::new(tr!(
            "{name} ({ped}: {count} switch(es), {count_2} cause(s))",
            name = set.name,
            ped = set.ped,
            count = set.switches.len(),
            count_2 = set.sources.len()
        ))
        .id_salt(("alternate set", &set.id))
        .show(ui, |ui| {
            let mut name = match &ui_state.rename {
                Some((id, typed)) if *id == set.id => typed.clone(),
                _ => set.name.clone(),
            };
            ui.horizontal(|ui| {
                ui.label(tr!("Name"));
                let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        tr!("Name of the set {name}", name = set.name),
                    )
                });
                if field.changed() {
                    ui_state.rename = Some((set.id.clone(), name.clone()));
                }
                if field.lost_focus() {
                    ui_state.rename = None;
                    if !name.trim().is_empty() && name.trim() != set.name {
                        asked.sets.push(AlternatesEdit::Rename {
                            set: set.id.clone(),
                            name: name.clone(),
                        });
                    }
                }
            });
            for (at, switch) in set.switches.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(tr!(
                        "{wardrobe} wears alternate {alt}",
                        wardrobe = asset_words(wardrobe, &switch.target),
                        alt = switch.alt
                    ));
                    let remove = ui.small_button(tr!("Remove"));
                    remove.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            tr!("Remove switch {at} of {set}", at = at + 1, set = set.name),
                        )
                    });
                    if remove.clicked() {
                        asked.sets.push(AlternatesEdit::RemoveSwitch {
                            set: set.id.clone(),
                            at,
                        });
                    }
                });
            }
            add_switch(ui, ui_state, wardrobe, set, asked);
            let remove = ui.button(tr!("Remove this set"));
            remove.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr!("Remove the set {name}", name = set.name),
                )
            });
            if remove.clicked() {
                asked.sets.push(AlternatesEdit::Remove {
                    set: set.id.clone(),
                });
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut ui_state.set)
                .hint_text(tr!("A new set…"))
                .desired_width(130.0),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("Name of a new set of alternate variations"),
            )
        });
        for ped in Ped::ALL {
            let word = super::words::ped_words(Some(ped));
            let make = ui
                .add_enabled(
                    !ui_state.set.trim().is_empty(),
                    egui::Button::new(tr!("{ped} set", ped = word)),
                )
                .on_hover_text(tr!("Make the set for the {ped} ped", ped = ped));
            if make.clicked() {
                asked.sets.push(AlternatesEdit::New {
                    ped,
                    name: std::mem::take(&mut ui_state.set),
                    join: None,
                });
            }
        }
    });
}

fn asset_words(wardrobe: &Wardrobe, asset: &Asset) -> String {
    match asset {
        Asset::Garment { garment } => match wardrobe.garment(garment) {
            Some(g) => tr!(
                "{rules} (this pack)",
                rules = or_wardrobe::rules::name_of(g)
            ),
            None => tr!("a garment the pack lacks").to_string(),
        },
        Asset::Outside(o) => format!(
            "{} {} {:03}",
            o.dlc.as_deref().unwrap_or(tr!("The game's own")),
            o.slot,
            o.index
        ),
    }
}

/// A switch for a drawable of the game or of the pack: its collection, its number and the alternate it wears.
fn add_switch(
    ui: &mut egui::Ui,
    ui_state: &mut Ui,
    wardrobe: &Wardrobe,
    set: &AlternateSet,
    asked: &mut Asked,
) {
    let draft = &mut ui_state.switch;
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut draft.dlc)
                .hint_text(tr!("Collection, empty for the game's own"))
                .desired_width(190.0),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("Collection of the hair or bag"),
            )
        });
        for (value, name) in [
            (&mut draft.index, tr!("Drawable number")),
            (&mut draft.alt, tr!("Alternate")),
        ] {
            ui.label(name);
            let drag = ui.add(
                egui::DragValue::new(value)
                    .range(0.0..=255.0)
                    .speed(1.0)
                    .fixed_decimals(0),
            );
            drag.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, name));
        }
    });
    ui.horizontal(|ui| {
        let slot = egui::ComboBox::from_id_salt(("slot of the drawable", &set.id))
            .selected_text(draft.slot.unwrap_or(Component::Hair).name())
            .show_ui(ui, |ui| {
                for c in Component::ALL {
                    if ui
                        .selectable_label(draft.slot == Some(c), c.name())
                        .clicked()
                    {
                        draft.slot = Some(c);
                    }
                }
            });
        slot.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                true,
                tr!("Slot of the drawable that changes"),
            )
        });
        let hairs = egui::ComboBox::from_id_salt(("a drawable of the pack", &set.id))
            .selected_text(tr!("Or a drawable of this pack"))
            .show_ui(ui, |ui| {
                for g in wardrobe.garments.iter().filter(|g| {
                    g.ped == Some(set.ped) && matches!(g.slot, Some(Slot::Component(_)))
                }) {
                    let chosen = draft.pack_drawable.as_deref() == Some(g.id.as_str());
                    if ui
                        .selectable_label(chosen, or_wardrobe::rules::name_of(g))
                        .clicked()
                    {
                        draft.pack_drawable = Some(g.id.clone());
                    }
                }
            });
        hairs.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                true,
                tr!("A drawable of this pack that changes"),
            )
        });
        let add = ui.button(tr!("Add switch"));
        add.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                tr!("Add a switch to {name}", name = set.name),
            )
        });
        if add.clicked() {
            let target = match draft.pack_drawable.take() {
                Some(id) => Asset::garment(&id),
                None => Asset::outside(
                    Some(draft.dlc.trim()).filter(|d| !d.is_empty()),
                    Slot::Component(draft.slot.unwrap_or(Component::Hair)),
                    draft.index as u32,
                ),
            };
            asked.sets.push(AlternatesEdit::AddSwitch {
                set: set.id.clone(),
                target,
                alt: draft.alt as u8,
            });
        }
    });
    small(ui, tr!("The drawable is the one of the collection named in the slot chosen (the game's own hair, or a bag), and the alternate is the number after its model's name"));
}
