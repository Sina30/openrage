pub fn convert_datatype_to_friendly_name(s: &str) -> String {
    let mut t = s.replace("__", "::");
    while t.starts_with("::") {
        t.drain(..2);
    }
    t
}

pub fn consistent_struct_short_name(s: &str) -> String {
    let trimmed = s.trim_start_matches(':');
    trimmed.replace("::", "__")
}

/// ⚠ The `m_` comes off the **front** and nowhere else. A global replace
/// turns `m_CTX1_from_DXT1` into `CTX1_froDXT1`, which then hashes to a
/// number no document stores. 2 of the 33,999 members in the 1,134-file PSC
/// corpus carry a second `m_`.
pub fn normalize_member_name(s: &str) -> String {
    s.strip_prefix("m_").unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ The name is what the member's hash is taken over, so a mangled one
    /// resolves to nothing in any document. `m_CTX1_from_DXT1` became
    /// `CTX1_froDXT1` under a global replace.
    #[test]
    fn only_the_leading_m_comes_off_a_member_name() {
        assert_eq!(normalize_member_name("m_Name"), "Name");
        assert_eq!(normalize_member_name("m_CTX1_from_DXT1"), "CTX1_from_DXT1");
        assert_eq!(normalize_member_name("m_atom_count"), "atom_count");
        assert_eq!(normalize_member_name("m_m_Twice"), "m_Twice");
        assert_eq!(normalize_member_name("Name"), "Name");
        assert_eq!(normalize_member_name("aim_Offset"), "aim_Offset");
        assert_eq!(normalize_member_name(""), "");
    }

    #[test]
    fn friendly_name_double_underscore_to_colons() {
        assert_eq!(
            convert_datatype_to_friendly_name("rage__game__Foo"),
            "rage::game::Foo"
        );
    }

    #[test]
    fn friendly_name_strips_leading_scope() {
        assert_eq!(
            convert_datatype_to_friendly_name("::__rage__Foo"),
            "rage::Foo"
        );
    }

    #[test]
    fn friendly_name_plain_unchanged() {
        assert_eq!(
            convert_datatype_to_friendly_name("CAmbientModel"),
            "CAmbientModel"
        );
    }

    #[test]
    fn member_name_only_when_prefixed() {
        assert_eq!(normalize_member_name("foo_m_bar"), "foo_m_bar");
        assert_eq!(normalize_member_name("Probability"), "Probability");
    }

    #[test]
    fn consistent_short_name() {
        assert_eq!(
            consistent_struct_short_name("::rage::evtInstancePrintf"),
            "rage__evtInstancePrintf"
        );
    }
}
