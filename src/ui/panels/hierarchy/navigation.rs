//! Getting around the tree: back and forward, breadcrumbs, the sort order and
//! the places a user keeps.

use super::*;

/// Back and forward from the keyboard and the mouse's side buttons, while the
/// pointer is over the panel.
///
/// ⚠ **Scoped to the panel**, because the same buttons mean something else
/// over the timeline and Alt+Left belongs to a text field that has focus.
pub(super) fn history_input(ui: &egui::Ui, state: &mut AppState) {
    if !ui.ui_contains_pointer() {
        return;
    }
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (back, forward) = ui.ctx().input_mut(|i| {
        let back = i.pointer.button_pressed(egui::PointerButton::Extra1)
            || (!typing && i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft));
        let forward = i.pointer.button_pressed(egui::PointerButton::Extra2)
            || (!typing && i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight));
        (back, forward)
    });
    if back {
        state.navigate_back();
    } else if forward {
        state.navigate_forward();
    }
}

pub(super) fn history_buttons(ui: &mut egui::Ui, state: &mut AppState) {
    let back = ui.add_enabled(
        state.navigation.can_go_back(),
        egui::Button::new(ph::ARROW_LEFT).small(),
    );
    let back = crate::ui::accessible_name::named(back, tr!("Back"))
        .on_hover_text(tr!("Back (Alt+Left, or the mouse's back button)"));
    if back.clicked() {
        state.navigate_back();
    }
    let forward = ui.add_enabled(
        state.navigation.can_go_forward(),
        egui::Button::new(ph::ARROW_RIGHT).small(),
    );
    let forward = crate::ui::accessible_name::named(forward, tr!("Forward"))
        .on_hover_text(tr!("Forward (Alt+Right)"));
    if forward.clicked() {
        state.navigate_forward();
    }
}

pub(super) fn sort_menu(ui: &mut egui::Ui, state: &mut AppState) {
    let current = state.prefs.explorer.sort;
    let glyph = if current.descending {
        ph::SORT_DESCENDING
    } else {
        ph::SORT_ASCENDING
    };
    ui.menu_button(format!("{glyph} {}", current.key.label()), |ui| {
        for key in SortKey::ALL {
            if ui.radio(current.key == key, key.label()).clicked() {
                state.set_explorer_sort(SortOrder { key, ..current });
                ui.close_kind(egui::UiKind::Menu);
            }
        }
        ui.separator();
        let mut descending = current.descending;
        if ui
            .add_enabled(
                current.key != SortKey::Stored,
                egui::Checkbox::new(&mut descending, tr!("Descending")),
            )
            .changed()
        {
            state.set_explorer_sort(SortOrder {
                descending,
                ..current
            });
        }
        ui.label(
            egui::RichText::new(tr!("Folders stay first in either direction."))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    })
    .response
    .on_hover_text(tr!("Sort archive contents"));
}

/// Go to File: the focus in the field of Files, with what the words find listed under it.
///
/// ⚠ With nothing open the start page fills the window and no Files panel is drawn, so the box of the palette is the field.
pub(crate) fn go_to_file(state: &mut AppState) {
    if state.documents.is_empty() {
        return crate::ui::panels::command_palette::go_to_file(state);
    }
    let on_screen = state.active_workspace;
    (state.workspace_docks).reveal(on_screen, crate::ui::panels::registry::ids::FILES);
    state.hide_start_page();
    // Each asking looks again for what changed on disk outside the tree.
    state.file_index.recheck = true;
    crate::ui::panels::command_palette::begin_finding(state);
    let files = ui_state(state);
    files.filter.clear();
    files.deep = false;
    files.focus_filter_next_frame = true;
    files.open_matches = true;
}

/// Go to file… as a button of the panel: the search of the whole install by name or by hash was on a key and in the palette alone.
pub(super) fn go_to_file_button(ui: &mut egui::Ui, state: &mut AppState) {
    use crate::ui::commands::{self, Action};

    let Some(command) = commands::of(state, Action::GoToFile) else {
        return;
    };
    let text = tr!("{icon} Find", icon = ph::MAGNIFYING_GLASS);
    let button = ui.add_enabled(command.enabled.is_ok(), egui::Button::new(text));
    let button = crate::ui::accessible_name::named(button, &command.label);
    if commands::hints(button, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
    }
}

/// Dock the preview pane in the workspace on screen.
pub(super) fn preview_pane_button(ui: &mut egui::Ui, state: &mut AppState) {
    let eye = crate::ui::accessible_name::named(ui.button(ph::EYE), tr!("Show the preview pane"));
    if eye
        .on_hover_text(tr!("Show the preview pane for the selected entry"))
        .clicked()
    {
        let ws = state.active_workspace;
        state
            .workspace_docks
            .reveal(ws, crate::ui::panels::registry::ids::FILE_PREVIEW);
    }
}

/// Hand the filter on to the search that reaches archives nobody has opened.
pub(super) fn escalate_button(ui: &mut egui::Ui, state: &mut AppState, filter: &str) {
    if ui
        .small_button(tr!(
            "{icon} Search every archive",
            icon = ph::MAGNIFYING_GLASS
        ))
        .on_hover_text(tr!(
            "Look for {explorer} through the whole mount, nested archives included",
            explorer = explorer::escalated_glob(filter)
        ))
        .clicked()
    {
        let hierarchy = ui_state(state);
        hierarchy.filter = explorer::escalated_glob(filter);
        hierarchy.deep = true;
        state.deep_find.clear();
    }
}

/// Favourites to open, recent projects to open, and recent files to open or pin.
pub(super) fn places_menu(ui: &mut egui::Ui, state: &mut AppState) {
    ui.menu_button(tr!("{icon} Places", icon = ph::STAR), |ui| {
        let favourites = state.prefs.explorer.favourites.clone();
        ui.label(
            egui::RichText::new(tr!("Favourites"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        if favourites.is_empty() {
            ui.label(
                egui::RichText::new(tr!("Right-click a row ▸ Add to favourites"))
                    .small()
                    .color(theme::TEXT_DISABLED),
            );
        }
        let last = favourites.len().saturating_sub(1);
        for (index, place) in favourites.into_iter().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .button(place.label())
                    .on_hover_text(format!("{}\n{}", place.source, place.inner))
                    .clicked()
                {
                    state.open_place(place.clone());
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui
                    .add_enabled(index > 0, egui::Button::new(ph::ARROW_UP).small())
                    .clicked()
                {
                    state.move_favourite(index, index - 1);
                }
                if ui
                    .add_enabled(index < last, egui::Button::new(ph::ARROW_DOWN).small())
                    .clicked()
                {
                    state.move_favourite(index, index + 1);
                }
                if ui
                    .small_button(ph::X)
                    .on_hover_text(tr!("Remove from favourites"))
                    .clicked()
                {
                    state.toggle_favourite(place.clone());
                }
            });
        }

        let projects = state.prefs.recent_projects.clone();
        if !projects.is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new(tr!("Recent projects"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        for path in projects {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            if ui
                .button(name)
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                // ⚠ As every other door: it asks where when a project is open, and a project that is gone shows the start page.
                state.open_project_asking(path);
                ui.close_kind(egui::UiKind::Menu);
            }
        }

        let recent: Vec<_> = (state.prefs.recent_files.iter())
            .filter(|r| r.exists())
            .cloned()
            .collect();
        if !recent.is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new(tr!("Recent files"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        for file in recent {
            let place = explorer::Place {
                source: file.source.clone(),
                inner: file.inner.clone(),
            };
            ui.horizontal(|ui| {
                if ui
                    .button(place.label())
                    .on_hover_text(file.location())
                    .clicked()
                {
                    state.open_place(place.clone());
                    ui.close_kind(egui::UiKind::Menu);
                }
                let pinned = state.prefs.explorer.is_favourite(&place);
                if ui
                    .add_enabled(!pinned, egui::Button::new(ph::PUSH_PIN).small())
                    .on_hover_text(tr!("Pin to favourites"))
                    .on_disabled_hover_text(tr!("Already a favourite"))
                    .clicked()
                {
                    state.toggle_favourite(place);
                }
            });
        }
    })
    .response
    .on_hover_text(tr!("Favourites, recent projects and recent files"));
}

pub(super) fn breadcrumbs(ui: &mut egui::Ui, state: &mut AppState) {
    let trail = state.breadcrumb_trail();
    let mut go: Option<Location> = None;
    ui.horizontal_wrapped(|ui| {
        history_buttons(ui, state);
        ui.spacing_mut().item_spacing.x = 3.0;
        let last = trail.len().saturating_sub(1);
        for (index, crumb) in trail.iter().enumerate() {
            if index > 0 {
                ui.label(egui::RichText::new("›").small().color(theme::TEXT_DISABLED));
            }
            let color = if index == last {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            };
            let text = egui::RichText::new(&crumb.label).small().color(color);
            if ui
                .add(egui::Label::new(text).sense(egui::Sense::click()))
                .on_hover_text(tr!("Go here"))
                .clicked()
            {
                go = Some(crumb.to);
            }
        }
    });
    if let Some(to) = go {
        state.navigate_to(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::navigation::{resolve_path, Resolved};

    /// ⚠ **These draw real frames and read what the drawing wrote**, not what
    /// it painted: the order the rows were laid out in, and whether the tree
    /// consumed a reveal. A render-only check would pass with the sort and
    /// the reveal both disconnected.
    fn frame(ctx: &egui::Context, state: &mut AppState, n: usize) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 4000.0),
            )),
            time: Some(n as f64 / 30.0),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| super::super::draw(ui, state));
    }

    fn archive_state() -> (AppState, DocumentId) {
        let mut state = AppState::default();
        let doc = state.new_empty_document(DocumentKind::Rpf);
        let DocPayload::Rpf(rpf) = &mut state.document_mut(doc).unwrap().payload else {
            panic!("an empty RPF document holds an RPF payload");
        };
        let root = rpf.session.root_id();
        rpf.session
            .add_file_copy(root, "lod10.ydr", vec![0; 8])
            .unwrap();
        let deep = rpf.session.add_folder(root, "props").unwrap();
        let inner = rpf.session.add_folder(deep, "deep").unwrap();
        rpf.session
            .add_file_copy(inner, "x.ydr", vec![0; 8])
            .unwrap();
        rpf.session
            .add_file_copy(root, "lod2.ydr", vec![0; 64])
            .unwrap();
        state.set_active_document(doc);
        // The default view and order, whatever the preferences held.
        state.prefs.explorer = Default::default();
        (state, doc)
    }

    fn id(state: &AppState, doc: DocumentId, path: &str) -> u64 {
        let DocPayload::Rpf(rpf) = &state.document(doc).unwrap().payload else {
            unreachable!()
        };
        match resolve_path(&rpf.session, path) {
            Resolved::Found(id) => id,
            other => panic!("{path}: {other:?}"),
        }
    }

    fn drawn(state: &mut AppState, doc: DocumentId) -> Vec<u64> {
        super::super::ui_state(state)
            .visible_rows
            .get(&doc)
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn the_tree_lays_rows_out_in_the_sort_order() {
        let (mut state, doc) = archive_state();
        let ctx = egui::Context::default();
        for n in 0..3 {
            frame(&ctx, &mut state, n);
        }
        let by_name = vec![
            id(&state, doc, "props"),
            id(&state, doc, "lod2.ydr"),
            id(&state, doc, "lod10.ydr"),
        ];
        assert_eq!(drawn(&mut state, doc), by_name);

        state.prefs.explorer.sort = SortOrder {
            key: SortKey::Size,
            descending: false,
        };
        frame(&ctx, &mut state, 3);
        assert_eq!(
            drawn(&mut state, doc),
            vec![
                id(&state, doc, "props"),
                id(&state, doc, "lod10.ydr"),
                id(&state, doc, "lod2.ydr"),
            ]
        );
    }

    #[test]
    fn a_shift_click_spans_the_rows_as_this_frame_drew_them() {
        let (mut state, doc) = archive_state();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0);
        let (props, lod10) = (id(&state, doc, "props"), id(&state, doc, "lod10.ydr"));
        state.selection.select(doc, props);
        super::super::ui_state(&mut state).pending_range = Some((doc, lod10));
        frame(&ctx, &mut state, 1);

        let mut selected = state.selection.selected_ids(doc);
        selected.sort_unstable();
        let mut expected = vec![props, id(&state, doc, "lod2.ydr"), lod10];
        expected.sort_unstable();
        assert_eq!(selected, expected);
    }

    /// ⚠ **Clicks the button through a real frame**; the glob helper being
    /// right says nothing about whether the readout offers it or the click
    /// reaches the search.
    #[test]
    fn the_filter_escalates_into_the_archive_wide_search() {
        let (mut state, _doc) = archive_state();
        super::super::ui_state(&mut state).filter = "lod".to_string();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let find_button = |state: &mut AppState, n: usize| -> Option<egui::Pos2> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 4000.0),
                )),
                time: Some(n as f64 / 30.0),
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| super::super::draw(ui, state));
            out.platform_output.accesskit_update.and_then(|update| {
                update.nodes.iter().find_map(|(_, node)| {
                    let hit = node
                        .label()
                        .is_some_and(|l| l.contains("Search every archive"));
                    let b = node.bounds().filter(|_| hit)?;
                    Some(egui::pos2(
                        ((b.x0 + b.x1) / 2.0) as f32,
                        ((b.y0 + b.y1) / 2.0) as f32,
                    ))
                })
            })
        };
        let _ = find_button(&mut state, 0);
        let at = find_button(&mut state, 1).expect("a non-empty filter offers the escalation");
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        for (n, events) in [
            vec![egui::Event::PointerMoved(at), press(true)],
            vec![press(false)],
        ]
        .into_iter()
        .enumerate()
        {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 4000.0),
                )),
                time: Some((n + 2) as f64 / 30.0),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| super::super::draw(ui, &mut state));
        }
        let hierarchy = super::super::ui_state(&mut state);
        assert!(hierarchy.deep, "the click switched the search on");
        assert_eq!(hierarchy.filter, "**/*lod*");
    }

    #[test]
    fn a_reveal_opens_the_folders_above_its_row_and_is_consumed() {
        let (mut state, doc) = archive_state();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0);
        let target = id(&state, doc, "props/deep/x.ydr");
        assert!(
            !drawn(&mut state, doc).contains(&target),
            "starts collapsed"
        );

        state.navigate_to(Location { doc, node: target });
        let mut frames = 0;
        while state.navigation.reveal.is_some() && frames < 20 {
            frames += 1;
            frame(&ctx, &mut state, frames);
        }
        assert!(
            state.navigation.reveal.is_none(),
            "the tree reached the row"
        );
        assert!(
            frames <= 10,
            "took {frames} frames, and gives up at {REVEAL_PATIENCE}"
        );
        let rows = drawn(&mut state, doc);
        assert!(rows.contains(&id(&state, doc, "props/deep")));
        assert!(rows.contains(&target));
    }
}
