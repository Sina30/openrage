//! Adding a panel to an arrangement, taking one out, and floating one.
//!
//! These are the operations the View menu offers, kept apart from the menu
//! that offers them so they can be checked without a `Ui`.
//!
//! ⚠ **A floating window's close button removes the panel from the
//! arrangement**, which is data loss unless [`show`] puts it back.

use egui_dock::{DockState, SurfaceIndex, TabPath};

use crate::ui::dock::Tab;
use crate::ui::panel_registry::{self, PanelId};
use crate::ui::panels::registry::ids;

/// Where a panel is, if it is anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Docked,
    Floating,
    Absent,
}

pub fn placement(dock: &DockState<Tab>, id: PanelId) -> Placement {
    match path_of(dock, id) {
        Some(path) if path.surface.is_main() => Placement::Docked,
        Some(_) => Placement::Floating,
        None => Placement::Absent,
    }
}

fn path_of(dock: &DockState<Tab>, id: PanelId) -> Option<TabPath> {
    dock.iter_all_tabs()
        .find(|(_, tab)| tab.id() == id)
        .map(|(path, _)| path)
}

/// Put a panel back, in the main surface's focused leaf.
pub fn show(dock: &mut DockState<Tab>, id: PanelId) {
    if placement(dock, id) != Placement::Absent {
        return;
    }
    dock.main_surface_mut().push_to_focused_leaf(Tab(id));
}

/// Make a panel the visible, focused tab of its leaf, adding it if absent.
pub fn reveal(dock: &mut DockState<Tab>, id: PanelId) {
    show(dock, id);
    if let Some(path) = path_of(dock, id) {
        let _ = dock.set_active_tab(path);
        dock.set_focused_node_and_surface(path.node_path());
    }
}

/// The panels the rooms of the rail bring forward, which stand in the side column of a screen.
pub const SIDE: [PanelId; 6] = [
    ids::FILES,
    ids::PROJECT,
    ids::COMMAND_LOG,
    ids::PLUGINS,
    ids::SEARCH,
    ids::MODS,
];

/// The room panel in front in the side column of `dock`: the open tab of the first leaf that holds one, when it is one.
pub fn side_in_front(dock: &DockState<Tab>) -> Option<PanelId> {
    dock.iter_all_nodes().find_map(|(_, node)| match node {
        egui_dock::Node::Leaf(leaf) => {
            let open = leaf.tabs.get(leaf.active.0).map(|tab| tab.id())?;
            SIDE.contains(&open).then_some(open)
        }
        _ => None,
    })
}

/// Bring a panel forward in the side column, adding it there if absent: beside a panel of another room where one is docked, else in the leaf at the left edge, which is where the list of a screen stands.
///
/// ⚠ Not the focused leaf, which [`show`] adds to: that is the centre, and a room is a column beside it.
pub fn reveal_in_the_side(dock: &mut DockState<Tab>, id: PanelId) {
    if placement(dock, id) == Placement::Absent {
        // ⚠ The leaf at the left edge and, of those, the one at the top: the row of panels along the bottom of a project's screen starts at the left edge too, and is no side column.
        let place = |tabs: &[Tab], rect: egui::Rect| {
            let beside = tabs.iter().any(|tab| SIDE.contains(&tab.id()));
            // A leaf not laid out yet has no place: it counts as the top left corner.
            let at = |v: f32| if v.is_finite() { v.round() } else { 0.0 };
            (!beside, at(rect.min.x), at(rect.min.y))
        };
        let side = dock
            .iter_all_nodes_mut()
            .filter_map(|(_, node)| match node {
                egui_dock::Node::Leaf(leaf) => Some(leaf),
                _ => None,
            })
            .min_by(|a, b| {
                let (a, b) = (place(&a.tabs, a.rect), place(&b.tabs, b.rect));
                (a.0.cmp(&b.0))
                    .then(a.1.total_cmp(&b.1))
                    .then(a.2.total_cmp(&b.2))
            });
        if let Some(leaf) = side {
            leaf.tabs.push(Tab(id));
        }
    }
    reveal(dock, id);
}

/// Take a panel out of the arrangement entirely.
pub fn hide(dock: &mut DockState<Tab>, id: PanelId) {
    if let Some(path) = path_of(dock, id) {
        dock.remove_tab(path);
    }
}

/// Float a docked panel in its own window.
pub fn detach(dock: &mut DockState<Tab>, id: PanelId, bounds: egui::Rect) -> Option<SurfaceIndex> {
    let path = path_of(dock, id)?;
    if !path.surface.is_main() {
        return None;
    }
    // ⚠ The last tab of the main surface cannot leave it: `detach_tab`
    // removes the leaf it emptied, and a main surface with no leaf renders
    // nothing at all.
    if dock.main_surface().num_tabs() <= 1 {
        return None;
    }
    Some(dock.detach_tab(path, bounds))
}

/// Every panel the View menu can offer, in registration order.
pub fn offerable(state: &crate::state::AppState) -> Vec<(PanelId, String)> {
    panel_registry::all()
        .into_iter()
        .map(|panel| (panel.id(), panel.title(state)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dock::{Workspace, WorkspaceDocks};

    fn particle() -> DockState<Tab> {
        let mut docks = WorkspaceDocks::default();
        docks.get_mut(Workspace::Particle).clone()
    }

    #[test]
    fn hiding_a_panel_then_showing_it_puts_it_back() {
        let mut dock = particle();
        let id = Tab::DETAILS.id();
        assert_eq!(placement(&dock, id), Placement::Docked);

        hide(&mut dock, id);
        assert_eq!(placement(&dock, id), Placement::Absent);

        show(&mut dock, id);
        assert_eq!(placement(&dock, id), Placement::Docked);
    }

    #[test]
    fn a_detached_panel_reports_itself_floating() {
        let mut dock = particle();
        let id = Tab::DETAILS.id();
        let bounds = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));

        assert!(detach(&mut dock, id, bounds).is_some());
        assert_eq!(placement(&dock, id), Placement::Floating);
        assert!(
            detach(&mut dock, id, bounds).is_none(),
            "a floating panel cannot be detached again"
        );
    }

    /// ⚠ A main surface emptied by a detach renders nothing.
    #[test]
    fn the_last_docked_panel_cannot_float_away() {
        let mut dock = DockState::new(vec![Tab::VIEWPORT_3D]);
        assert!(detach(
            &mut dock,
            Tab::VIEWPORT_3D.id(),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
        )
        .is_none());
        assert_eq!(placement(&dock, Tab::VIEWPORT_3D.id()), Placement::Docked);
    }

    #[test]
    fn every_registered_panel_can_be_offered_by_name() {
        let offered = offerable(&crate::state::AppState::default());
        assert!(
            offered.len() > 5,
            "only {} panel(s) registered, so this is asserting nothing",
            offered.len()
        );
        for (id, label) in &offered {
            assert!(!label.is_empty(), "`{id}` has no menu label");
        }
    }
}
