//! The data file that defines peds to the game, read for the names it defines them under.
//!
//! The engine ships it as it is and writes none of it: the game's structure for a ped's data
//! (`CPedModelInfo__InitDataList`) is not declared by a typed root here, so a field of it would be
//! written from memory. It reads one thing, the `Name` of each entry, to hold the model's peds to it.

use std::collections::BTreeSet;
use std::path::Path;

use crate::model::AddonPeds;

/// The names a data file defines peds under, lower case, or why it cannot be read.
pub type Entries = Result<BTreeSet<String>, String>;

/// The text of every `<Name>` element of an XML data file, lower case.
///
/// ⚠ Text, not a tree: the file reloads through the metadata reader before it is read for names
/// (the facts do that), and an element called `Name` is how an entry of a ped's data states its model.
pub fn names_in(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("<Name>") {
        let after = &rest[start + "<Name>".len()..];
        let Some(end) = after.find("</Name>") else {
            break;
        };
        let name = after[..end].trim();
        if !name.is_empty() {
            names.insert(name.to_lowercase());
        }
        rest = &after[end + "</Name>".len()..];
    }
    names
}

/// The entries of the data file `model` states, read in `folder`; none when it states none.
pub fn entries(folder: &Path, model: &AddonPeds) -> Option<Entries> {
    let path = model.meta.as_deref()?;
    if !crate::facts::stays_inside(path) {
        return Some(Err(format!("`{path}` leaves the module's folder")));
    }
    let at = folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    Some(match std::fs::read(&at) {
        Err(e) => Err(format!("`{path}` cannot be read: {e}")),
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Ok(names_in(&text)),
            Err(_) => Err(format!(
                "`{path}` is not text, so the names it defines peds under cannot be read; an XML data file is"
            )),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_of_each_entry_is_read_and_nothing_else_called_a_name() {
        let text = "<CPedModelInfo__InitDataList>\n<InitDatas>\n<Item><Name>My_Ped</Name><PropsName>null</PropsName></Item>\n<Item>\n  <Name> other_ped </Name>\n</Item>\n</InitDatas>\n</CPedModelInfo__InitDataList>";
        assert_eq!(
            names_in(text).into_iter().collect::<Vec<_>>(),
            ["my_ped", "other_ped"]
        );
        assert!(names_in("<Name></Name><Name>unclosed").is_empty());
    }
}
