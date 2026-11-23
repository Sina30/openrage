//! An error's title says which operation failed.

use super::{AppState, LoadResult, SaveResult};

/// Every label a frame of the editor's overlays draws.
fn overlays(state: &mut AppState) -> Vec<String> {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut labels = Vec::new();
    // ⚠ A window's first frame only sizes it.
    for _ in 0..2 {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::app::modals::draw_editor_overlays(ui.ctx(), state);
        });
        labels = output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| node.label().or_else(|| node.value()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
    }
    labels
}

#[test]
fn a_failed_save_is_titled_as_a_save_and_a_failed_open_as_an_open() {
    let mut state = AppState::default();

    let (tx, receiver) = std::sync::mpsc::channel();
    state.save_receiver = Some(super::file_io::SaveJob {
        path: "vehicle.ydr".into(),
        cancel: or_job::Cancel::new(),
        receiver,
    });
    tx.send(SaveResult::Error {
        message: "the disk is full".into(),
    })
    .expect("the save's result");
    state.poll_save_result();
    let shown = overlays(&mut state);
    assert!(
        shown.iter().any(|label| label == "Could not save the file"),
        "a failed save does not say it was a save: {shown:?}"
    );
    assert!(
        !shown.iter().any(|label| label.contains("load")),
        "a failed save reads as a failed load: {shown:?}"
    );
    state.clear_error();

    let (tx, rx) = std::sync::mpsc::channel();
    state.load_receiver = Some(("vehicle.ydr".into(), rx));
    tx.send(LoadResult::Error {
        path: "vehicle.ydr".into(),
        message: "not a drawable".into(),
    })
    .expect("the load's result");
    state.poll_load_result();
    let shown = overlays(&mut state);
    assert!(
        shown
            .iter()
            .any(|label| label == "Could not open vehicle.ydr"),
        "a failed open does not say it was an open: {shown:?}"
    );
}
