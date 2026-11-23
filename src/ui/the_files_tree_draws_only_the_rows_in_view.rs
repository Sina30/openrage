//! Files lists every row of an archive and draws the ones in view: a frame's cost follows the
//! window's height, not the archive's size.

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::names;
use crate::state::navigation::Location;
use crate::state::{DocPayload, DocumentId, DocumentKind, RpfNodeId, ThemeChoice};
use crate::ui::dock::Workspace;
use crate::ui::theme;

const FILES: usize = 2000;

/// An archive of [`FILES`] files open in Browse, and its last file's row.
fn a_long_archive(shell: &mut Shell) -> (DocumentId, RpfNodeId) {
    shell.state.documents.clear();
    let archive = shell.state.new_empty_document(DocumentKind::Rpf);
    let Some(DocPayload::Rpf(rpf)) = shell.state.document_mut(archive).map(|d| &mut d.payload)
    else {
        panic!("an empty archive document holds an archive");
    };
    let root = rpf.session.root_id();
    let mut last = root;
    for n in 0..FILES {
        let name = format!("row_{n:04}.ydr");
        last = (rpf.session.add_file_copy(root, &name, vec![0; 8])).expect("a file in the archive");
    }
    shell.state.set_active_document(archive);
    shell.state.active_workspace = Workspace::Browse;
    (archive, last)
}

fn rows_drawn(shell: &Shell) -> usize {
    (names(shell).iter())
        .filter(|name| name.contains("row_"))
        .count()
}

#[test]
fn a_long_archive_lists_every_row_and_draws_the_ones_in_view() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let (archive, _) = a_long_archive(&mut shell);
    shell.settled();

    let listed = crate::ui::panels::hierarchy::ui_state(&mut shell.state)
        .visible_rows
        .get(&archive)
        .map_or(0, Vec::len);
    assert_eq!(
        listed, FILES,
        "a shift-click spans the rows out of view too"
    );
    let drawn = rows_drawn(&shell);
    assert!(drawn > 0, "{:?}", names(&shell));
    assert!(drawn < FILES / 4, "{drawn} of {FILES} rows were drawn");
}

#[test]
fn a_row_out_of_view_is_scrolled_to_when_it_is_revealed() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let (archive, last) = a_long_archive(&mut shell);
    shell.settled();
    let name = format!("row_{:04}.ydr", FILES - 1);
    let shown = |shell: &Shell| names(shell).iter().any(|n| n.ends_with(&name));
    assert!(!shown(&shell), "the last row is in view before the reveal");

    shell.state.navigation.reveal = Some(Location {
        doc: archive,
        node: last,
    });
    // The area scrolls to the row over a few frames.
    for _ in 0..120 {
        shell.draw(Vec::new());
        if shown(&shell) {
            break;
        }
    }
    assert!(shown(&shell), "{:?}", names(&shell));
    assert_eq!(shell.state.navigation.reveal, None);
}
