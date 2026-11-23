use super::*;

use or_io::gta5::PhBoundAny;

pub(super) fn draw_ybn(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let bound = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::Bounds(b))) => b.clone(),
        _ => return,
    };

    let id_root = ui.make_persistent_id("hierarchy_ybn");
    let mut next_id: u64 = 0;
    draw_ybn_node(ui, state, doc_id, &bound, &mut next_id, id_root, "Root");
}

/// A bound's type as a row names it, lowercase.
pub(crate) fn bound_type_name(bound: &PhBoundAny) -> String {
    let header = crate::render::bound_meshes::header(bound);
    format!("{:?}", header.r#type).to_ascii_lowercase()
}

fn draw_ybn_node(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    bound: &PhBoundAny,
    next_id: &mut u64,
    parent_id: egui::Id,
    label_prefix: &str,
) {
    let id = *next_id;
    *next_id += 1;

    let header_text = format!("{} ({})", label_prefix, bound_type_name(bound));
    let stats = bound_stats(bound);

    let selected = state.selection.is_selected(doc_id, id);
    let row_id = parent_id.with(("bound", id));

    let label_color = if selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let icon = icons::bound_icon(
        matches!(bound, PhBoundAny::PhBoundComposite(_)),
        matches!(
            bound,
            PhBoundAny::PhBoundGeometry(_) | PhBoundAny::PhBoundBVH(_)
        ),
    );
    if let PhBoundAny::PhBoundComposite(comp) = bound {
        let job = icons::icon_label(&icon, header_text.clone(), label_color.into());
        // The nodes below this one are the next `node_count - 1` numbers.
        if revealing(state, doc_id)
            .is_some_and(|row| (id + 1..id + node_count(bound)).contains(&row))
        {
            force_open_collapsing(ui.ctx(), row_id);
        }
        let response = collapsing_row(
            ui,
            row_id,
            id == 0,
            |ui, _open| {
                let resp = ui.selectable_label(selected, job);
                ui.add(detail_label(stats.clone()));
                resp
            },
            |ui| {
                for (i, child) in comp.bounds.iter().enumerate() {
                    let Some(child) = child else {
                        continue;
                    };
                    draw_ybn_node(
                        ui,
                        state,
                        doc_id,
                        child,
                        next_id,
                        row_id.with("children"),
                        &format!("Child {}", i),
                    );
                }
            },
        );
        // ⚠ A closed composite draws none of its children, so the numbers its subtree holds are
        // skipped here, as the pick numbers every node.
        *next_id = id + node_count(bound);
        pulse_scroll_into_view(state, doc_id, id, &response);
        if response.clicked() {
            state.selection.select(doc_id, id);
        }
    } else {
        let job = icons::icon_label(&icon, header_text, label_color.into());
        let response = leaf_row(ui, |ui| {
            let r = ui.selectable_label(selected, job);
            ui.add(detail_label(stats));
            r
        });
        pulse_scroll_into_view(state, doc_id, id, &response);
        if response.clicked() {
            state.selection.select(doc_id, id);
        }
    }
}

/// How many nodes a bound holds, itself included, counted as the rows are numbered.
fn node_count(bound: &PhBoundAny) -> u64 {
    match bound {
        PhBoundAny::PhBoundComposite(c) => {
            1 + c
                .bounds
                .iter()
                .flatten()
                .map(|b| node_count(b))
                .sum::<u64>()
        }
        _ => 1,
    }
}

/// The geometry a polygonal bound holds, its own or its BVH's.
pub(crate) fn bound_geometry(bound: &PhBoundAny) -> Option<&or_io::gta5::PhBoundGeometry> {
    match bound {
        PhBoundAny::PhBoundGeometry(g) => Some(g),
        PhBoundAny::PhBoundBVH(b) => Some(&b.base),
        _ => None,
    }
}

pub(super) fn bound_stats(bound: &PhBoundAny) -> String {
    if let Some(g) = bound_geometry(bound) {
        return format!(
            "{} verts • {} polys",
            g.compressed_vertices.len(),
            g.polygons.len()
        );
    }
    if let PhBoundAny::PhBoundComposite(c) = bound {
        return format!("{} child bounds", c.bounds.len());
    }
    format!(
        "radius {:.3}",
        crate::render::bound_meshes::header(bound).radius_around_centroid
    )
}
