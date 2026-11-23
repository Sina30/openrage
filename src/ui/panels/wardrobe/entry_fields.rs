//! The shop entry of one texture, in the Shop section of the Garment panel: its price, its restriction tags, its forced components, and the other elements it states kept as written.
//!
//! ⚠ Thin by design. Every control asks for one `Edit::Shop` (one step of Undo) and `or_wardrobe::shop_edit` decides what it
//! may say; a refusal comes back as a toast from the edit. A lock hash, variant components and outfits are elements no staged
//! shop file states, so they are edited as the XML they are written in.

use std::collections::BTreeMap;

use or_wardrobe::edit::Edit;
use or_wardrobe::shop_edit::{self, Field, Forced};
use or_wardrobe::{Component, Garment, Slot};

use super::words::slot_words;
use crate::ui::theme;

/// What the controls keep between frames: text typed and not yet let go, by what it is for.
#[derive(Default)]
pub(super) struct Ui {
    drafts: BTreeMap<String, String>,
    /// The name and the XML of the element being added.
    element: (String, String),
    /// The restriction tag being typed.
    tag: String,
}

/// A line of text for `key`, drawn with `now` until something is typed; the text once the field is let go with something else in it.
fn typed(
    ui: &mut egui::Ui,
    shown: &mut Ui,
    (key, name): (String, String),
    now: &str,
    wide: Option<usize>,
) -> Option<String> {
    let mut text = shown
        .drafts
        .get(&key)
        .cloned()
        .unwrap_or_else(|| now.to_string());
    let field = match wide {
        Some(rows) => ui.add(
            egui::TextEdit::multiline(&mut text)
                .desired_rows(rows)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        ),
        None => ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0)),
    };
    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name.clone()));
    if field.changed() {
        shown.drafts.insert(key.clone(), text.clone());
    }
    if field.lost_focus() {
        shown.drafts.remove(&key);
        return (text != now).then_some(text);
    }
    None
}

fn small(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .small()
                .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
}

/// What the header of the entry says of it in a few words.
fn summary(letter: char, g: &Garment, texture: usize) -> String {
    let Some(entry) = g.variations.get(texture).and_then(|v| v.shop.as_ref()) else {
        return tr!(
            "Entry of texture {letter}: the build's own",
            letter = letter
        );
    };
    let mut said = Vec::new();
    if let Some(cost) = shop_edit_cost(entry) {
        said.push(tr!("price {cost}", cost = cost));
    }
    let tags = shop_edit::tags(entry).len();
    if tags > 0 {
        said.push(crate::state::front_door::counted(tags, "tag"));
    }
    let forced = shop_edit::forced(entry).map_or(1, |f| f.len());
    if forced > 0 {
        said.push(tr!("{forced} forced", forced = forced));
    }
    let others = shop_edit::others(entry).len();
    if others > 0 {
        said.push(crate::state::front_door::counted(
            others,
            tr!("other element"),
        ));
    }
    match said.is_empty() {
        true => tr!(
            "Entry of texture {letter}: the build's own",
            letter = letter
        ),
        false => tr!(
            "Entry of texture {letter}: {said}",
            letter = letter,
            said = said.join(", ")
        ),
    }
}

fn shop_edit_cost(entry: &or_wardrobe::ShopEntry) -> Option<&str> {
    entry.cost.as_deref()
}

/// The controls of the entry of texture `texture` of `g`, folded under a header that says what it states.
pub(super) fn draw(
    ui: &mut egui::Ui,
    shown: &mut Ui,
    g: &Garment,
    texture: usize,
    asked: &mut Option<(String, Edit)>,
) {
    let letter = char::from(b'a' + u8::try_from(texture).unwrap_or(25).min(25));
    let entry = g
        .variations
        .get(texture)
        .and_then(|v| v.shop.clone())
        .unwrap_or_default();
    let mut ask = |field: Field| *asked = Some((g.id.clone(), Edit::Shop(texture, field)));
    let key = |what: &str| format!("{}/{texture}/{what}", g.id);
    egui::CollapsingHeader::new(summary(letter, g, texture))
        .id_salt(("shop entry", &g.id, texture))
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr!("Price"));
                let now = entry.cost.clone().unwrap_or_default();
                if let Some(text) = typed(
                    ui,
                    shown,
                    (key("price"), tr!("Price of texture {letter}", letter = letter)),
                    &now,
                    None,
                ) {
                    ask(Field::Cost(Some(text)));
                }
            });
            small(ui, tr!("A whole number; empty is written as 0"));

            // The restriction tags.
            let tags = shop_edit::tags(&entry);
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Restriction tags"));
                for (at, tag) in tags.iter().enumerate() {
                    let button = ui.small_button(format!("{tag}  {}", egui_phosphor::regular::X));
                    let name = tr!("Take the restriction tag {tag} off texture {letter}", tag = tag, letter = letter);
                    button.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
                    });
                    if button.on_hover_text(tr!("Take the tag off")).clicked() {
                        let mut rest = tags.clone();
                        rest.remove(at);
                        ask(Field::Tags(rest));
                    }
                }
                let field = ui.add(
                    egui::TextEdit::singleline(&mut shown.tag)
                        .hint_text(tr!("Add a restriction tag"))
                        .desired_width(150.0),
                );
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        tr!("New restriction tag of texture {letter}", letter = letter),
                    )
                });
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if entered && !shown.tag.trim().is_empty() {
                    let mut all = tags.clone();
                    all.push(std::mem::take(&mut shown.tag));
                    ask(Field::Tags(all));
                }
            });

            // The forced components, as the staged files state them.
            match shop_edit::forced(&entry) {
                Some(forced) => forced_rows(ui, shown, (&key("forced"), letter), &forced, &mut ask),
                None => small(
                    ui,
                    tr!("This entry states forced components of a shape this does not name; they are kept as they are, under Other elements"),
                ),
            }

            // Everything else the entry states, as the XML it is written in: folded away unless the entry holds some, since most creators never meet it.
            ui.add_space(theme::SPACING_TIGHT);
            let others = shop_edit::others(&entry);
            egui::CollapsingHeader::new(tr!("Other elements"))
                .id_salt(("shop entry others", &g.id, texture))
                .default_open(!others.is_empty())
                .show(ui, |ui| {
                    small(
                        ui,
                        tr!("Anything else this entry holds is kept as it is. Add one only if you know how it is written."),
                    );
                    other_elements(ui, shown, (g, texture, letter), &others, &mut ask);
                });
        });
}

/// The elements of the entry that have no field of their own, each as the XML it is written in, and the three controls that state another: its name, its XML and the button, stacked so that none runs past the panel.
fn other_elements(
    ui: &mut egui::Ui,
    shown: &mut Ui,
    (g, texture, letter): (&Garment, usize, char),
    others: &[(String, String)],
    ask: &mut impl FnMut(Field),
) {
    let key = |what: &str| format!("{}/{texture}/{what}", g.id);
    for (name, xml) in others {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(name).monospace());
            let gone = ui.small_button(tr!("Remove"));
            let words = tr!(
                "Remove the element {name} of texture {letter}",
                name = name,
                letter = letter
            );
            gone.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, words.clone())
            });
            if gone.clicked() {
                ask(Field::Raw(name.clone(), None));
            }
        });
        let rows = xml.lines().count().clamp(1, 6);
        let said = tr!(
            "XML of the element {name} of texture {letter}",
            name = name,
            letter = letter
        );
        if let Some(text) = typed(
            ui,
            shown,
            (key(&format!("raw {name}")), said),
            xml,
            Some(rows),
        ) {
            ask(Field::Raw(name.clone(), Some(text)));
        }
    }
    let width = ui.available_width();
    let name = ui.add(
        egui::TextEdit::singleline(&mut shown.element.0)
            .hint_text(tr!("Name of the element, such as lockHash"))
            .desired_width(width),
    );
    name.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            tr!("Name of a new element of texture {letter}", letter = letter),
        )
    });
    let xml = ui.add(
        egui::TextEdit::singleline(&mut shown.element.1)
            .hint_text("<lockHash>…</lockHash>")
            .desired_width(width),
    );
    xml.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            tr!("XML of a new element of texture {letter}", letter = letter),
        )
    });
    let ready = !shown.element.0.trim().is_empty() && !shown.element.1.trim().is_empty();
    let add = ui
        .add_enabled(ready, egui::Button::new(tr!("Add element")))
        .on_disabled_hover_text(tr!("Name the element and write its XML first"));
    add.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ready,
            tr!("Add the element to texture {letter}", letter = letter),
        )
    });
    // Enter in the XML field adds, as the button does.
    let entered = xml.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if ready && (add.clicked() || entered) {
        let (name, xml) = std::mem::take(&mut shown.element);
        ask(Field::Raw(name.trim().to_string(), Some(xml)));
    }
}

/// The forced components of the entry, a row each, and a button for another.
fn forced_rows(
    ui: &mut egui::Ui,
    shown: &mut Ui,
    (key, letter): (&str, char),
    forced: &[Forced],
    ask: &mut impl FnMut(Field),
) {
    ui.label(tr!("Forced components"));
    for (at, one) in forced.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut component = one.component;
            let combo = egui::ComboBox::from_id_salt((key, at, "component"))
                .selected_text(slot_words(Slot::Component(component)))
                .show_ui(ui, |ui| {
                    for choice in Component::ALL {
                        ui.selectable_value(
                            &mut component,
                            choice,
                            slot_words(Slot::Component(choice)),
                        );
                    }
                });
            let name = tr!(
                "Component of forced component {at} of texture {letter}",
                at = at + 1,
                letter = letter
            );
            combo.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name.clone())
            });
            if component != one.component {
                let mut now = forced.to_vec();
                now[at].component = component;
                ask(Field::Forced(now));
            }
            ui.label(tr!("number"));
            let said = tr!(
                "Number of forced component {at} of texture {letter}",
                at = at + 1,
                letter = letter
            );
            if let Some(text) = typed(
                ui,
                shown,
                (format!("{key}/{at}/number"), said),
                &one.enum_value.to_string(),
                None,
            ) {
                if let Ok(number) = text.trim().parse::<u32>() {
                    let mut now = forced.to_vec();
                    now[at].enum_value = number;
                    ask(Field::Forced(now));
                }
            }
            ui.label(tr!("name"));
            let said = tr!(
                "Name hash of forced component {at} of texture {letter}",
                at = at + 1,
                letter = letter
            );
            if let Some(text) = typed(
                ui,
                shown,
                (format!("{key}/{at}/name"), said),
                &one.name_hash,
                None,
            ) {
                let mut now = forced.to_vec();
                now[at].name_hash = text;
                ask(Field::Forced(now));
            }
            let gone = ui.small_button(tr!("Remove"));
            let words = tr!(
                "Remove forced component {at} of texture {letter}",
                at = at + 1,
                letter = letter
            );
            gone.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, words.clone())
            });
            if gone.clicked() {
                let mut now = forced.to_vec();
                now.remove(at);
                ask(Field::Forced(now));
            }
        });
    }
    let add = ui.small_button(tr!("Add a forced component"));
    add.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            tr!(
                "Add a forced component to texture {letter}",
                letter = letter
            ),
        )
    });
    if add.clicked() {
        let mut now = forced.to_vec();
        now.push(Forced {
            name_hash: "0".into(),
            enum_value: 0,
            component: Component::Uppr,
        });
        ask(Field::Forced(now));
    }
}
