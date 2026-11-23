//! What the filter understands, on screen: a button at the right end of the field opens the kinds of term it reads with what each asks, and a click puts an example of one in the field.
//!
//! ⚠ **Thin by design.** Which line is a query is `or-wardrobe`'s (`or_wardrobe::query`); this lists the words a person can write and puts one in the text.

use egui_phosphor::regular as icon;

use crate::ui::accessible_name::named;

/// The room the button takes at the right end of the field.
pub(super) const ROOM: f32 = 22.0;

/// One kind of term the filter reads: what it asks of the garments in plain words, an example that is a query as it stands, and the value of it that a person changes.
struct Term {
    asks: &'static str,
    example: &'static str,
    /// The end of the example that is a value to type over; none for a term that asks the one thing.
    value: &'static str,
}

const TERMS: [Term; 11] = [
    Term {
        asks: tr_key!("A tag you gave a garment"),
        example: "tag:wool",
        value: "wool",
    },
    Term {
        asks: tr_key!("A group of the list"),
        example: "group:coats",
        value: "coats",
    },
    Term {
        asks: tr_key!("Male or female"),
        example: "ped:female",
        value: "female",
    },
    Term {
        asks: tr_key!("A slot, by its short name"),
        example: "slot:jbib",
        value: "jbib",
    },
    Term {
        asks: tr_key!("More triangles than a number"),
        example: "polys>5000",
        value: "5000",
    },
    Term {
        asks: tr_key!("A model with no medium or low level of detail"),
        example: "lod:missing",
        value: "",
    },
    Term {
        asks: tr_key!("A garment that ships the same model as another"),
        example: "is:duplicate",
        value: "",
    },
    Term {
        asks: tr_key!("The copies of one garment"),
        example: "duplicate-of:jacket",
        value: "jacket",
    },
    Term {
        asks: tr_key!("A garment with a problem"),
        example: "is:problem",
        value: "",
    },
    Term {
        asks: tr_key!("One kind of problem, by its code"),
        example: "problem:model-missing",
        value: "model-missing",
    },
    Term {
        asks: tr_key!("In front of a term, the garments it does not match"),
        example: "-tag:wool",
        value: "wool",
    },
];

/// The button inside the field at `field`, which opens the terms; the example clicked and how many of its last characters are a value to type over.
pub(super) fn button(ui: &mut egui::Ui, field: egui::Rect) -> Option<(&'static str, usize)> {
    let side = field.height() - 4.0;
    let at = egui::Rect::from_min_size(
        egui::pos2(field.right() - side - 3.0, field.top() + 2.0),
        egui::vec2(side, side),
    );
    let button = ui.place(at, egui::Button::new(icon::FUNNEL).small().frame(false));
    let button = named(button, tr!("Filter terms")).on_hover_text(
        tr!("What the filter understands: a part of a name, or terms such as tag:wool, mixed. Click one to put it in the filter"),
    );
    let mut picked = None;
    egui::Popup::menu(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.set_max_width(340.0);
            ui.label(tr!("Type a part of a name, or ask with terms and mix them"));
            ui.add_space(crate::ui::theme::SPACING_TIGHT);
            egui::Grid::new("filter_terms")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for term in &TERMS {
                        let put = ui.add(egui::Button::new(
                            egui::RichText::new(term.example).monospace(),
                        ));
                        let put = named(
                            put,
                            &tr!("Add {example} to the filter", example = term.example),
                        );
                        if put.clicked() {
                            picked = Some((term.example, term.value.chars().count()));
                        }
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(term.asks))
                                .small()
                                .color(crate::ui::theme::TEXT_SECONDARY),
                        );
                        ui.end_row();
                    }
                });
        });
    picked
}

/// Put `example` at the end of `text` with a space before it, and its last `value` characters selected to be typed over once the field has the keyboard, which it takes when it is drawn next.
pub(super) fn put(
    ctx: &egui::Context,
    field: &egui::Response,
    text: &mut String,
    (example, value): (&str, usize),
) {
    if !text.is_empty() && !text.ends_with(' ') {
        text.push(' ');
    }
    text.push_str(example);
    let end = text.chars().count();
    let range = egui::text::CCursorRange::two(
        egui::text::CCursor::new(end - value),
        egui::text::CCursor::new(end),
    );
    let mut state = egui::text_edit::TextEditState::load(ctx, field.id).unwrap_or_default();
    state.cursor.set_char_range(Some(range));
    state.store(ctx, field.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ Falsified by an example the filter cannot read: a click would put an error in the field.
    #[test]
    fn every_example_the_list_offers_is_a_query_the_filter_reads_and_ends_in_its_value() {
        for term in &TERMS {
            let parsed = or_wardrobe::query::Query::parse(term.example);
            assert!(
                parsed.is_ok(),
                "`{}` is not read: {:?}",
                term.example,
                parsed.err()
            );
            assert!(!term.asks.is_empty());
            assert!(
                term.example.ends_with(term.value),
                "`{}` does not end in `{}`",
                term.example,
                term.value
            );
        }
    }
}
