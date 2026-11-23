//! The tags and the group of the garments picked, which are set on every one of them: a tag is shown as on all of them or on some, and a group that differs reads as several.

use egui_phosphor::regular as icon;
use or_wardrobe::Garment;

use super::place;
use crate::ui::panels::wardrobe::WardrobeUi;
use crate::ui::theme;

/// What the tags and the group asked for, carried out once the Garment panel is drawn.
#[derive(Default)]
pub(super) struct Asked {
    /// Tags to put on every garment picked.
    pub add: Vec<String>,
    /// A tag to take off every garment picked.
    pub remove: Option<String>,
    /// The group to put every garment picked in; none takes them out of theirs.
    pub group: Option<Option<String>>,
}

/// Each tag any of `of` has, in the order they are met, with how many of them have it.
fn tags_of(of: &[&Garment]) -> Vec<(String, usize)> {
    let mut found: Vec<(String, usize)> = Vec::new();
    for tag in of.iter().flat_map(|g| g.tags.iter()) {
        match found.iter_mut().find(|(seen, _)| seen == tag) {
            Some((_, n)) => *n += 1,
            None => found.push((tag.clone(), 1)),
        }
    }
    found
}

/// The group every one of `of` is in, none where they differ or none is in one; and whether they all agree.
fn group_of(of: &[&Garment]) -> (Option<String>, bool) {
    let first = of.first().and_then(|g| g.group.clone());
    let agree = of.iter().all(|g| g.group == first);
    (first.filter(|_| agree), agree)
}

pub(super) fn draw(ui: &mut egui::Ui, shown: &mut WardrobeUi, of: &[&Garment], asked: &mut Asked) {
    let Some(g) = of.first() else { return };
    let many = of.len();
    let heading = match many {
        1 => tr!("Tags and group").to_string(),
        n => tr!("Tags and group, set on all {n} selected", n = n),
    };
    super::heading(ui, &heading);
    ui.horizontal_wrapped(|ui| {
        for (tag, on) in tags_of(of) {
            // ⚠ A tag only some of them have is dimmed and says how many: taking it off acts on those, and a tag typed below goes on all.
            let some = on < many;
            let words = match some {
                true => tr!(
                    "{tag} · on {on} of {many}  {icon}",
                    tag = tag,
                    on = on,
                    many = many,
                    icon = icon::X
                ),
                false => format!("{tag} {}", icon::X),
            };
            let words = match some {
                true => egui::RichText::new(words).color(theme::TEXT_SECONDARY),
                false => egui::RichText::new(words),
            };
            let chip = ui.small_button(words);
            let name = match some {
                true => tr!(
                    "Remove tag {tag} (on {on} of {many})",
                    tag = tag,
                    on = on,
                    many = many
                ),
                false => tr!("Remove tag {tag}", tag = tag),
            };
            chip.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
            });
            let tip = match (some, many) {
                (_, 1) => tr!("Take the tag off").to_string(),
                (true, n) => tr!(
                    "On {on} of the {n} garments. Click to take it off those",
                    on = on,
                    n = n
                ),
                (false, n) => tr!(
                    "On all {n} garments. Click to take it off all of them",
                    n = n
                ),
            };
            if chip.on_hover_text(tip).clicked() {
                asked.remove = Some(tag.clone());
            }
        }
    });
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut shown.tag)
                .hint_text(tr!("Add a tag…"))
                .desired_width(120.0),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Add a tag"))
        });
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let button = ui
            .add_enabled(!shown.tag.trim().is_empty(), egui::Button::new(tr!("Add")))
            .on_disabled_hover_text(tr!("Type a tag first"));
        button.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Add tag"))
        });
        if entered || button.clicked() {
            let words = std::mem::take(&mut shown.tag);
            asked.add.extend(
                (words.split(','))
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_string),
            );
            // A tag after another is typed without reaching for the field again.
            if entered {
                field.request_focus();
            }
        }
    });
    // ⚠ The group of several that differ is not one of them: the field says Several and reads empty, and what is typed is set on all.
    let (now, agree) = group_of(of);
    let now = now.unwrap_or_default();
    let mut text = match &shown.group {
        Some((who, typed)) if *who == g.id => typed.clone(),
        _ => now.clone(),
    };
    place::field(ui, tr!("Group"), |ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut text)
                .hint_text(if agree { tr!("None") } else { tr!("Several") })
                .desired_width(ui.available_width().min(190.0)),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Group"))
        });
        if field.changed() {
            shown.group = Some((g.id.clone(), text.clone()));
        }
        if field.lost_focus() {
            let typed = shown.group.take().is_some();
            if typed && (!agree || text.trim() != now.trim()) {
                asked.group = Some(Some(text.clone()));
            }
        }
    });
}
