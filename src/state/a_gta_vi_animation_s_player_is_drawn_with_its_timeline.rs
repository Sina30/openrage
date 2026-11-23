//! A GTA VI animation's player, the bar that picks what plays on which model over its timeline, is
//! drawn by the Timeline panel for the tab on screen, and by the Details panel only for an animation
//! no Timeline panel shows: one it is pinned to while another tab is on screen.

use std::sync::Arc;

use or_io::gta6::anim::{AnimResource, AnimSet, Animation, RootKind};

use super::an_edit_stays_where_it_was_made::texts;
use super::{AppState, DocPayload, DocumentId};
use crate::ui::dock::Workspace;
use crate::ui::panel_registry::{draw_panel, PanelContext, PanelId};
use crate::ui::panels::inspector::InspectorUiState;
use crate::ui::panels::registry::ids;
use crate::ui::panels::selection_ids::DOC_ROOT_SEL;

/// What the player's bar draws: the names of its two pickers, its box and its button.
const BAR: [&str; 4] = [
    "Animation",
    "Model",
    "Apply root motion",
    "Find the sets it names in the scan",
];

/// The animation's name, wider than a picker.
const NAME: &str = "a_walk_whose_name_is_longer_than_the_picker_that_shows_it";

/// Opens a clip dictionary of one animation that names a set another body stores.
fn animation(state: &mut AppState) -> DocumentId {
    let walk = Animation {
        name: NAME.into(),
        frames: 2,
        duration: 1.0,
        ..Animation::default()
    };
    let resource = AnimResource {
        root: RootKind::ClipDictionary,
        root_word: 2,
        name: "dict_a".into(),
        key: 0,
        block_map: None,
        word_at_60: 0,
        word_at_98: None,
        sets: vec![AnimSet {
            name: "dict_a".into(),
            key: 0,
            animations: vec![walk],
        }],
        external_sets: vec![1],
        animation_keys: Vec::new(),
        clips: Vec::new(),
        motions: Vec::new(),
        matchers: Vec::new(),
        dictionaries: Vec::new(),
        end_region: None,
        sections: None,
        edited: false,
    };
    let payload = DocPayload::Gta6Anim(Arc::new(resource));
    state.insert_document("dict_a.anim".into(), payload, None, None)
}

/// The second frame of the panel `id` drawn `width` wide: egui lays some widgets out only then.
fn drawn(state: &mut AppState, id: PanelId, width: f32) -> egui::FullOutput {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut frame = || {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
            )),
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let mut panel = PanelContext {
                state: &mut *state,
                bridge: None,
            };
            draw_panel(id, ui, &mut panel);
        })
    };
    frame();
    frame()
}

/// Which of the bar's texts the panel `id` paints.
fn bar_in(state: &mut AppState, id: PanelId) -> Vec<&'static str> {
    let painted = texts(&drawn(state, id, 900.0).shapes);
    (BAR.into_iter())
        .filter(|text| painted.iter().any(|(painted, _)| painted == text))
        .collect()
}

#[test]
fn the_timeline_panel_draws_the_player_of_the_animation_on_screen_and_the_details_panel_draws_none()
{
    let mut state = AppState::new();
    animation(&mut state);
    assert_eq!(state.active_workspace, Workspace::Animation);
    assert_eq!(bar_in(&mut state, ids::TIMELINE), BAR);
    assert_eq!(
        bar_in(&mut state, ids::DETAILS),
        [""; 0],
        "the details panel draws a second player"
    );

    // A panel narrower than the bar wraps it: each of its texts and its two pickers ends inside.
    const NARROW: f32 = 250.0;
    let output = drawn(&mut state, ids::TIMELINE, NARROW);
    let painted = texts(&output.shapes);
    let labels = BAR.into_iter().map(|text| {
        let (_, rect) = painted.iter().find(|(painted, _)| painted == text)?;
        Some(rect.right())
    });
    let nodes = (output.platform_output.accesskit_update).map_or_else(Vec::new, |u| u.nodes);
    let pickers = (nodes.iter())
        .filter(|(_, node)| node.role() == egui::accesskit::Role::ComboBox)
        .map(|(_, node)| node.bounds().map(|bounds| bounds.x1 as f32));
    let ends: Vec<Option<f32>> = labels.chain(pickers).collect();
    assert_eq!(ends.len(), BAR.len() + 2, "{ends:?}");
    assert!(
        ends.iter().all(|end| end.is_some_and(|end| end <= NARROW)),
        "the bar runs past a narrow panel: {ends:?}"
    );
}

#[test]
fn a_details_panel_pinned_to_an_animation_draws_its_player_while_another_tab_is_on_screen() {
    let mut state = AppState::new();
    let pinned = animation(&mut state);
    let model = DocPayload::Resource(or_io::Asset::Drawable(Default::default()));
    state.insert_document("prop.ydr".into(), model, None, None);
    assert_eq!(state.active_workspace, Workspace::Model);
    state.panels.get::<InspectorUiState>().pinned = Some((pinned, DOC_ROOT_SEL));
    assert_eq!(bar_in(&mut state, ids::DETAILS), BAR);
}
