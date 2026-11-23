//! A name for the AccessKit tree on a widget whose own is an icon or nothing.

use egui::accesskit::Role;

/// `response`, named `name` for a screen reader.
///
/// ⚠ After the widget is added: egui describes it as it is added, and a
/// button whose text is an icon-font glyph is described by that glyph.
pub fn named(response: egui::Response, name: &str) -> egui::Response {
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(name));
    response
}

/// As [`named`], for a name made per row of a list: `name` runs only while a screen reader is attached.
pub fn named_with(response: egui::Response, name: impl FnOnce() -> String) -> egui::Response {
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(name()));
    response
}

/// The widest a bar can be and still be a divider, and the shortest it can be.
const DIVIDER: (f32, f32) = (8.0, 24.0);

/// Whether `c` is from the icon font's private-use block.
fn is_icon(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// The most text a stop may hold and still be called by it: a cell, not a table.
const NAME_FROM_AT_MOST: usize = 3;

/// Names the widget the keyboard is on, at the end of a frame, when AccessKit has no name for it: by the text inside it, else as a divider when it is a thin bar.
///
/// ⚠ egui_dock's splitters and egui_extras' cells and column handles allocate their own widgets and describe none, and every one of them takes Tab; what holds no text and is no bar stays unnamed, which `every_tab_stop_in_the_dock_has_a_name_a_screen_reader_can_speak` lists.
///
/// ⚠ A `Ui` allocated with a sense, as a table's cell is, has no bounds in the tree but the empty rect it began with, so the rect is read from the widget and written back.
pub fn name_the_focus(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|m| m.focused()) else {
        return;
    };
    let Some(rect) = ctx.read_response(id).map(|response| response.rect) else {
        return;
    };
    let name = ctx.viewport(|viewport| {
        let nodes = &viewport.this_pass.accesskit_state.as_ref()?.nodes;
        let node = nodes.get(&id)?;
        // ⚠ Every Ui is a generic container, so a cell or a region allocated with a sense is one too.
        if !matches!(node.role(), Role::Unknown | Role::GenericContainer) || node.label().is_some()
        {
            return None;
        }
        let (thin, long) = (
            rect.width().min(rect.height()),
            rect.width().max(rect.height()),
        );
        if thin <= DIVIDER.0 && long >= DIVIDER.1 {
            return Some("Divider".to_string());
        }
        // Where a text starts, for a cell clipped narrower than its text is wide.
        let inside = |b: egui::accesskit::Rect| {
            let y = ((b.y0 + b.y1) / 2.0) as f32;
            (rect.min.x..=rect.max.x).contains(&(b.x0 as f32))
                && (rect.min.y..=rect.max.y).contains(&y)
        };
        let mut texts: Vec<(f64, f64, String)> = nodes
            .iter()
            .filter(|(other, node)| **other != id && node.role() == Role::Label)
            .filter(|(_, node)| node.bounds().is_some_and(inside))
            .filter_map(|(_, node)| {
                let text = node.value().or_else(|| node.label())?;
                // An icon-font glyph is a private-use character a screen reader says nothing for.
                let spoken: String = text.chars().filter(|c| !is_icon(*c)).collect();
                let at = node.bounds()?;
                (!spoken.trim().is_empty()).then(|| (at.y0, at.x0, spoken.trim().to_string()))
            })
            .collect();
        texts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        (1..=NAME_FROM_AT_MOST).contains(&texts.len()).then(|| {
            texts
                .into_iter()
                .map(|(_, _, text)| text)
                .collect::<Vec<_>>()
                .join(", ")
        })
    });
    ctx.accesskit_node_builder(id, |node| {
        if node
            .bounds()
            .is_none_or(|b| !b.x0.is_finite() || !b.x1.is_finite())
        {
            node.set_bounds(egui::accesskit::Rect {
                x0: rect.min.x.into(),
                y0: rect.min.y.into(),
                x1: rect.max.x.into(),
                y1: rect.max.y.into(),
            });
        }
        if let Some(name) = name {
            node.set_label(name);
        }
    });
}

/// A node `role` called `name` over `rect`, for something painted by hand that no widget stands behind.
///
/// ⚠ Describes only: the node takes no input and is no Tab stop, and `id` must be one no widget has.
pub fn described(ctx: &egui::Context, id: egui::Id, rect: egui::Rect, role: Role, name: &str) {
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(role);
        node.set_label(name);
        node.set_bounds(egui::accesskit::Rect {
            x0: rect.min.x.into(),
            y0: rect.min.y.into(),
            x1: rect.max.x.into(),
            y1: rect.max.y.into(),
        });
    });
}
