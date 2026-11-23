//! A scanned GTA VI fragment's glass panes, each outlined by its corners while Collision is on.

use or_io::gta6::headerless::{GlassPane, Resource};
use or_renderer::LineVertex;

use crate::state::{AppState, DocPayload};

const PANE: [f32; 4] = [0.55, 0.90, 1.0, 1.0];

/// The outline of each glass pane of the active GTA VI fragment whose owner group places it.
pub(crate) fn pane_lines(state: &AppState) -> Vec<LineVertex> {
    let Some(DocPayload::Gta6Drawable(resource)) = state.active_document().map(|d| &d.payload)
    else {
        return Vec::new();
    };
    match &**resource {
        Resource::DrawableWithBound(r) => match &r.fragment.panes {
            Some(Ok(panes)) => outlines(panes),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Each placed pane's four edges, two vertices an edge.
fn outlines(panes: &[GlassPane]) -> Vec<LineVertex> {
    let mut out = Vec::new();
    for c in panes.iter().filter_map(|p| p.corners) {
        for (k, from) in c.iter().enumerate() {
            out.push(LineVertex::new(*from, PANE));
            out.push(LineVertex::new(c[(k + 1) % 4], PANE));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placed_pane_is_four_closed_edges_and_an_unplaced_one_draws_nothing() {
        let corners = [
            [0.0, 0.0, 1.0],
            [2.0, 0.0, 1.0],
            [2.0, 0.0, 3.0],
            [0.0, 0.0, 3.0],
        ];
        let pane = |corners| GlassPane {
            base: [0.0; 3],
            width: [2.0, 0.0, 0.0],
            height: [0.0, 0.0, 2.0],
            uv_min: [0.0; 2],
            uv_max: [1.0; 2],
            outlines: vec![4],
            owner: None,
            corners,
        };
        let lines = outlines(&[pane(Some(corners)), pane(None)]);
        let at: Vec<[f32; 3]> = lines.iter().map(|v| v.position).collect();
        let want: Vec<[f32; 3]> = (0..4)
            .flat_map(|k| [corners[k], corners[(k + 1) % 4]])
            .collect();
        assert_eq!(at, want);
    }
}
