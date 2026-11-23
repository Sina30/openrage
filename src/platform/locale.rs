//! The language the system shows its own interface in, as the code of the language alone: `fr` for French.

/// The system's language, or none in a test build and a self-check, which draw the same whatever the machine speaks.
pub fn system_language() -> Option<String> {
    if super::standing_in() {
        return None;
    }
    real().map(|tag| primary(&tag))
}

/// `fr` of `fr-FR`, `fr_FR.UTF-8` and `FR`.
fn primary(tag: &str) -> String {
    tag.split(['-', '_', '.', '@'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn real() -> Option<String> {
    use windows_sys::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};

    /// `LOCALE_NAME_MAX_LENGTH`, in UTF-16 units with the end.
    const LONGEST: usize = 85;
    let mut name = [0u16; LONGEST];
    // SAFETY: takes no pointer and reads the user's setting.
    let language = unsafe { GetUserDefaultUILanguage() };
    // SAFETY: `name` holds `LONGEST` units, which is what it is told, and the call writes no more than that.
    let written =
        unsafe { LCIDToLocaleName(u32::from(language), name.as_mut_ptr(), LONGEST as i32, 0) };
    let written = usize::try_from(written).ok().filter(|n| *n > 1)?;
    Some(String::from_utf16_lossy(&name[..written - 1]))
}

#[cfg(not(windows))]
fn real() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .into_iter()
        .filter_map(|variable| std::env::var(variable).ok())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_tag_is_reduced_to_its_language() {
        for (tag, language) in [
            ("fr-FR", "fr"),
            ("fr_CA.UTF-8", "fr"),
            ("EN", "en"),
            ("de@euro", "de"),
            ("", ""),
        ] {
            assert_eq!(primary(tag), language, "{tag}");
        }
    }

    /// The call into the system answers with a tag, whatever the machine speaks.
    #[cfg(windows)]
    #[test]
    fn the_system_answers_with_a_language_tag() {
        let tag = real().expect("Windows names its display language");
        assert!(tag.len() >= 2, "{tag:?}");
    }
}
