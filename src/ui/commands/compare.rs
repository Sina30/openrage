//! The commands that compare a file or archive of Files or of the Files view
//! with another, or one in the mods overlay with the game's own copy.

use super::table::{Action, On, Spec, BASE};
use super::target::{self, Target};
use crate::state::{AppState, DocumentId, RpfNodeId};

pub(super) const SELECT_FOR_COMPARE: Spec = Spec {
    action: Action::SelectForCompare,
    id: "select_for_compare",
    label: tr_key!("Select for compare"),
    category: tr_key!("Files"),
    hover: tr_key!("Remember this file or archive, to compare another with it"),
    on: Some(On {
        focused: target::selection,
        enabled: |state, target| file(state, target).map(|_| ()),
        label: None,
        run: |state, _, target| {
            if let Ok((doc, row)) = file(state, target) {
                state.select_for_compare(doc, row);
            }
        },
    }),
    ..BASE
};

pub(super) const COMPARE_WITH_SELECTED: Spec = Spec {
    action: Action::CompareWithSelected,
    id: "compare_with_selected",
    label: tr_key!("Compare with selected"),
    category: tr_key!("Files"),
    hover: tr_key!(
        "Show how this file differs from the one Select for compare chose: a resource by part \
            and field, binary metadata by record and field, a texture dictionary by texture, an \
            archive by entry, text and XML line by line"
    ),
    on: Some(On {
        focused: target::selection,
        enabled: |state, target| {
            let (doc, row) = file(state, target)?;
            state.compares_with_selected(doc, row)
        },
        label: Some(|state, _| match state.selected_for_compare() {
            Some(name) => tr!("Compare with {name}", name = name),
            None => tr!("Compare with selected").to_string(),
        }),
        run: |state, _, target| {
            if let Ok((doc, row)) = file(state, target) {
                state.compare_with_selected(doc, row);
            }
        },
    }),
    ..BASE
};

pub(super) const COMPARE_WITH_ORIGINAL: Spec = Spec {
    action: Action::CompareWithOriginal,
    id: "compare_with_original",
    label: tr_key!("Compare with original"),
    category: tr_key!("Files"),
    hover: tr_key!(
        "Show what the mod changed: this file or archive in the mods overlay beside the game's \
            own copy"
    ),
    on: Some(On {
        focused: target::selection,
        enabled: |state, target| {
            let (doc, row) = file(state, target)?;
            state.compares_with_original(doc, row)
        },
        label: None,
        run: |state, _, target| {
            if let Ok((doc, row)) = file(state, target) {
                state.compare_with_original(doc, row);
            }
        },
    }),
    ..BASE
};

/// The one file or archive row `target` names.
fn file(state: &AppState, target: &Target) -> Result<(DocumentId, RpfNodeId), &'static str> {
    let (doc, node) = super::files::row(state, target)?;
    state.compares(doc, node.id).map(|()| (doc, node.id))
}
