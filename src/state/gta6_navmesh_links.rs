//! The special links of the open bodies of a scanned GTA VI cell's neighbours, made again in the
//! step of an edit that renumbers the cell's records, and written beside the cell by its save.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::container::rpf::Rpf9Sections;
use or_io::gta6::headerless::{relinked, with_links, write_links, NavMesh, NavRebuild};

use crate::state::document::{DocPayload, Document};
use crate::state::edit_history::Swap;
use crate::state::extract_job::ExtractItem;
use crate::state::gta6_edit::{self, EditSections};
use crate::state::{gta6_navmesh, rpf9_scan};

/// Per cell an edit was made in, each neighbouring body it renumbered, as it opened.
type Relinks = BTreeMap<PathBuf, BTreeMap<PathBuf, Arc<NavMesh>>>;

// ponytail: an entry per cell whose edit renumbered a neighbour, emptied when the cell opens again.
static RELINKS: Mutex<Relinks> = Mutex::new(BTreeMap::new());

fn relinks() -> MutexGuard<'static, Relinks> {
    RELINKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The cell whose edit holds the body at `path` with its special links renumbered; `None` where
/// none does.
pub(in crate::state) fn renumbered_by(path: &Path) -> Option<PathBuf> {
    let held: Vec<(PathBuf, Arc<NavMesh>)> = (relinks().iter())
        .filter_map(|(cell, bodies)| Some((cell.clone(), Arc::clone(bodies.get(path)?))))
        .collect();
    if held.is_empty() {
        return None;
    }
    let now = gta6_navmesh::scanned_cell(path)?;
    (held.into_iter())
        .find(|(_, opened)| opened.special_links != now.special_links)
        .map(|(cell, _)| cell)
}

/// Put back each body the edits of the cell at `path` renumbered, whose own body opens again.
pub(in crate::state) fn forget(path: &Path) {
    let Some(bodies) = relinks().remove(path) else {
        return;
    };
    for (body, opened) in bodies {
        gta6_navmesh::replace(&body, opened);
    }
}

/// Each open neighbouring body of the cell at `path` whose special links `made`, made from `cell`,
/// renumbers, with them made again and tried on its stored bytes; why not, where one is refused.
pub(in crate::state) fn planned(
    path: &Path,
    cell: &NavMesh,
    made: &NavRebuild,
) -> Result<Vec<(PathBuf, NavMesh)>, String> {
    let mut out = Vec::new();
    for (body_path, body) in gta6_navmesh::neighbour_paths(path, cell) {
        let Some(links) = relinked(&body, cell, made).map_err(|e| e.to_string())? else {
            continue;
        };
        if renumbered_by(&body_path).is_some_and(|by| by != path) {
            return Err(tr!(
                "The special links of the neighbouring cell {cell} were renumbered by an edit of another cell, whose save writes them: save that cell or undo its edit first.",
                cell = body.cell
            ));
        }
        let after = with_links(&body, links).map_err(|e| e.to_string())?;
        // ponytail: the body is read on the UI thread, as the cell's own deletion is tried.
        if let Some((archive, offset)) = rpf9_scan::scanned_body(&body_path) {
            let links = after.special_links.clone();
            let edit = move |_, s: &mut Rpf9Sections| {
                (write_links(s, links.clone()).map(drop)).map_err(|e| e.to_string())
            };
            gta6_edit::written(archive, offset, &edit)?;
        }
        out.push((body_path, after));
    }
    Ok(out)
}

/// The bodies `planned` gave kept as the cells the inspector reads, each as a part of the step of
/// the cell at `path` that puts it back.
pub(in crate::state) fn apply(path: &Path, planned: Vec<(PathBuf, NavMesh)>) -> Vec<Box<dyn Swap>> {
    let mut parts: Vec<Box<dyn Swap>> = Vec::new();
    for (body, after) in planned {
        let Some(held) = gta6_navmesh::replace(&body, Arc::new(after)) else {
            continue;
        };
        (relinks().entry(path.to_path_buf()).or_default())
            .entry(body.clone())
            .or_insert_with(|| Arc::clone(&held));
        parts.push(Box::new(BodyPart {
            path: body,
            cell: held,
        }));
    }
    parts
}

/// A neighbouring body's cell as the inspector reads it, which an undo puts back.
struct BodyPart {
    path: PathBuf,
    cell: Arc<NavMesh>,
}

impl Swap for BodyPart {
    fn swap(&mut self, _: &mut DocPayload) -> bool {
        let Some(now) = gta6_navmesh::replace(&self.path, Arc::clone(&self.cell)) else {
            return false;
        };
        self.cell = now;
        true
    }
}

/// Each neighbouring body the edits of the cell `document` shows hold renumbered, read again on
/// the worker with those links written over it, as `<offset>.rsc`.
pub(in crate::state) fn beside(document: &Document) -> Vec<ExtractItem> {
    let Some(path) = document.path.as_deref() else {
        return Vec::new();
    };
    let held: Vec<(PathBuf, Arc<NavMesh>)> = (relinks().get(path).into_iter())
        .flat_map(|bodies| bodies.iter().map(|(p, m)| (p.clone(), Arc::clone(m))))
        .collect();
    let mut items = Vec::new();
    for (body, opened) in held {
        let Some(now) = gta6_navmesh::scanned_cell(&body) else {
            continue;
        };
        let Some((archive, offset)) = rpf9_scan::scanned_body(&body) else {
            continue;
        };
        if now.special_links == opened.special_links {
            continue;
        }
        let links = now.special_links.clone();
        let edit: Arc<EditSections> = Arc::new(move |_, sections| {
            (write_links(sections, links.clone()).map(drop)).map_err(|e| e.to_string())
        });
        let name = format!("{offset:#X}.rsc");
        items.push(ExtractItem::edited(
            name,
            archive.to_path_buf(),
            offset,
            edit,
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use or_io::gta6::headerless::{read_navmesh, SpecialLink};

    use super::*;
    use crate::state::opening_an_rpf9_archive::navmesh_cell;

    fn cell() -> NavMesh {
        read_navmesh(&navmesh_cell()).expect("the cell reads")
    }

    #[test]
    fn a_renumbered_body_is_held_by_its_cell_until_undone_or_the_cell_opens_again() {
        let (edited, body) = (
            PathBuf::from("a_renumbered_body/0x1010.cell"),
            PathBuf::from("a_renumbered_body/0x2020.cell"),
        );
        crate::state::gta6_navmesh::remember(&body, cell());
        assert_eq!(renumbered_by(&body), None);
        let mut after = cell();
        after.special_links.push(SpecialLink {
            from: [0; 3],
            to: [0; 3],
            from_polygon: 0,
            to_polygon: 0,
            from_mesh: after.cell,
            to_mesh: after.cell + 1,
            word_18: 0,
            heading: 0,
        });
        let mut parts = apply(&edited, vec![(body.clone(), after)]);
        assert_eq!(parts.len(), 1);
        assert_eq!(renumbered_by(&body), Some(edited.clone()));
        // Opened again, the body keeps the links the edit holds.
        crate::state::gta6_navmesh::remember(&body, cell());
        let links =
            |p: &Path| crate::state::gta6_navmesh::scanned_cell(p).map(|c| c.special_links.len());
        assert_eq!(links(&body), Some(1));

        let mut payload = DocPayload::Resource(or_io::Asset::NavMesh(Default::default()));
        assert!(parts[0].swap(&mut payload), "undone");
        assert_eq!((links(&body), renumbered_by(&body)), (Some(0), None));
        assert!(parts[0].swap(&mut payload), "made again");
        assert_eq!(renumbered_by(&body), Some(edited.clone()));
        forget(&edited);
        assert_eq!((links(&body), renumbered_by(&body)), (Some(0), None));
        assert!(apply(
            &edited,
            vec![(PathBuf::from("no body is kept here"), cell())]
        )
        .is_empty());
    }
}
