//! Cut, Copy, Paste and Delete act on what was clicked, else on what the
//! focused panel has selected: the rows of the Files tree or of the Files view
//! in the centre, the records ticked in a metadata file's form, or for Delete
//! an effect or a rule in a particle file's outline.

use super::table::On;
use super::target::{self, Target};
use crate::state::AppState;
use crate::ui::panels::hierarchy;

const NO_RECORD: &str = tr_key!("Tick a record's box in the form first");

/// Refuses a target with nothing in it to act on.
fn chosen(state: &AppState, target: &Target) -> Result<(), &'static str> {
    match target {
        Target::Rows { rows, .. } if rows.is_empty() => Err(tr!("Select a file or folder first")),
        Target::Records(doc) if state.records_selected.of(*doc).is_none() => {
            Err(crate::i18n::tr(NO_RECORD))
        }
        Target::Rows { .. } | Target::Records(_) => Ok(()),
        _ => Err(target::NOTHING_SELECTED),
    }
}

fn can_cut(state: &AppState, target: &Target) -> Result<(), &'static str> {
    chosen(state, target)?;
    match target {
        Target::Records(_) => Err(tr!(
            "Records are copied: paste them, then delete the originals"
        )),
        _ => Ok(()),
    }
}

fn can_paste(state: &AppState, target: &Target) -> Result<(), &'static str> {
    match target {
        Target::Rows { .. } if state.rows_to_paste() => Ok(()),
        Target::Rows { .. } => Err(crate::i18n::tr(NOTHING_TO_PASTE)),
        Target::Records(_) if !state.records_to_paste() => Err(tr!("Copy records first")),
        Target::Records(doc) if state.records_selected.of(*doc).is_none() => {
            Err(tr!("Tick the record to paste after first"))
        }
        Target::Records(_) => Ok(()),
        _ => Err(target::NOTHING_SELECTED),
    }
}

pub const NOTHING_TO_PASTE: &str =
    tr_key!("Nothing to paste: copy or cut files first, here or in Explorer");

fn can_delete(state: &AppState, target: &Target) -> Result<(), &'static str> {
    match target {
        Target::Particle { id, .. } if hierarchy::particle_row_deletes(*id) => Ok(()),
        Target::Particle { .. } => Err(tr!("Select an effect or a rule in the Outline first")),
        Target::Rows { rows, .. } if rows.len() > 1 => Err(tr!(
            "Several rows cannot be deleted at once yet: select one"
        )),
        Target::Garments(garments) => super::wardrobe::can_remove_garments(state, garments),
        _ => chosen(state, target),
    }
}

fn said(state: &mut AppState, done: Result<String, String>) {
    match done {
        Ok(said) => state.show_info_toast(said),
        Err(e) => state.show_error_toast(e),
    }
}

fn copy(state: &mut AppState, target: &Target, cut: bool) {
    let done = match target {
        Target::Rows { doc, rows, .. } => state.copy_rows(*doc, rows, cut),
        Target::Records(doc) if !cut => state.copy_records(*doc),
        _ => return,
    };
    said(state, done);
}

fn paste(state: &mut AppState, target: &Target) {
    match *target {
        Target::Rows { doc, into, .. } => {
            if let Err(e) = state.paste_rows(doc, into) {
                state.show_error_toast(e);
            }
        }
        Target::Records(doc) => {
            let done = state.paste_records(doc);
            said(state, done);
        }
        _ => {}
    }
}

fn delete(state: &mut AppState, target: &Target) {
    match target {
        Target::Rows { doc, rows, .. } => {
            if let [row] = rows[..] {
                if let Err(e) = state.rpf_delete_node(*doc, row) {
                    state.show_error_toast(e);
                }
            }
        }
        &Target::Records(doc) => {
            let done = state.delete_records(doc);
            said(state, done);
        }
        &Target::Particle { doc, id } => hierarchy::delete_particle_row(state, doc, id),
        Target::Garments(garments) => state.remove_garments(garments.clone()),
        _ => {}
    }
}

pub(super) const CUT: On = On {
    focused: target::selection,
    enabled: can_cut,
    label: None,
    run: |state, _, target| copy(state, target, true),
};

pub(super) const COPY: On = On {
    focused: target::selection,
    enabled: chosen,
    label: None,
    run: |state, _, target| copy(state, target, false),
};

pub(super) const PASTE: On = On {
    focused: target::selection,
    enabled: can_paste,
    label: None,
    run: |state, _, target| paste(state, target),
};

pub(super) const DELETE: On = On {
    focused: target::selection,
    enabled: can_delete,
    label: None,
    run: |state, _, target| delete(state, target),
};
