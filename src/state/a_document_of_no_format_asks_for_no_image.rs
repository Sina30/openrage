//! A folder and a game root, whose kinds declare no format, are described by nothing and ask for
//! no image they never hold, wherever the description is asked for, so the Log panel stays clear.

use std::sync::Arc;

use super::the_inspector_leads_with_a_summary::drawn_by;
use super::{described_root, AppState, DocumentId};
use crate::diagnostics::log_buffer::LogBuffer;
use crate::ui::panels::inspector;

/// The warnings and errors logged while `doc` is described, drawn in both panels that show a document's fields, and the frame's edits are sealed.
fn logged_describing(state: &mut AppState, doc: DocumentId) -> Vec<String> {
    state.set_active_document(doc);
    let buffer = Arc::new(LogBuffer::default());
    let subscriber = crate::diagnostics::subscriber(Arc::clone(&buffer), std::io::sink);
    tracing::subscriber::with_default(subscriber, || {
        let described = state.document(doc).and_then(described_root);
        assert!(described.is_none(), "a kind with no format was described");
        drawn_by(state, inspector::draw);
        drawn_by(state, inspector::draw_body);
        state.seal_edits();
    });
    let snapshot = buffer.snapshot();
    (snapshot.records.iter())
        .filter(|record| record.is_serious())
        .map(|record| record.line())
        .collect()
}

#[test]
fn a_folder_and_a_game_root_are_described_and_drawn_with_no_warning_about_an_image_logged() {
    let folder = tempfile::tempdir().expect("a scratch folder");
    let install = tempfile::tempdir().expect("a scratch install");
    std::fs::write(install.path().join("gta5.exe"), b"").expect("the marker an install has");
    let mut state = super::comparing_two_files::state();
    state.open_folder(folder.path().to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let game = state.open_game_root(install.path().to_path_buf());

    for (what, doc) in [("a folder", folder), ("a game root", game)] {
        assert_eq!(
            logged_describing(&mut state, doc),
            Vec::<String>::new(),
            "{what} logged"
        );
    }
}
