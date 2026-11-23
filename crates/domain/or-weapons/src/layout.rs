//! Where the files of a weapon land inside a resource, and the type the game is told a weapon's data file is.
//!
//! A model or a texture source cooks to `stream/<name>.<extension>`, as every streamed file does. A data source, the text `.meta` a
//! weapon is defined in, ships at [`DATA_FOLDER`] and is registered by that path, since the game reads a data file by the path it
//! is registered under and not by where it streams from (ruling V3 of the views design).

/// The folder FiveM streams from.
pub const STREAM: &str = "stream";

/// The folder of a resource at which a data source of the weapons content ships and a build registers it by that path.
pub const DATA_FOLDER: &str = "data/";

/// The type the game is told a weapon file is, by the root the document states.
///
/// ⚠ Read off the registrations of the install: every file of the install whose root is one of these is registered with the
/// type beside it (`a_weapon_file_registers_under_the_type_its_root_names`), and the animations, loadouts, pickups and shops that
/// the same folders hold are other roots and other types, which this content does not write.
pub const DATA_FILE_TYPES: &[(&str, &str)] = &[
    ("CWeaponInfoBlob", "WEAPONINFO_FILE"),
    ("CWeaponComponentInfoBlob", "WEAPONCOMPONENTSINFO_FILE"),
    ("CWeaponModelInfo__InitDataList", "WEAPON_METADATA_FILE"),
];

/// The type a document of the root `root` is registered as, none for a root this content does not know.
pub fn data_file_type(root: &str) -> Option<&'static str> {
    DATA_FILE_TYPES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(root))
        .map(|(_, ty)| *ty)
}

/// Where a cooked model or texture called `file` (`w_pi_pistol.ydr`) lands inside the resource.
pub fn stream_path(file: &str) -> String {
    format!("{STREAM}/{file}")
}

/// Where the data source cooked to `file` (`weapons.meta`) lands inside the resource.
pub fn data_path(file: &str) -> String {
    format!("{DATA_FOLDER}{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_has_a_type_and_the_folder_of_the_data_ends_in_a_slash() {
        assert_eq!(data_file_type("CWeaponInfoBlob"), Some("WEAPONINFO_FILE"));
        assert_eq!(data_file_type("cweaponinfoblob"), Some("WEAPONINFO_FILE"));
        assert_eq!(data_file_type("CPickupDataManager"), None);
        assert!(DATA_FOLDER.ends_with('/') && !DATA_FOLDER.starts_with('/'));
        assert_eq!(data_path("weapons.meta"), "data/weapons.meta");
        assert_eq!(stream_path("w_pi.ydr"), "stream/w_pi.ydr");
    }
}
