//! A bulk edit on a query: what it would change, said before anything is written, and made as one step of Undo.
//!
//! The garments a query selects are the garments the edit acts on, and the edit is one the Garment panel makes to one
//! garment ([`Edit`]). [`dry_run`] changes a copy of the pack and says, for each garment, what differs; [`apply`] writes the pack once
//! and returns the [`Revision`] that takes all of it back. An edit that one garment refuses is refused for all of them, as
//! [`crate::edit::apply_all`] does, and the dry run says which refused and why.

use std::path::Path;

use crate::changes::garment_lines;
use crate::edit::{apply, apply_all, revise, Edit, Error, Revision};
use crate::facts::inspect;
use crate::model::Wardrobe;
use crate::query::Query;
use crate::rules::name_of;
use crate::Depth;

/// One garment a bulk edit changes, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub id: String,
    pub name: String,
    /// What differs, one line each; a garment taken out says so.
    pub lines: Vec<String>,
}

/// What a bulk edit would do, and nothing of it done.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DryRun {
    /// How many garments the edit acts on.
    pub selected: usize,
    /// The garments it changes.
    pub changed: Vec<Changed>,
    /// The garments it would leave as they are, such as one that has the tag already.
    pub unchanged: usize,
    /// The garments that refuse it, with the reason; while there is one, the edit is made for none.
    pub refused: Vec<(String, String)>,
}

impl DryRun {
    /// The sentence that says what the edit does, for a button's hover and a dialog's first line.
    pub fn sentence(&self) -> String {
        let garments = |n: usize| match n {
            1 => "1 garment".to_string(),
            n => format!("{n} garments"),
        };
        if self.selected == 0 {
            return "The query selects no garment, so there is nothing to change".to_string();
        }
        if let Some((name, why)) = self.refused.first() {
            return format!(
                "{} refuse{} it, so none is changed: {name} {why}",
                garments(self.refused.len()),
                if self.refused.len() == 1 { "s" } else { "" }
            );
        }
        match self.changed.len() {
            0 if self.selected == 1 => {
                "Would change nothing: the garment selected already states it".to_string()
            }
            0 => format!(
                "Would change nothing: the {} selected already state it",
                garments(self.selected)
            ),
            1 => "Would change the garment selected".to_string(),
            n if n == self.selected => format!("Would change all {}", garments(n)),
            n => format!(
                "Would change {n} of the {} selected",
                garments(self.selected)
            ),
        }
    }

    /// Whether the edit can be made and makes a change.
    pub fn is_worth_making(&self) -> bool {
        self.refused.is_empty() && !self.changed.is_empty()
    }
}

/// What `edit` would do to the garments `ids` of `wardrobe`, which is left as it is.
pub fn dry_run(wardrobe: &Wardrobe, ids: &[String], edit: &Edit) -> DryRun {
    let mut run = DryRun::default();
    // One copy that each garment is changed in turn, as the edit itself changes the pack, so a long list costs a pass and not one copy of the pack for each garment.
    let mut copy = wardrobe.clone();
    let mut seen = std::collections::HashSet::new();
    for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
        let Some(before) = wardrobe.garment(id) else {
            run.refused
                .push((id.clone(), "is not in the pack any more".to_string()));
            continue;
        };
        run.selected += 1;
        let name = name_of(before);
        if let Err(why) = apply(&mut copy, id, edit.clone()) {
            run.refused.push((name, why.to_string()));
            continue;
        }
        let lines = match copy.garment(id) {
            Some(after) => garment_lines(before, after),
            None => vec!["is taken out of the pack".to_string()],
        };
        match lines.is_empty() {
            true => run.unchanged += 1,
            false => run.changed.push(Changed {
                id: id.clone(),
                name,
                lines,
            }),
        }
    }
    run
}

/// The ids of the garments the query `text` selects in the pack in `folder`, as the Wardrobe's list shows them.
pub fn ids_of(folder: &Path, text: &str) -> Result<Vec<String>, Error> {
    let query = Query::parse(text).map_err(Error::Refused)?;
    let wardrobe = Wardrobe::read(folder)?;
    let facts = inspect(folder, &wardrobe, Depth::Measure);
    let planned = crate::plan::plan(&wardrobe, &facts, "wardrobe", folder);
    let subject = crate::query::Subject::new(
        &wardrobe,
        &planned.allocation,
        &planned.findings,
        &planned.counts,
    );
    Ok(query.ids(&subject))
}

/// Make `edit` to the garments `ids` of the pack in `folder`, all or none, and keep the step that takes it back whole; none when nothing changed.
pub fn make(
    folder: &Path,
    ids: &[String],
    edit: Edit,
) -> Result<(DryRun, Option<Revision>), Error> {
    let seen = dry_run(&Wardrobe::read(folder)?, ids, &edit);
    if let Some((name, why)) = seen.refused.first() {
        return Err(Error::Refused(format!("{name} {why}")));
    }
    let ((), revision) = revise(folder, |w| apply_all(w, ids, edit))?;
    Ok((seen, revision))
}
