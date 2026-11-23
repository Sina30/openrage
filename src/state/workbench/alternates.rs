//! The edits the Wardrobe makes to the sets of alternate variations, each one step of Undo.

use or_wardrobe::alternate::{self, Asset};
use or_wardrobe::Ped;

use crate::state::AppState;

/// One change to the sets of alternate variations of a pack.
#[derive(Debug, Clone, PartialEq)]
pub enum AlternatesEdit {
    /// A new set for `ped`; `join` is a garment that causes it from the start.
    New {
        ped: Ped,
        name: String,
        join: Option<String>,
    },
    Rename {
        set: String,
        name: String,
    },
    Remove {
        set: String,
    },
    /// Make the drawable `target` wear alternate `alt`.
    AddSwitch {
        set: String,
        target: Asset,
        alt: u8,
    },
    RemoveSwitch {
        set: String,
        at: usize,
    },
    Join {
        set: String,
        source: Asset,
    },
    Leave {
        set: String,
        source: Asset,
    },
}

impl AppState {
    /// Change the sets of alternate variations of the pack. Undo takes it back.
    pub fn edit_alternates(&mut self, change: AlternatesEdit) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let step = match &change {
            AlternatesEdit::New { name, .. } => {
                ("alternates", tr!("Made the set {name}", name = name))
            }
            AlternatesEdit::Rename { name, .. } => {
                ("alternates", tr!("Renamed a set to {name}", name = name))
            }
            AlternatesEdit::Remove { .. } => (
                "alternates",
                tr!("Removed a set of alternate variations").to_string(),
            ),
            AlternatesEdit::AddSwitch { .. } => {
                ("alternates", tr!("Added a switch to a set").to_string())
            }
            AlternatesEdit::RemoveSwitch { .. } => {
                ("alternates", tr!("Took a switch out of a set").to_string())
            }
            AlternatesEdit::Join { .. } => (
                "alternates",
                tr!("Made a garment change the drawables of a set").to_string(),
            ),
            AlternatesEdit::Leave { .. } => (
                "alternates",
                tr!("Stopped a garment changing the drawables of a set").to_string(),
            ),
        };
        self.pack_edit(
            tr!("Changing alternate variations"),
            step,
            &module,
            false,
            move |w| {
                match change {
                    AlternatesEdit::New { ped, name, join } => {
                        let id = alternate::new_set(w, ped, &name)?;
                        if let Some(garment) = join {
                            alternate::join(w, &id, Asset::garment(&garment))?;
                        }
                    }
                    AlternatesEdit::Rename { set, name } => alternate::rename_set(w, &set, &name)?,
                    AlternatesEdit::Remove { set } => alternate::remove_set(w, &set)?,
                    AlternatesEdit::AddSwitch { set, target, alt } => {
                        alternate::add_switch(w, &set, target, alt)?
                    }
                    AlternatesEdit::RemoveSwitch { set, at } => {
                        alternate::remove_switch(w, &set, at)?
                    }
                    AlternatesEdit::Join { set, source } => alternate::join(w, &set, source)?,
                    AlternatesEdit::Leave { set, source } => alternate::leave(w, &set, &source)?,
                }
                Ok(String::new())
            },
        );
    }
}
