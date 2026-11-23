//! What a build changed since the last one, by stable id, and the redirect map of what moved.
//!
//! A build records the items it shipped in a hidden `.<resource>.openrage-items.json` beside the
//! resource, so the next plan can say what differs before anything is written.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::contribution::Item;
use crate::plan::Plan;
use crate::project::replace_file;

/// The difference between the items of two builds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<Item>,
    /// Items whose content differs, as they are now; one that also moved is here as well.
    pub changed: Vec<Item>,
    pub moved: Vec<Moved>,
    pub removed: Vec<Item>,
}

/// An item whose name differs, which saved outfits and shop tables that hold the old one must follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub id: String,
    pub from: String,
    pub to: String,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.changed.is_empty()
            && self.moved.is_empty()
            && self.removed.is_empty()
    }

    /// The redirect map as a canonical JSON document: every move, old name to new name, by id.
    ///
    /// ⚠ The keys come out sorted because `serde_json` keeps an object in a `BTreeMap`; a build
    /// that turns on its `preserve_order` feature writes them in the order given here instead.
    pub fn redirect_map(&self) -> String {
        let redirects: Vec<Value> = self
            .moved
            .iter()
            .map(|m| json!({ "id": m.id, "from": m.from, "to": m.to }))
            .collect();
        let map = json!({ "format": REDIRECT_FORMAT, "schema": 1, "redirects": redirects });
        let mut text = serde_json::to_string_pretty(&map).expect("a JSON value serialises");
        text.push('\n');
        text
    }
}

const REDIRECT_FORMAT: &str = "openrage-redirects";
const RECORD_FORMAT: &str = "openrage-build-items";

/// What differs between the items of the last build and of the next, by id and never by name.
pub fn diff(previous: &[Item], current: &[Item]) -> Diff {
    let by_id = |items: &[Item]| -> BTreeMap<String, Item> {
        items.iter().map(|i| (i.id.clone(), i.clone())).collect()
    };
    let (old, new) = (by_id(previous), by_id(current));
    let mut found = Diff::default();
    for (id, now) in &new {
        let Some(was) = old.get(id) else {
            found.added.push(now.clone());
            continue;
        };
        if was.name != now.name {
            found.moved.push(Moved {
                id: id.clone(),
                from: was.name.clone(),
                to: now.name.clone(),
            });
        }
        if was.digest != now.digest {
            found.changed.push(now.clone());
        }
    }
    found.removed.extend(
        old.iter()
            .filter(|(id, _)| !new.contains_key(*id))
            .map(|(_, was)| was.clone()),
    );
    found
}

/// The diff `plan` would make from the build that is in `root` now, or none when that build recorded nothing.
pub fn since_last_build(plan: &Plan, root: &Path) -> Option<Diff> {
    recorded(root, &plan.resource).map(|previous| diff(&previous, &plan.items))
}

pub(crate) fn record_path(root: &Path, resource: &str) -> PathBuf {
    root.join(format!(".{resource}.openrage-items.json"))
}

/// The items the build of `resource` in `root` recorded, or none when there is no readable record.
pub(crate) fn recorded(root: &Path, resource: &str) -> Option<Vec<Item>> {
    let bytes = std::fs::read(record_path(root, resource)).ok()?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    if record.get("format")? != RECORD_FORMAT {
        return None;
    }
    record
        .get("items")?
        .as_array()?
        .iter()
        .map(|entry| {
            let text = |key: &str| entry.get(key)?.as_str().map(str::to_string);
            Some(Item {
                id: text("id")?,
                name: text("name")?,
                digest: text("digest")?,
            })
        })
        .collect()
}

/// Record the items of the build of `resource` just swapped into `root`.
pub(crate) fn record(root: &Path, resource: &str, items: &[Item]) -> io::Result<()> {
    let items: Vec<Value> = items
        .iter()
        .map(|i| json!({ "id": i.id, "name": i.name, "digest": i.digest }))
        .collect();
    let record = json!({ "format": RECORD_FORMAT, "schema": 1, "items": items });
    let mut text = serde_json::to_string_pretty(&record).map_err(io::Error::other)?;
    text.push('\n');
    replace_file(&record_path(root, resource), text.as_bytes())
}
