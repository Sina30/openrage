//! One job per region: Files and the Outline on the left, the document in the
//! centre, the Inspector on the right.

use super::{AppState, DocumentId, DocumentKind, Prefs};
use crate::ui::panels::registry::ids;

/// Every labelled widget one frame of the tab strip and the dock lays out, and its centre.
fn frame(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        crate::ui::panels::document_tabs::draw(ui, state);
        crate::ui::dock::draw(ui, state, None);
    });
    output
        .platform_output
        .accesskit_update
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let text = node.label().or_else(|| node.value())?;
                    let b = node.bounds()?;
                    let centre =
                        egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                    Some((text.to_string(), centre))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Where the workspace on screen last laid out the leaf holding `panel`.
fn leaf_of(state: &AppState, panel: &str) -> egui::Rect {
    state
        .workspace_docks
        .peek(state.active_workspace)
        .and_then(|dock| {
            dock.iter_all_nodes().find_map(|(_, node)| match node {
                egui_dock::Node::Leaf(leaf) if leaf.tabs.iter().any(|t| t.id() == panel) => {
                    Some(leaf.rect)
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("{:?} has no {panel}", state.active_workspace))
}

/// Which of `names` the Outline draws, once the screen has settled.
fn outlined(ctx: &egui::Context, state: &mut AppState, names: &[&str]) -> Vec<String> {
    frame(ctx, state, Vec::new());
    let shown = frame(ctx, state, Vec::new());
    let outline = leaf_of(state, ids::OUTLINE);
    names
        .iter()
        .filter(|name| {
            shown
                .iter()
                .any(|(text, at)| outline.contains(*at) && text.contains(*name))
        })
        .map(|name| name.to_string())
        .collect()
}

#[test]
fn with_three_documents_open_the_outline_shows_only_the_active_one_and_switching_tabs_switches_it()
{
    let mut state = AppState::from_prefs(Prefs::default());
    let names = ["boulder", "moss", "wave"];
    let docs: Vec<DocumentId> = [DocumentKind::Ydr, DocumentKind::Ytd, DocumentKind::Ycd]
        .into_iter()
        .zip(names)
        .map(|(kind, name)| {
            let id = state.new_empty_document(kind);
            state.document_mut(id).expect("just made").name = name.to_string();
            id
        })
        .collect();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    assert_eq!(outlined(&ctx, &mut state, &names), ["wave"]);

    for index in [0, 1, 2, 0] {
        let shown = frame(&ctx, &mut state, Vec::new());
        let dock_top = leaf_of(&state, ids::OUTLINE)
            .top()
            .min(leaf_of(&state, ids::FILES).top());
        let tab = shown
            .iter()
            .find(|(text, at)| text.contains(names[index]) && at.y < dock_top)
            .map(|(_, at)| *at)
            .unwrap_or_else(|| panic!("no tab for {}: {shown:?}", names[index]));
        let press = |pressed| egui::Event::PointerButton {
            pos: tab,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(tab), press(true)],
        );
        frame(&ctx, &mut state, vec![press(false)]);
        assert_eq!(state.active_document, Some(docs[index]), "the tab click");
        assert_eq!(
            outlined(&ctx, &mut state, &names),
            [names[index]],
            "the Outline after switching to {}'s tab",
            names[index]
        );
    }
}
