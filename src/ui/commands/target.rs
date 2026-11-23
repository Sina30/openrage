//! What a command acts on: the rows, results or document a context menu was
//! opened on, or else what the focused panel has selected.

use crate::state::{AppState, DocPayload, DocumentId, DocumentKind, RpfNodeId};
use crate::ui::panels::inspector::InspectorUiState;
use crate::ui::panels::registry::{details_are_the_files_fields, ids};
use crate::ui::panels::{hierarchy, search::RESULTS};

/// What a command acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Rows of an install, folder or archive, the one clicked first, and the
    /// folder a paste or a new row goes into; `centre` when they are the Files view's.
    Rows {
        doc: DocumentId,
        rows: Vec<RpfNodeId>,
        into: RpfNodeId,
        centre: bool,
    },
    /// The records ticked in a metadata file's form.
    Records(DocumentId),
    /// Rows of a result list, the one clicked first.
    Hits { list: Hits, rows: Vec<usize> },
    /// A document, by its tab or its row in Files.
    Document(DocumentId),
    /// A row of a particle file's outline, by its selection id.
    Particle { doc: DocumentId, id: u64 },
    /// The garments picked in the Wardrobe's list, by id.
    Garments(Vec<String>),
    /// A texture a document's Textures list names, which may lie in another document.
    Texture { doc: DocumentId, name: String },
}

/// The result list a hit is a row of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hits {
    /// The Search panel's.
    Search,
    /// The search of every archive a document's filter in Files runs.
    Inside(DocumentId),
}

pub const NOTHING_SELECTED: &str =
    tr_key!("Click the files in Files or in the centre, or a metadata file's form, first");

impl Target {
    /// `rows` of `doc`, which go into the first when it is a folder and else into the folder holding it.
    pub fn rows(
        state: &AppState,
        doc: DocumentId,
        rows: Vec<RpfNodeId>,
        centre: bool,
    ) -> Option<Self> {
        let into = state.drop_folder_for(doc, *rows.first()?)?;
        Some(Self::Rows {
            doc,
            rows,
            into,
            centre,
        })
    }
}

/// What the focused panel has selected.
pub fn selection(state: &AppState) -> Result<Target, &'static str> {
    let open = |kind: fn(&DocPayload) -> bool| {
        state
            .active_document
            .filter(|&doc| state.document(doc).is_some_and(|d| kind(&d.payload)))
    };
    let is_meta = |payload: &DocPayload| matches!(payload, DocPayload::Meta(_));
    let is_tree = |payload: &DocPayload| matches!(payload, DocPayload::Rpf(_));
    match state.workspace_docks.focused(state.active_workspace) {
        Some(ids::FILES) => match open(is_tree).filter(|&doc| searched_inside(state, doc)) {
            Some(doc) => hits(&state.deep_find.selection, Hits::Inside(doc)),
            None => in_files(state),
        },
        Some(ids::DOCUMENT | ids::META_EDITOR) if open(is_meta).is_some() => open(is_meta)
            .map(Target::Records)
            .ok_or(crate::i18n::tr(NOTHING_SELECTED)),
        Some(ids::DOCUMENT) => {
            let doc = open(is_tree).ok_or(crate::i18n::tr(NOTHING_SELECTED))?;
            let kept =
                hierarchy::ui_state_ref(state).and_then(|ui| ui.open_folder.get(&doc).copied());
            let into = state
                .files_view_folder(doc, kept)
                .ok_or(crate::i18n::tr(NOTHING_SELECTED))?;
            let mut rows = state.selection.selected_ids(doc);
            rows.retain(|&row| row != into);
            Ok(Target::Rows {
                doc,
                rows,
                into,
                centre: true,
            })
        }
        Some(ids::SEARCH) => hits(&state.search.selection, Hits::Search),
        Some(ids::OUTLINE) => {
            let doc = open(|p| {
                matches!(
                    p,
                    DocPayload::Resource(
                        or_io::Asset::Particles(_) | or_io::Asset::ParticlesGen9(_)
                    )
                )
            })
            .ok_or(crate::i18n::tr(NOTHING_SELECTED))?;
            let id = (state.selection.selected(doc))
                .ok_or("Select an effect or a rule in the Outline first")?;
            Ok(Target::Particle { doc, id })
        }
        Some(ids::WARDROBE) => match state.picked_garments() {
            garments if garments.is_empty() => Err(tr!("Select a garment in the Wardrobe first")),
            garments => Ok(Target::Garments(garments)),
        },
        Some(ids::DETAILS) if details_are_the_files_fields(state) => {
            Err(tr!("Right-click a field in Details to copy or paste it"))
        }
        _ => Err(crate::i18n::tr(NOTHING_SELECTED)),
    }
}

/// The active document.
pub fn document(state: &AppState) -> Result<Target, &'static str> {
    (state.active_document)
        .map(Target::Document)
        .ok_or(tr!("No document is open"))
}

/// What the focused panel has selected, else the active document.
pub fn selection_or_document(state: &AppState) -> Result<Target, &'static str> {
    selection(state).or_else(|_| document(state))
}

/// What the focused panel has selected, else the active document when it is an Open Folder and no row is chosen.
pub fn selection_or_folder_opened(state: &AppState) -> Result<Target, &'static str> {
    let selected = selection(state);
    let none_chosen = match &selected {
        Ok(Target::Rows { rows, .. }) => rows.is_empty(),
        Ok(_) => false,
        Err(_) => true,
    };
    let opened = (state.active_document).filter(|&doc| {
        state
            .document(doc)
            .is_some_and(|d| d.kind == DocumentKind::Folder)
    });
    match opened {
        Some(doc) if none_chosen => Ok(Target::Document(doc)),
        _ => selected,
    }
}

/// The document the focused panel shows, which Undo, Redo and the History panel act on: a pinned inspector's, the archive or folder Files shows, else the active one; the History panel, focused itself, follows the panel it took focus from.
pub fn focused_document(state: &AppState) -> Option<DocumentId> {
    let (docks, ws) = (&state.workspace_docks, state.active_workspace);
    let panel = match docks.focused(ws) {
        Some(ids::HISTORY) => docks.focused_before(ws),
        panel => panel,
    };
    let pinned = (state.panels.peek::<InspectorUiState>()).and_then(|inspector| inspector.pinned);
    match (panel, pinned) {
        (Some(ids::DETAILS), Some((doc, _)))
            if state.document(doc).is_some() && details_are_the_files_fields(state) =>
        {
            Some(doc)
        }
        (Some(ids::FILES), _) => shown_in_files(state),
        _ => state.active_document,
    }
}

/// The install, folder or archive Files shows: the only one it lists, else the one holding its selection.
///
/// ⚠ The selection can lie outside every tree it lists: a model's part picked in the Outline before a click on Files' empty space.
fn shown_in_files(state: &AppState) -> Option<DocumentId> {
    let trees: Vec<DocumentId> = (state.documents.iter())
        .filter(|d| d.rpf_entry_binding.is_none() && matches!(d.payload, DocPayload::Rpf(_)))
        .map(|d| d.id)
        .collect();
    let selected = (state.selection.current()).map(|(doc, _)| doc);
    match trees[..] {
        [only] => Some(only),
        _ => selected.filter(|doc| trees.contains(doc)),
    }
}

/// The rows of the tree in Files that are selected.
fn in_files(state: &AppState) -> Result<Target, &'static str> {
    let unselected = tr!("Select a file or folder in Files first");
    let (doc, focus) = (state.selection.current())
        .filter(|&(doc, _)| {
            (state.document(doc)).is_some_and(|d| matches!(d.payload, DocPayload::Rpf(_)))
        })
        .ok_or(unselected)?;
    let into = state.drop_folder_for(doc, focus).ok_or(unselected)?;
    Ok(Target::Rows {
        doc,
        rows: state.selection.selected_ids(doc),
        into,
        centre: false,
    })
}

/// Whether Files lists `doc`'s search of every archive in place of its tree.
fn searched_inside(state: &AppState, doc: DocumentId) -> bool {
    hierarchy::ui_state_ref(state).is_some_and(|ui| {
        let pattern = ui.filter.trim();
        ui.deep && state.deep_find.found_for(doc, pattern).is_some()
    })
}

fn hits(selection: &crate::state::Selection, list: Hits) -> Result<Target, &'static str> {
    let rows: Vec<usize> = (selection.selected_ids(RESULTS).into_iter())
        .map(|row| row as usize)
        .collect();
    if rows.is_empty() {
        return Err(tr!("Select a result first"));
    }
    Ok(Target::Hits { list, rows })
}
