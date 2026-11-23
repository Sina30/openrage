//! The directory the app and `openrage-cli` keep their state in: preferences, the keys cache, the
//! key snapshot, the shader pack and plugins.
//!
//! ⚠ One function names it, so a run redirected with `OPENRAGE_CONFIG_DIR` is redirected for the
//! app and the CLI alike. The CLI used to ask `dirs` for itself and ignored the variable in four of
//! its five places, so a CLI run meant to be contained read the owner's real preferences.

use std::ffi::OsString;
use std::path::PathBuf;

/// The variable that redirects the configuration directory.
pub const CONFIG_DIR_VARIABLE: &str = "OPENRAGE_CONFIG_DIR";

/// The directory `OPENRAGE_CONFIG_DIR` names, when it names one.
///
/// ⚠ An empty value is unset, not the current directory: PowerShell deletes a variable set to an
/// empty string, so the two have to mean the same thing on every platform.
pub fn config_dir_override() -> Option<PathBuf> {
    from_variable(std::env::var_os(CONFIG_DIR_VARIABLE))
}

fn from_variable(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|dir| !dir.is_empty()).map(PathBuf::from)
}

/// Where a real run keeps its state: `openrage` under the platform's configuration directory,
/// `%APPDATA%` on Windows, `~/.config` on Linux and `~/Library/Application Support` on macOS.
pub fn user_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("openrage"))
}

/// The configuration directory of this run: the override, else the user's.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_override().or_else(user_config_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_variable_is_the_directory_and_an_empty_one_is_unset() {
        assert_eq!(
            from_variable(Some("/tmp/x".into())),
            Some(PathBuf::from("/tmp/x"))
        );
        assert_eq!(from_variable(Some(OsString::new())), None);
        assert_eq!(from_variable(None), None);
    }

    #[test]
    fn the_user_directory_is_openrage_under_the_platforms_own() {
        let user = user_config_dir().expect("a platform with a configuration directory");
        assert_eq!(user.file_name().and_then(|n| n.to_str()), Some("openrage"));
        assert_eq!(user.parent(), dirs::config_dir().as_deref());
    }
}
